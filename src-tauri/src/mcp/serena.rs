use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, Tool},
    service::RunningService,
    transport::StreamableHttpClientTransport,
};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
// A dropped/failed request makes the backend session unusable until reactivation.
struct InFlight(Option<rmcp::service::RunningServiceCancellationToken>);
impl Drop for InFlight {
    fn drop(&mut self) {
        if let Some(token) = self.0.take() {
            token.cancel();
        }
    }
}
pub struct Client {
    service: RunningService<RoleClient, ()>,
    pub tools: Vec<Tool>,
}
impl Client {
    pub async fn connect(port: u16) -> Result<Self, String> {
        let transport =
            StreamableHttpClientTransport::from_uri(format!("http://127.0.0.1:{port}/mcp"));
        let service = tokio::time::timeout(Duration::from_secs(15), ().serve(transport))
            .await
            .map_err(|_| "TOOL_TIMEOUT")?
            .map_err(|e| format!("BACKEND_UNAVAILABLE: {e}"))?;
        let mut client = Self {
            service,
            tools: Vec::new(),
        };
        let tools = tokio::time::timeout(Duration::from_secs(15), client.service.list_all_tools())
            .await
            .map_err(|_| "TOOL_TIMEOUT")?
            .map_err(|e| e.to_string())?;
        for name in super::registry::SOURCES
            .iter()
            .map(|t| t.1)
            .chain(["activate_project", "get_current_config"])
        {
            let tool = tools
                .iter()
                .find(|t| t.name == name)
                .ok_or_else(|| format!("BACKEND_INCOMPATIBLE: missing {name}"))?;
            let mut params: Vec<&str> = super::registry::SOURCES
                .iter()
                .find(|t| t.1 == name)
                .map(|t| t.2.iter().copied().filter(|p| *p != "max_bytes").collect())
                .unwrap_or_default();
            if name == "activate_project" {
                params.push("project");
            }
            if super::registry::SOURCES.iter().any(|t| t.1 == name) && name != "find_file" {
                params.push("max_answer_chars");
            }
            for param in params {
                if tool
                    .input_schema
                    .get("properties")
                    .and_then(|p| p.get(param))
                    .is_none()
                {
                    return Err(format!("BACKEND_INCOMPATIBLE: missing {name}.{param}"));
                }
            }
        }
        client.tools = tools;
        Ok(client)
    }
    pub async fn call(&self, name: &str, args: Value) -> Result<String, String> {
        let mut flight = InFlight(Some(self.service.cancellation_token()));
        let result = tokio::time::timeout(
            Duration::from_secs(45),
            self.service.call_tool(
                CallToolRequestParams::new(name.to_owned())
                    .with_arguments(args.as_object().cloned().unwrap_or_default()),
            ),
        )
        .await
        .map_err(|_| "TOOL_TIMEOUT")?
        .map_err(|e| format!("BACKEND_UNAVAILABLE: {e}"))?;
        flight.0 = None;
        let value = serde_json::to_value(result).map_err(|e| e.to_string())?;
        if value["isError"] == true {
            return Err(format!("BACKEND_ERROR: {}", value["content"]));
        }
        let content = value["content"]
            .as_array()
            .ok_or("BACKEND_INCOMPATIBLE: 无文本结果")?;
        let mut texts = Vec::new();
        for item in content {
            if item["type"] != "text" {
                return Err("BACKEND_INCOMPATIBLE: 仅接受文本/JSON 工具结果".into());
            }
            texts.push(item["text"].as_str().ok_or("BACKEND_INCOMPATIBLE")?);
        }
        let text = texts.join("\n");
        if text.starts_with("The answer is too long") {
            return Err("OUTPUT_LIMIT_EXCEEDED: 请缩小查询范围".into());
        }
        Ok(text)
    }
    pub async fn activate(&self, root: &Path) -> Result<(), String> {
        let result = self
            .call("activate_project", json!({"project":display(root)}))
            .await?;
        // v1.7.0 activation header contains the actual root, unlike get_current_config (name only).
        let expected = format!(" at {} is activated.", display(root));
        let first = result.lines().next().unwrap_or_default().replace('\\', "/");
        let known = first.starts_with("The project with name '")
            && first.ends_with(&expected.replace('\\', "/"));
        let created = first.starts_with("Created and activated a new project with name '")
            && first.ends_with(&format!(" at {}.", display(root)).replace('\\', "/"));
        if !known && !created {
            return Err(format!(
                "BACKEND_INCOMPATIBLE: 无法验证 Serena 激活 root: {result}"
            ));
        }
        self.call("get_current_config", json!({})).await?;
        Ok(())
    }
    pub fn closed(&self) -> bool {
        self.service.is_closed()
    }
}
pub fn display(path: &Path) -> String {
    let s = path.to_string_lossy();
    s.strip_prefix("\\\\?\\").unwrap_or(&s).to_owned()
}
