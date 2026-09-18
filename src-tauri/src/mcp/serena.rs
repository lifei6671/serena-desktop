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
        if !activation_header_matches_root(result.lines().next().unwrap_or_default(), root) {
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

/// 将 Windows verbatim 路径和普通路径统一为 Serena 回执可比较的形式。
fn normalized_root(path: &str) -> String {
    let path = path.replace('\\', "/");
    if let Some(unc) = path.strip_prefix("//?/UNC/") {
        format!("//{unc}")
    } else {
        path.strip_prefix("//?/").unwrap_or(&path).to_owned()
    }
}

/// 按目标平台的文件系统语义比较已统一格式的 Root；Windows 仅放宽 ASCII 大小写。
fn normalized_roots_match(actual: &str, expected: &str, windows_semantics: bool) -> bool {
    if windows_semantics {
        actual.eq_ignore_ascii_case(expected)
    } else {
        actual == expected
    }
}

/// 仅接受 Serena 两种完整成功回执，并逐字比较其中的实际 root。
fn activation_header_matches_root(header: &str, root: &Path) -> bool {
    const EXISTING_PREFIX: &str = "The project with name '";
    const EXISTING_SUFFIX: &str = " is activated.";
    const CREATED_PREFIX: &str = "Created and activated a new project with name '";
    const CREATED_SUFFIX: &str = ".";

    let actual_root = [
        (EXISTING_PREFIX, EXISTING_SUFFIX),
        (CREATED_PREFIX, CREATED_SUFFIX),
    ]
    .into_iter()
    .find_map(|(prefix, suffix)| {
        header
            .strip_prefix(prefix)?
            .strip_suffix(suffix)?
            .rsplit_once("' at ")
            .map(|(_, path)| path)
    });
    actual_root.is_some_and(|actual| {
        normalized_roots_match(
            &normalized_root(actual),
            &normalized_root(&display(root)),
            cfg!(windows),
        )
    })
}

/// 为 CLI 仅移除 Windows verbatim 表示，并保留 UNC 根的双前导反斜杠。
pub fn display(path: &Path) -> String {
    let s = path.to_string_lossy();
    if let Some(unc) = s.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{unc}")
    } else {
        s.strip_prefix("\\\\?\\").unwrap_or(&s).to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_header_accepts_normal_windows_root() {
        let root = Path::new(r"C:\workspace\project");
        assert!(activation_header_matches_root(
            "The project with name 'project' at C:/workspace/project is activated.",
            root
        ));
    }

    #[test]
    fn activation_header_normalizes_verbatim_actual_and_expected_roots() {
        let root = Path::new(r"\\?\C:\workspace\project");
        assert!(activation_header_matches_root(
            r"The project with name 'project' at \\?\C:\workspace\project is activated.",
            root
        ));
    }

    #[test]
    fn activation_header_normalizes_slash_verbatim_root_in_header_middle() {
        let root = Path::new(r"C:\workspace\project");
        assert!(activation_header_matches_root(
            "The project with name 'project' at //?/C:/workspace/project is activated.",
            root
        ));
    }

    #[test]
    fn display_normalizes_verbatim_unc_without_changing_other_windows_roots() {
        assert_eq!(display(Path::new(r"\\?\C:\workspace")), r"C:\workspace");
        assert_eq!(
            display(Path::new(r"\\?\UNC\server\share\workspace")),
            r"\\server\share\workspace"
        );
        assert_eq!(
            display(Path::new(r"\\server\share\workspace")),
            r"\\server\share\workspace"
        );
        assert_eq!(display(Path::new(r"C:\workspace")), r"C:\workspace");
    }

    #[test]
    fn normalized_root_normalizes_verbatim_unc_without_changing_ordinary_unc() {
        assert_eq!(normalized_root("//?/C:/workspace"), "C:/workspace");
        assert_eq!(
            normalized_root("//?/UNC/server/share/workspace"),
            "//server/share/workspace"
        );
        assert_eq!(
            normalized_root("//server/share/workspace"),
            "//server/share/workspace"
        );
    }

    #[test]
    fn activation_header_matches_verbatim_and_ordinary_unc_roots() {
        let verbatim_unc = Path::new(r"\\?\UNC\server\share\workspace");
        let ordinary_unc = Path::new(r"\\server\share\workspace");
        let header = r"The project with name 'workspace' at \\server\share\workspace is activated.";
        assert!(activation_header_matches_root(header, verbatim_unc));
        assert!(activation_header_matches_root(header, ordinary_unc));
    }

    #[test]
    fn windows_root_identity_ignores_case_after_verbatim_and_slash_normalization() {
        assert!(normalized_roots_match(
            &normalized_root("c:/workspace/project"),
            &normalized_root(r"\\?\C:\Workspace\Project"),
            true,
        ));
        assert!(!normalized_roots_match(
            &normalized_root("c:/workspace/other"),
            &normalized_root(r"C:\Workspace\Project"),
            true,
        ));
        assert!(!normalized_roots_match(
            &normalized_root("c:/workspace/project"),
            &normalized_root(r"C:\Workspace\Project"),
            false,
        ));
    }

    #[test]
    fn activation_header_accepts_created_project() {
        let root = Path::new(r"C:\workspace\project");
        assert!(activation_header_matches_root(
            "Created and activated a new project with name 'project' at C:/workspace/project.",
            root
        ));
    }

    #[test]
    fn activation_header_rejects_different_root() {
        let root = Path::new(r"C:\workspace\project");
        assert!(!activation_header_matches_root(
            "The project with name 'project' at C:/workspace/other is activated.",
            root
        ));
    }
}
