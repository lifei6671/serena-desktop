//! Public MCP transport for the Command Runtime. Product/runtime semantics live in crate::command.

use crate::command::{CommandEnvelope, ExecuteRequest, QueryRequest};
use rmcp::{
    model::{Tool, ToolAnnotations},
    schemars::{self, JsonSchema},
};
use serde_json::Value;

pub const NAMES: [&str; 2] = ["command_query", "command_execute"];

pub fn contains(name: &str) -> bool {
    NAMES.contains(&name)
}

fn schema<T: JsonSchema>() -> Value {
    let mut value = serde_json::to_value(schemars::schema_for!(T))
        .expect("command schema serialization");
    value["type"] = serde_json::json!("object");
    value
}

fn output_schema() -> Value {
    let mut value = serde_json::to_value(
        schemars::generate::SchemaSettings::default()
            .for_serialize()
            .into_generator()
            .into_root_schema_for::<CommandEnvelope>(),
    )
    .expect("command output schema serialization");
    value["type"] = serde_json::json!("object");
    value
}

pub fn descriptors() -> Vec<Tool> {
    [
        (
            "command_query",
            "【做什么】\n查询或观察受管 CommandRun，包括状态、列表、增量 stdout/stderr。\n\n【什么时候使用】\n命令已启动后查询结果；长任务使用 observe，输出按 cursor 增量读取。\n\n【关键约束】\n只读；output cursor 由调用方持有，不会消费其他客户端的输出。observe 最长等待 20 秒。Command Receipt 可重复读取。",
            schema::<QueryRequest>(),
            true,
            false,
        ),
        (
            "command_execute",
            "【做什么】\n在显式 workspaceId 对应的已登记 Workspace 中启动或取消受管命令。\n\n【什么时候使用】\n需要由 Serena Desktop Host 真实执行构建、测试、脚本或其他开发命令，并获取 exit code 与可持久化 Command Receipt 时使用。\n\n【关键约束】\nstart 必须显式携带 workspaceId 与 requestKey；cwd 只允许 Workspace-relative。process 模式执行 PATH 中的原生命令，shell 模式支持 PowerShell/cmd 组合语义。命令以当前桌面用户权限运行，Workspace 边界不等于 OS Sandbox。Remote 工具仅在本机显式授权后公开。",
            schema::<ExecuteRequest>(),
            false,
            true,
        ),
    ]
    .into_iter()
    .map(|(name, description, input, read_only, open_world)| {
        let mut tool = Tool::new(name, description, input.as_object().unwrap().clone());
        tool.annotations = Some(
            ToolAnnotations::default()
                .read_only(read_only)
                .destructive(!read_only)
                .idempotent(read_only)
                .open_world(open_world),
        );
        let output = output_schema();
        tool.output_schema = Some(output.as_object().unwrap().clone().into());
        tool
    })
    .collect()
}

pub fn validate(name: &str, args: &Value) -> Result<(), String> {
    match name {
        "command_query" => serde_json::from_value::<QueryRequest>(args.clone()).map(|_| ()),
        "command_execute" => serde_json::from_value::<ExecuteRequest>(args.clone()).map(|_| ()),
        _ => return Err("UNKNOWN_TOOL".into()),
    }
    .map_err(|_| "COMMAND_INVALID_ARGUMENT".into())
}

pub fn parse_query(args: Value) -> Result<QueryRequest, String> {
    serde_json::from_value(args).map_err(|_| "COMMAND_INVALID_ARGUMENT".into())
}

pub fn parse_execute(args: Value) -> Result<ExecuteRequest, String> {
    serde_json::from_value(args).map_err(|_| "COMMAND_INVALID_ARGUMENT".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn descriptors_are_closed_typed_contracts() {
        let descriptors = descriptors();
        assert_eq!(descriptors.len(), 2);
        for descriptor in descriptors {
            assert_eq!(descriptor.input_schema["type"], "object");
            assert!(descriptor.output_schema.is_some());
        }
    }

    #[test]
    fn execute_rejects_absolute_cwd_and_unknown_fields_at_dto_boundary() {
        assert!(validate(
            "command_execute",
            &json!({
                "action":"start",
                "workspaceId":"w",
                "requestKey":"r",
                "spec":{"mode":"process","executable":"cargo","args":["test"]},
                "relativeCwd":"src",
                "unexpected":true
            })
        ).is_err());
    }
}
