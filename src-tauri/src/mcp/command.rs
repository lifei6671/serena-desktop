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
    let mut value =
        serde_json::to_value(schemars::schema_for!(T)).expect("command schema serialization");
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
            "【做什么】\n查询或观察受管 CommandRun，包括状态、列表、增量 stdout/stderr。\n\n【什么时候使用】\nget(commandRunId) 读取单个 Run；list(workspaceId?/workRunId?/limit?) 查询列表；observe(commandRunId, knownRevision?, waitMs?) 有界等待状态变化；output(commandRunId, stdoutCursor?, stderrCursor?, maxOutputBytes?) 增量读取输出。\n\n【关键约束】\n只读；stdoutCursor 与 stderrCursor 由调用方分别持有，读取不会消费其他客户端的输出。observe 的 waitMs 范围 0..=20000；不传 knownRevision 时只返回即时 snapshot，要有界等待必须把上次返回的 revision 原样传回。stdout/stderr 只在 bounded in-memory retention 内可读取，首次 retained 输出可从 cursor 0 开始，后续使用 nextCursor；retained=false 表示正文已不可恢复，但 Command Receipt、总字节数和哈希仍可查询。长命令不依赖单个 MCP 调用持续到进程结束。",
            schema::<QueryRequest>(),
            true,
            false,
        ),
        (
            "command_execute",
            "【做什么】\n在显式 workspaceId 对应的已登记 Workspace 中启动或取消受管命令。\n\n【什么时候使用】\nstart(workspaceId, requestKey, spec, ...) 启动已确定的 Git 写操作、构建、测试、包管理、脚本或本地诊断；cancel(commandRunId) 取消已有 Run。具体 executable/args 或 shell command 已确定时优先使用，可获取 exit code 和 Command Receipt。\n\n【关键约束】\nstart 必须显式携带 workspaceId 与 requestKey；可选 relativeCwd 只能是 Workspace-relative 目录。spec.mode=process 时 executable 只能是由受管 PATH 解析的命令名，args 为独立 argv；spec.mode=shell 时使用 spec.command。macOS Shell 以账户 Shell 的 -c 执行，不加载 profile、alias 或 shell function；子进程只继承受控环境键并叠加显式 env，可通过 env.PATH 覆盖本次 Run 的 PATH。executionMode=sync 也只在单次 MCP 调用的有界等待窗口内等待，未终态时通过 command_query observe/output 继续。命令以当前桌面用户权限运行，Workspace 边界不等于 OS Sandbox。Remote 工具仅在本机显式授权后公开。如果任务还需要自主读代码、修改实现并根据结果迭代，应使用 agent_execute。",
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
    fn descriptions_remain_action_specific_when_union_schema_is_not_projected() {
        let descriptors = descriptors();
        let query = descriptors
            .iter()
            .find(|tool| tool.name == "command_query")
            .and_then(|tool| tool.description.as_deref())
            .unwrap();
        for phrase in [
            "get(commandRunId)",
            "list(workspaceId?/workRunId?/limit?)",
            "observe(commandRunId, knownRevision?, waitMs?)",
            "output(commandRunId, stdoutCursor?, stderrCursor?, maxOutputBytes?)",
            "waitMs 范围 0..=20000",
        ] {
            assert!(query.contains(phrase), "command_query: {phrase}");
        }

        let execute = descriptors
            .iter()
            .find(|tool| tool.name == "command_execute")
            .and_then(|tool| tool.description.as_deref())
            .unwrap();
        for phrase in [
            "start(workspaceId, requestKey, spec, ...)",
            "cancel(commandRunId)",
            "relativeCwd",
            "spec.mode=process",
            "env.PATH",
            "executionMode=sync",
            "command_query observe/output",
        ] {
            assert!(execute.contains(phrase), "command_execute: {phrase}");
        }
    }

    #[test]
    fn execute_rejects_absolute_cwd_and_unknown_fields_at_dto_boundary() {
        assert!(
            validate(
                "command_execute",
                &json!({
                    "action":"start",
                    "workspaceId":"w",
                    "requestKey":"r",
                    "spec":{"mode":"process","executable":"cargo","args":["test"]},
                    "relativeCwd":"src",
                    "unexpected":true
                })
            )
            .is_err()
        );
    }
}
