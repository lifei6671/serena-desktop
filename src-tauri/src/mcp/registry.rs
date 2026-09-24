use super::source_write_domain::{SourceWriteError, SourceWriteTool};
use crate::{
    serena::SupervisorState,
    workspace_resolver::{WorkspaceLease, WorkspaceResolver},
};
use rmcp::{
    model::{Tool, ToolAnnotations},
    schemars::{self, JsonSchema},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceArgs {
    #[serde(default)]
    pub relative_path: Option<String>,
    #[serde(default)]
    pub start_line: Option<u32>,
    #[serde(default)]
    pub end_line: Option<u32>,
    #[serde(default)]
    pub recursive: Option<bool>,
    #[serde(default)]
    pub file_mask: Option<String>,
    #[serde(default)]
    pub substring_pattern: Option<String>,
    #[serde(default)]
    pub name_path_pattern: Option<String>,
    #[serde(default)]
    pub name_path: Option<String>,
    #[serde(default)]
    pub depth: Option<u32>,
    #[serde(default)]
    pub include_body: Option<bool>,
    #[serde(default)]
    pub max_bytes: Option<usize>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActivateArgs {
    pub id: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceIdArgs {
    pub workspace_id: String,
}
/// CodeGraph 公开查询只接受请求级 Workspace identity 和既有查询语义。
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code, reason = "schema and validation own these fields")]
pub struct CodeGraphExploreArgs {
    pub workspace_id: String,
    pub query: String,
    #[serde(default)]
    pub max_files: Option<u32>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
pub const SOURCES: &[(&str, &[&str], &[&str])] = &[
    (
        "source_read_file",
        &["relative_path", "start_line", "end_line", "max_bytes"],
        &["relative_path"],
    ),
    (
        "source_list_dir",
        &["relative_path", "recursive", "max_bytes"],
        &["relative_path"],
    ),
    (
        "source_find_file",
        &["relative_path", "file_mask", "max_bytes"],
        &["file_mask"],
    ),
    (
        "source_search_pattern",
        &["relative_path", "substring_pattern", "max_bytes"],
        &["substring_pattern"],
    ),
    (
        "source_symbols_overview",
        &["relative_path", "depth", "max_bytes"],
        &["relative_path"],
    ),
    (
        "source_find_symbol",
        &[
            "relative_path",
            "name_path_pattern",
            "depth",
            "include_body",
            "max_bytes",
        ],
        &["name_path_pattern"],
    ),
    (
        "source_find_references",
        &["relative_path", "name_path", "max_bytes"],
        &["relative_path", "name_path"],
    ),
];
/// 已迁移到 Workspace Capability 的 Serena Semantic public tool。
pub(crate) const SEMANTIC_SOURCES: &[&str] = &[
    "source_symbols_overview",
    "source_find_symbol",
    "source_find_references",
];

/// 已迁移到本地 Rust 的基础 Source Tool，仍必须使用 request-resolved WorkspaceLease。
pub(crate) const LOCAL_SOURCES: &[&str] = &[
    "source_read_file",
    "source_list_dir",
    "source_find_file",
    "source_search_pattern",
];

/// 判断名称是否属于六个由本地用户统一授权的 Source Write Tool。
pub(crate) fn is_source_write_tool(name: &str) -> bool {
    SourceWriteTool::ALL
        .into_iter()
        .any(|tool| tool.code() == name)
}

/// 在公开与分派边界复用同一 capability 判定，避免旧 catalog 绕过本地开关。
pub(crate) fn authorize_source_write(
    remote_source_write_enabled: bool,
    name: &str,
) -> Result<(), SourceWriteError> {
    if is_source_write_tool(name) && !remote_source_write_enabled {
        return Err(SourceWriteError::WriteRemoteDisabled);
    }
    Ok(())
}

/// 所有公开 Source Tool 都只能由 request-resolved WorkspaceLease 承载 Authority。
pub(crate) fn is_workspace_scoped_source(name: &str) -> bool {
    SEMANTIC_SOURCES.contains(&name) || LOCAL_SOURCES.contains(&name)
}

pub const GITS: &[&str] = &[
    "git_status",
    "git_diff",
    "git_log",
    "git_show",
    "git_branch",
    "git_worktree_list",
];
fn git_fields(name: &str) -> &[&str] {
    match name {
        "git_diff" => &["workspaceId", "scope", "path", "max_bytes"],
        "git_log" => &["workspaceId", "reference", "path", "count", "max_bytes"],
        "git_show" => &["workspaceId", "reference", "path", "max_bytes"],
        _ => &["workspaceId", "max_bytes"],
    }
}
fn schema<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).unwrap()
}
pub(crate) fn parse_workspace_id(args: &Value) -> Result<String, String> {
    let object = args.as_object().ok_or("INVALID_PARAMS")?;
    match object.get("workspaceId") {
        None | Some(Value::Null) => Err("WORKSPACE_CONTEXT_REQUIRED".into()),
        Some(Value::String(id)) if id.trim().is_empty() => {
            Err("INVALID_PARAMS: workspaceId 不能为空".into())
        }
        Some(Value::String(id)) => Ok(id.clone()),
        Some(_) => Err("INVALID_PARAMS: workspaceId 必须是字符串".into()),
    }
}
pub(crate) fn resolve_workspace_lease(
    supervisor: &SupervisorState,
    args: &Value,
) -> Result<WorkspaceLease, String> {
    WorkspaceResolver::new(supervisor).resolve(&parse_workspace_id(args)?)
}
pub(crate) fn workspace_provenance_schema() -> Value {
    json!({
        "type":"object",
        "properties":{"id":{"type":"string"},"generation":{"type":"integer","minimum":0}},
        "required":["id","generation"]
    })
}
fn tool(name: &'static str, desc: &'static str, value: Value) -> Tool {
    let mut t = Tool::new(name, desc, value.as_object().unwrap().clone());
    t.annotations = Some(ToolAnnotations::default().read_only(true));
    let workspace = json!({"type":"object", "properties":{"id":{"type":"string"},"name":{"type":"string"},"root":{"type":"string"}}, "required":["id","name","root"]});
    let registry_workspace = json!({"type":"object", "properties":{"id":{"type":"string"},"name":{"type":"string"},"root":{"type":"string"},"generation":{"type":"integer","minimum":0}}, "required":["id","name","root","generation"]});
    let mut output = json!({"type":"object","properties":{"truncated":{"type":"boolean"}},"required":["truncated"]});
    if name == "workspace_list" {
        output["properties"]["workspaces"] = json!({"type":"array","items":registry_workspace});
        output["properties"]["registryRevision"] = json!({"type":"integer","minimum":0});
        output["required"] = json!(["registryRevision", "workspaces", "truncated"]);
    } else if name == "workspace_get" {
        output["properties"]["workspace"] = registry_workspace;
        output["properties"]["registryRevision"] = json!({"type":"integer","minimum":0});
        output["required"] = json!(["registryRevision", "workspace", "truncated"]);
    } else if name.starts_with("workspace_") {
        output["properties"]["activeWorkspace"] = json!({"anyOf":[workspace,{"type":"null"}]});
        output["properties"]["status"] = json!({"type":"string"});
        output["required"] = json!(["activeWorkspace", "truncated"]);
    } else if GITS.contains(&name) || SOURCES.iter().any(|source| source.0 == name) {
        output["properties"]["workspace"] = workspace_provenance_schema();
        output["properties"]["text"] = json!({"type":"string"});
        output["properties"]["hint"] = json!({"type":"string"});
        output["required"] = json!(["workspace", "text", "truncated"]);
    } else {
        output["properties"]["workspace"] = workspace;
        output["properties"]["text"] = json!({"type":"string"});
        output["properties"]["hint"] = json!({"type":"string"});
        output["required"] = json!(["workspace", "text", "truncated"]);
    }
    if name == "source_read_file" {
        output["properties"]["path"] =
            json!({"type":"string", "description":"Validated workspace-relative file path."});
        output["properties"]["sha256"] = json!({"type":"string", "pattern":"^[0-9a-f]{64}$", "description":"SHA-256 of the complete local file bytes, independent of the returned line range or budget."});
        output["required"] = json!(["workspace", "text", "truncated", "path", "sha256"]);
    }
    if name == "codegraph_explore" {
        // CodeGraph 成功结果只公开 request Lease provenance；通用 Workspace schema 含 root，不能用于 Remote。
        let codegraph_success = json!({"type":"object", "properties":{
            "workspace":workspace_provenance_schema(),
            "text":{"type":"string"},
            "truncated":{"type":"boolean"}
        }, "required":["workspace","text","truncated"]});
        output = json!({"type":"object", "oneOf":[codegraph_success, {
            "type":"object", "required":["error"], "properties":{"error":{
                "type":"object", "required":["code","message","workspace","recoverable"],
                "properties":{"code":{"type":"string"},"message":{"type":"string"},
                    "workspace":{"anyOf":[{"type":"object","required":["id","name"],"properties":{"id":{"type":"string"},"name":{"type":"string"}}},{"type":"null"}]},
                    "recoverable":{"type":"boolean"}}
            }}
        }]});
    }
    t.output_schema = Some(output.as_object().unwrap().clone().into());
    t
}

/// 为所有公开 Source Tool 提供与 Serena 运行状态无关的稳定说明。
fn source_description(name: &str) -> &'static str {
    match name {
        "source_read_file" => {
            "【做什么】\n读取指定 Workspace 中一个文本文件的全部或指定行范围，并返回已验证相对路径、完整文件 SHA-256 与 Workspace provenance。\n\n【什么时候使用】\n需要查看已登记 Workspace 内的源文件或文本配置时使用。\n\n【关键约束】\nworkspaceId 与 relative_path 必填；路径只能相对该 Workspace 根目录且不得越界。只支持 UTF-8 文本，max_bytes 默认 32768、范围 1–131072；超限时返回 truncated。"
        }
        "source_list_dir" => {
            "【做什么】\n列出指定 Workspace 内目录的文件和子目录，可选择递归返回。\n\n【什么时候使用】\n需要浏览某个源码目录的结构、确认文件或子目录时使用；relative_path 传 `.` 可列出该 Workspace 根目录。\n\n【关键约束】\nworkspaceId 与 relative_path 必填；`.` 仅表示该 workspaceId 解析出的 Workspace root，其他路径只能相对该根目录且不得越界。recursive 默认 false；max_bytes 默认 65536、范围 1–262144；超限时返回 truncated。"
        }
        "source_find_file" => {
            "【做什么】\n在指定 Workspace 内按文件名 glob 查找文件，返回 Workspace-relative 路径。\n\n【什么时候使用】\n已知文件名、扩展名或通配模式，需要定位候选文件时使用。\n\n【关键约束】\nworkspaceId 与 file_mask 必填；relative_path 可限制搜索子树。搜索不会跟随 Workspace 外链接，并遵循本地忽略和隐藏文件规则；max_bytes 默认 65536、范围 1–262144。"
        }
        "source_search_pattern" => {
            "【做什么】\n在指定 Workspace 的文本文件中执行逐行、大小写敏感的正则表达式搜索，返回命中的相对路径、0-based 行号和整行内容。\n\n【什么时候使用】\n需要用正则定位某段代码、配置值或文本片段出现位置时使用。\n\n【关键约束】\nworkspaceId 与 substring_pattern 必填；relative_path 可限制搜索子树。只搜索本地可读文本，不跟随 Workspace 外链接，并遵循本地忽略和隐藏文件规则；max_bytes 默认 65536、范围 1–262144。"
        }
        "source_symbols_overview" => {
            "【做什么】\n获取指定 Workspace 文件或目录的语义符号概览。\n\n【什么时候使用】\n需要理解代码结构、顶层符号或模块轮廓时使用。\n\n【关键约束】\nworkspaceId 与 relative_path 必填；语义分析由该 Workspace 的 Serena capability 按需提供。Serena 不可用时调用会返回既有 capability 错误；max_bytes 默认 65536、范围 1–262144。"
        }
        "source_find_symbol" => {
            "【做什么】\n在指定 Workspace 中按符号路径模式查找语义符号。\n\n【什么时候使用】\n需要定位函数、类、方法或其他代码符号时使用。\n\n【关键约束】\nworkspaceId 与 name_path_pattern 必填；relative_path 可限制范围。语义分析由该 Workspace 的 Serena capability 按需提供；Serena 不可用时调用会返回既有 capability 错误；max_bytes 默认 65536、范围 1–262144。"
        }
        "source_find_references" => {
            "【做什么】\n查找指定 Workspace 中某个语义符号的引用。\n\n【什么时候使用】\n需要评估符号影响范围或追踪调用关系时使用。\n\n【关键约束】\nworkspaceId、relative_path 与 name_path 必填。语义分析由该 Workspace 的 Serena capability 按需提供；Serena 不可用时调用会返回既有 capability 错误；max_bytes 默认 65536、范围 1–262144。"
        }
        _ => unreachable!("SOURCES contains only locally described Source tools"),
    }
}
// Stable core only; open execution fields preserve dynamic Product data.
pub(crate) fn agent_output_schema() -> Value {
    let mut schema = json!({
      "type": "object",
      "oneOf": [
        {
          "type": "object",
          "properties": {
            "ok": {
              "const": true
            },
            "data": {
              "oneOf": [
                {
                  "$ref": "#/$defs/execution"
                },
                {
                  "type": "object",
                  "properties": {
                    "executions": {
                      "type": "array",
                      "items": {
                        "$ref": "#/$defs/execution"
                      }
                    }
                  },
                  "required": [
                    "executions"
                  ],
                  "additionalProperties": false
                }
              ]
            },
            "control": {
              "$ref": "#/$defs/control"
            }
          },
          "required": [
            "ok",
            "data",
            "control"
          ],
          "additionalProperties": false
        },
        {
          "type": "object",
          "properties": {
            "ok": {
              "const": false
            },
            "error": {
              "type": "object",
              "properties": {
                "code": {
                  "type": "string"
                },
                "message": {
                  "type": "string"
                },
                "executionId": {
                  "type": "string"
                }
              },
              "required": [
                "code",
                "message"
              ],
              "additionalProperties": false
            },
            "control": {
              "$ref": "#/$defs/control"
            }
          },
          "required": [
            "ok",
            "error",
            "control"
          ],
          "additionalProperties": false
        }
      ],
      "$defs": {
        "control": {
          "type": [
            "object",
            "null"
          ],
          "properties": {
            "requestAccepted": {
              "type": "boolean"
            },
            "providerInvoked": {
              "type": [
                "boolean",
                "null"
              ]
            },
            "dispatchCertainty": {
              "enum": [
                "not_dispatched",
                "dispatched",
                "uncertain"
              ]
            },
            "nextAction": {
              "$ref": "#/$defs/nextAction"
            }
          },
          "required": [
            "requestAccepted",
            "providerInvoked",
            "dispatchCertainty",
            "nextAction"
          ],
          "additionalProperties": false
        },
        "nextAction": {
          "type": [
            "object",
            "null"
          ],
          "properties": {
            "action": {
              "enum": [
                "observe",
                "review_result",
                "resume_pending",
                "manual_resolution",
                "correct_input",
                "activate_workspace",
                "list"
              ]
            },
            "executionId": {
              "type": "string"
            },
            "waitMs": {
              "type": "integer",
              "minimum": 0,
              "maximum": 25000
            },
            "includeResult": {
              "type": "boolean"
            }
          },
          "required": [
            "action"
          ],
          "additionalProperties": false
        },
        "execution": {
          "type": "object",
          "properties": {
            "executionId": {
              "type": "string"
            },
            "agentId": {
              "type": "string"
            },
            "workspaceId": {
              "type": "string"
            },
            "status": {
              "type": "string"
            },
            "dispatchState": {
              "type": "string"
            },
            "revision": {
              "type": "string",
              "description": "Legacy alias of controlRevision; Activity alone does not change it."
            },
            "controlRevision": {
              "type": "string"
            },
            "activityRevision": {
              "type": "string",
              "description": "Activity/Progress source token derived from persisted Activity identity; display-only age/silence changes do not change it; never controls default observe wake-ups."
            },
            "resultCompleteness": {
              "type": "string"
            },
            "attention": {
              "type": "string"
            },
            "providerTerminalStatus": {
              "type": [
                "string",
                "null"
              ]
            },
            "resultAvailable": {
              "type": "boolean"
            },
            "progress": {
              "type": "object",
              "properties": {
                "phase": {
                  "enum": [
                    "pending",
                    "dispatching",
                    "running",
                    "finalizing",
                    "reconciling",
                    "terminal"
                  ]
                },
                "activityPhase": {
                  "type": [
                    "string",
                    "null"
                  ],
                  "enum": [
                    "provider",
                    "tool",
                    null
                  ]
                },
                "toolCategory": {
                  "type": [
                    "string",
                    "null"
                  ],
                  "enum": [
                    "build",
                    "test",
                    "command",
                    "read",
                    "edit",
                    "tool",
                    null
                  ]
                },
                "lastActivityAt": {
                  "type": [
                    "integer",
                    "null"
                  ]
                },
                "activityAgeMs": {
                  "type": [
                    "integer",
                    "null"
                  ],
                  "minimum": 0
                },
                "silenceLevel": {
                  "description": "silenceLevel 是由 activityAgeMs 派生的展示桶：fresh <30s；quiet 30s–<120s；prolonged >=120s；null 表示尚无可观察 Activity。quiet/prolonged 不表示 stalled、timeout、失败或卡死，不能单独作为控制行为依据。",
                  "type": [
                    "string",
                    "null"
                  ],
                  "enum": [
                    "fresh",
                    "quiet",
                    "prolonged",
                    null
                  ]
                }
              },
              "required": [
                "phase",
                "activityPhase",
                "toolCategory",
                "lastActivityAt",
                "activityAgeMs",
                "silenceLevel"
              ],
              "additionalProperties": false
            },
            "nextAction": {
              "$ref": "#/$defs/nextAction"
            },
            "availableActions": {
              "type": "object",
              "properties": {
                "canCancel": {
                  "type": "boolean"
                },
                "canContinue": {
                  "type": "boolean"
                },
                "canResumePending": {
                  "type": "boolean"
                }
              },
              "required": [
                "canCancel",
                "canContinue",
                "canResumePending"
              ],
              "additionalProperties": false
            },
            "prompt": {
              "type": "string"
            },
            "canonicalWorkspaceRoot": {
              "type": "string"
            },
            "threadId": {
              "type": [
                "string",
                "null"
              ]
            },
            "threadName": {
              "type": [
                "string",
                "null"
              ]
            },
            "turnId": {
              "type": [
                "string",
                "null"
              ]
            },
            "errorCode": {
              "type": [
                "string",
                "null"
              ]
            },
            "errorMessage": {
              "type": [
                "string",
                "null"
              ]
            },
            "interruptRequested": {
              "type": "boolean"
            },
            "interruptAcknowledged": {
              "type": "boolean"
            },
            "interruptTimedOut": {
              "type": "boolean"
            },
            "createdAt": {
              "type": "integer"
            },
            "updatedAt": {
              "type": "integer"
            },
            "completedAt": {
              "type": [
                "integer",
                "null"
              ]
            },
            "unchanged": {
              "type": "boolean"
            },
            "finalResult": {}
          },
          "required": [
            "executionId",
            "agentId",
            "workspaceId",
            "status",
            "dispatchState",
            "revision",
            "controlRevision",
            "activityRevision",
            "resultCompleteness",
            "attention",
            "providerTerminalStatus",
            "resultAvailable",
            "progress",
            "nextAction",
            "availableActions",
            "prompt",
            "canonicalWorkspaceRoot",
            "threadId",
            "threadName",
            "turnId",
            "errorCode",
            "errorMessage",
            "interruptRequested",
            "interruptAcknowledged",
            "interruptTimedOut",
            "createdAt",
            "updatedAt",
            "completedAt"
          ]
        }
      }
    });
    // Activity v2 字段在闭合 Product schema 中与 DTO 原子发布，避免校验拒绝新快照。
    let progress = &mut schema["$defs"]["execution"]["properties"]["progress"];
    progress["properties"]["summaryCode"] = json!({"type":["string","null"]});
    progress["required"]
        .as_array_mut()
        .expect("execution progress required fields")
        .push(json!("summaryCode"));
    let execution = &mut schema["$defs"]["execution"]["properties"];
    execution["wakeReason"] = json!({
        "enum":["initial_mismatch","control","activity","terminal","result","timeout"]
    });
    execution["mismatchKind"] = json!({"enum":["control","activity"]});
    // P4-006：execute 同样暴露 ExecutionView，Usage 必须是稳定且可空字段已显式声明的公共 DTO。
    execution["usage"] = json!({
        "type":"object",
        "properties": {
            "inputTokens":{"type":["integer","null"]},
            "cachedInputTokens":{"type":["integer","null"]},
            "cacheWriteInputTokens":{"type":["integer","null"]},
            "outputTokens":{"type":["integer","null"]},
            "reasoningTokens":{"type":["integer","null"]},
            "totalTokens":{"type":["integer","null"]},
            "modelContextWindow":{"type":["integer","null"]},
            "completeness":{"enum":["unknown","partial","complete"]},
            "usageRevision":{"type":"integer","minimum":0,"format":"uint64"},
            "updatedAt":{"type":["integer","null"]}
        },
        "required":[
            "inputTokens","cachedInputTokens","cacheWriteInputTokens","outputTokens",
            "reasoningTokens","totalTokens","modelContextWindow","completeness",
            "usageRevision","updatedAt"
        ],
        "additionalProperties":false
    });
    schema["$defs"]["execution"]["required"]
        .as_array_mut()
        .expect("execution required fields")
        .push(json!("usage"));
    schema
}
pub fn tool_contract_hash(descriptor: &Tool) -> String {
    use sha2::{Digest, Sha256};
    fn sorted(value: Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .map(|(k, v)| (k, sorted(v)))
                    .collect::<std::collections::BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
            v => v,
        }
    }
    let contract = sorted(
        json!({"name":descriptor.name,"description":descriptor.description,"inputSchema":descriptor.input_schema,"annotations":descriptor.annotations,"outputSchema":descriptor.output_schema}),
    );
    Sha256::digest(contract.to_string().as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn orchestration_contract_diagnostic(enabled: bool, descriptors: &[Tool]) -> String {
    let mut contracts = descriptors
        .iter()
        .filter(|tool| super::orchestration::contains(&tool.name))
        .map(|tool| format!("{}={}", tool.name, tool_contract_hash(tool)))
        .collect::<Vec<_>>();
    contracts.sort();
    format!(
        "agentEnabled={enabled} orchestration contracts sha256 {}",
        contracts.join(",")
    )
}
/// 返回完全由 Broker 本地定义的公开 Tool surface；Discovery 不连接 Serena。
#[cfg(test)]
pub fn list(agent_enabled: bool) -> Vec<Tool> {
    list_with_capabilities(agent_enabled, false, false)
}

/// 兼容现有测试；Command 工具默认不加入旧 helper 的 surface。
#[cfg(test)]
pub fn list_with_source_write(agent_enabled: bool, remote_source_write_enabled: bool) -> Vec<Tool> {
    list_with_capabilities(agent_enabled, remote_source_write_enabled, false)
}

/// 返回当前 Broker 配置允许公开的完整 Tool surface。
pub fn list_with_capabilities(
    agent_enabled: bool,
    remote_source_write_enabled: bool,
    remote_command_execution_enabled: bool,
) -> Vec<Tool> {
    let mut list = vec![
        tool(
            "workspace_list",
            "【做什么】\n列出 Serena Desktop Workspace Registry 中登记的 Workspace catalog，返回 ID、名称、根目录和 generation。\n\n【什么时候使用】\n需要查看当前已登记的 Workspace catalog 时使用。\n\n【关键约束】\n纯 Discovery 查询：不扫描目录、不验证 Root、不调用 Provider，也不建立 Binding 或改变任何选择/活动状态。registryRevision 仅表示 catalog freshness。",
            schema::<Empty>(),
        ),
        tool(
            "workspace_get",
            "【做什么】\n按 workspaceId 查询 Serena Desktop Workspace Registry 中当前登记的单个 Workspace。\n\n【什么时候使用】\n已知 Workspace ID，需要读取其登记 catalog 信息时使用。\n\n【关键约束】\n纯 Discovery 查询：不验证 Root 当前存在性、不调用 Provider，也不建立 Binding 或改变任何选择/活动状态。registryRevision 仅表示 catalog freshness。",
            schema::<WorkspaceIdArgs>(),
        ),
    ];
    for &(name, allowed, required) in SOURCES {
        let mut s = schema::<SourceArgs>();
        s["properties"]
            .as_object_mut()
            .unwrap()
            .retain(|key, _| allowed.contains(&key.as_str()));
        s["required"] = json!(required);
        if is_workspace_scoped_source(name) {
            // Source Tool 的 Workspace Authority 只公开 workspaceId；root 不进入公共 Schema。
            s["properties"]["workspaceId"] =
                schema::<WorkspaceIdArgs>()["properties"]["workspaceId"].clone();
            s["required"]
                .as_array_mut()
                .unwrap()
                .push(json!("workspaceId"));
        }
        list.push(tool(name, source_description(name), s));
    }
    if remote_source_write_enabled {
        list.extend(SourceWriteTool::ALL.into_iter().map(source_write_tool));
    }
    for &name in GITS {
        let mut s = schema::<super::git::GitArgs>();
        s["properties"]
            .as_object_mut()
            .unwrap()
            .retain(|key, _| git_fields(name).contains(&key.as_str()));
        s["required"] = json!(["workspaceId"]);
        let description = match name {
            "git_status" => {
                "【做什么】\n查看指定已登记 Workspace 的 Git 工作区状态，包括分支、已暂存、未暂存和未跟踪文件。\n\n【什么时候使用】\n开始修改前确认工作区状态，或检查哪些文件需要审查、暂存或提交。\n\n【关键约束】\nworkspaceId 必填，且必须是已登记 Workspace；只读，不暂存或修改文件。返回 Git porcelain v1 格式。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_diff" => {
                "【做什么】\n查看指定已登记 Workspace 中已跟踪文件的差异，可按相对路径过滤。\n\n【什么时候使用】\n审查未暂存修改、待提交修改，或比较整个工作区与 HEAD 的差异。\n\n【关键约束】\nworkspaceId 必填，且必须是已登记 Workspace；只读。scope 为 unstaged（默认，工作区对暂存区）、staged（暂存区对 HEAD）或 all（工作区对 HEAD），不包含未跟踪文件内容。path 必须相对 Workspace 根目录且不得越界。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_log" => {
                "【做什么】\n查看指定已登记 Workspace 的提交历史，返回提交哈希、时间和标题，可按引用和相对路径筛选。\n\n【什么时候使用】\n追踪某个文件的变更历史，寻找相关提交，或了解最近的开发记录。\n\n【关键约束】\nworkspaceId 必填，且必须是已登记 Workspace；只读。reference 默认 HEAD；count 默认 20，范围 1–100。path 必须相对 Workspace 根目录且不得越界。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_show" => {
                "【做什么】\n查看指定已登记 Workspace 中指定 Git 引用的内容，例如提交详情和补丁，或通过 HEAD:相对路径读取历史文件。\n\n【什么时候使用】\n深入检查某次提交，或读取指定版本中的文件内容。\n\n【关键约束】\nworkspaceId 必填，且必须是已登记 Workspace；只读。reference 默认 HEAD；path 可过滤提交涉及的路径，必须相对 Workspace 根目录且不得越界。不会切换分支或检出文件。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_branch" => {
                "【做什么】\n列出指定已登记 Workspace 的本地分支及 Git 的当前分支标记。\n\n【什么时候使用】\n确认当前分支，或查看仓库中已有的本地分支。\n\n【关键约束】\nworkspaceId 必填，且必须是已登记 Workspace；只读，不创建、删除或切换分支，不列出远程跟踪分支。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_worktree_list" => {
                "【做什么】\n列出指定已登记 Workspace 关联的 Git 工作树及其路径、HEAD 和分支信息。\n\n【什么时候使用】\n确认同一仓库有哪些工作树，或检查分支与工作目录的对应关系。\n\n【关键约束】\nworkspaceId 必填，且必须是已登记 Workspace；只读，返回 Git porcelain 格式。不创建、删除或切换工作树，也不将这些路径自动登记或激活。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            _ => unreachable!("GITS contains only locally implemented tools"),
        };
        list.push(tool(name, description, s));
    }
    let mut media = tool(
        "media_read_image",
        "【做什么】\nRead an image from the requested Workspace and return it as MCP image content for visual inspection.\n\n【什么时候使用】\n查看指定 Workspace 中的截图或图片。\n\n【关键约束】\nworkspaceId 必填且只由 Registry 解析；path 只能相对该 Workspace 根目录。只读 PNG/JPEG/WebP。输入最多 20 MiB、40 MP，最长边缩至 2560 px，重编码并剥离原始 metadata；输出最多 6 MiB。WebP 返回 PNG。",
        schema::<super::media::MediaReadImageArgs>(),
    );
    media.output_schema = None;
    list.push(media);
    list.push(tool(
        "codegraph_explore",
        "【做什么】\n在指定已登记 Workspace 的 CodeGraph 索引中查询结构化代码图信息。\n\n【什么时候使用】\n需要探索符号、调用关系或相关文件，且该 Workspace 已由本地用户完成 CodeGraph 准备时使用。\n\n【关键约束】\nworkspaceId 和 query 必填。workspaceId 只由服务端解析为 WorkspaceLease；不接受 root、canonicalRoot 或 path 作为 authority。Remote 查询不会初始化、同步或索引；准备动作只可在本地能力界面执行。",
        schema::<CodeGraphExploreArgs>(),
    ));
    if agent_enabled {
        list.extend(super::orchestration::descriptors());
    }
    if remote_command_execution_enabled {
        list.extend(super::command::descriptors());
    }
    list
}
pub fn validate(name: &str, args: &Value) -> Result<(), String> {
    if super::command::contains(name) {
        return super::command::validate(name, args);
    }
    if super::orchestration::contains(name) {
        return super::orchestration::validate(name, args);
    }
    if is_source_write_tool(name) {
        // 先冻结 Workspace Authority 的错误分类，再委托既有 Handler DTO 做全部参数校验。
        parse_workspace_id(args)?;
        match name {
            "source_create_text_file" => serde_json::from_value::<
                super::source_write_create::SourceCreateTextFileInput,
            >(args.clone())
            .map(|_| ()),
            "source_write_text_file" => serde_json::from_value::<
                super::source_write_file::SourceWriteTextFileInput,
            >(args.clone())
            .map(|_| ()),
            "source_insert_lines" => serde_json::from_value::<
                super::source_write_insert::SourceInsertLinesInput,
            >(args.clone())
            .map(|_| ()),
            "source_delete_lines" => serde_json::from_value::<
                super::source_write_delete::SourceDeleteLinesInput,
            >(args.clone())
            .map(|_| ()),
            "source_replace_lines" => serde_json::from_value::<
                super::source_write_replace::SourceReplaceLinesInput,
            >(args.clone())
            .map(|_| ()),
            "source_replace_content" => serde_json::from_value::<
                super::source_write_content::SourceReplaceContentInput,
            >(args.clone())
            .map(|_| ()),
            _ => unreachable!("source write membership was checked above"),
        }
        .map_err(|error| format!("INVALID_PARAMS: {error}"))?;
    } else if let Some((_, allowed, required)) = SOURCES.iter().find(|t| t.0 == name) {
        let object = args.as_object().ok_or("INVALID_PARAMS: 参数必须是对象")?;
        let workspace_scoped = is_workspace_scoped_source(name);
        if workspace_scoped {
            // 先给出 Workspace Authority 的冻结错误分类，再验证业务字段。
            parse_workspace_id(args)?;
        }
        if object.keys().any(|key| {
            !allowed.contains(&key.as_str()) && (!workspace_scoped || key != "workspaceId")
        }) || required
            .iter()
            .any(|k| !object.contains_key(*k) || object[*k].is_null())
        {
            return Err("INVALID_PARAMS: 缺少必要参数或存在未公开参数".into());
        }
        let mut source_args = args.clone();
        if workspace_scoped {
            source_args
                .as_object_mut()
                .expect("validated Source arguments must be an object")
                .remove("workspaceId");
        }
        serde_json::from_value::<SourceArgs>(source_args)
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
    } else if GITS.contains(&name) {
        parse_workspace_id(args)?;
        let object = args
            .as_object()
            .expect("parse_workspace_id verified object");
        if object
            .keys()
            .any(|k| !git_fields(name).contains(&k.as_str()))
        {
            return Err("INVALID_PARAMS: 此工具不支持该参数".into());
        }
        serde_json::from_value::<super::git::GitArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
    } else if name == "media_read_image" {
        parse_workspace_id(args)?;
        serde_json::from_value::<super::media::MediaReadImageArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
    } else if name == "codegraph_explore" {
        // 先给出 Workspace Authority 的冻结错误分类，再验证查询字段。
        parse_workspace_id(args)?;
        let CodeGraphExploreArgs {
            workspace_id: _,
            query,
            max_files: _,
        } = serde_json::from_value::<CodeGraphExploreArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
        if query.trim().is_empty() {
            return Err("INVALID_PARAMS: query 不能为空".into());
        }
    } else if name == "workspace_activate" {
        let parsed = serde_json::from_value::<ActivateArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
        if parsed.id.is_empty() {
            return Err("INVALID_PARAMS: id 不能为空".into());
        }
    } else if name == "workspace_get" {
        parse_workspace_id(args)?;
        serde_json::from_value::<WorkspaceIdArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
    } else if matches!(
        name,
        "workspace_list" | "workspace_current" | "workspace_deactivate"
    ) {
        serde_json::from_value::<Empty>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
    } else {
        return Err("UNKNOWN_TOOL".into());
    }
    Ok(())
}

/// 构造六个既有写入 Handler 的公开 descriptor；schema 只描述既有 wire DTO，不复制校验或写入语义。
fn source_write_tool(tool_name: SourceWriteTool) -> Tool {
    let (description, input_schema) = match tool_name {
        SourceWriteTool::CreateTextFile => (
            "【做什么】\n在指定 Workspace 内新建 UTF-8 文本文件。\n\n【关键约束】\nworkspaceId 与 relative_path 必填；路径只能相对服务器解析出的 Workspace 根目录。目标已存在时拒绝，不接受 root 或绝对路径。",
            source_write_schema(
                &["workspaceId", "relative_path", "content"],
                json!({
                    "relative_path":{"type":"string"}, "content":{"type":"string"}
                }),
            ),
        ),
        SourceWriteTool::WriteTextFile => (
            "【做什么】\n创建或覆盖指定 Workspace 内的 UTF-8 文本文件。\n\n【关键约束】\n覆盖既有文件时必须提供读取结果中的 expectedSha256；完整复用既有 OCC、边界和原子替换校验。",
            source_write_schema(
                &["workspaceId", "relative_path", "content", "ifExists"],
                json!({
                    "relative_path":{"type":"string"}, "content":{"type":"string"},
                    "ifExists":{"type":"string","enum":["fail","overwrite"]},
                    "expectedSha256":expected_sha256_schema()
                }),
            ),
        ),
        SourceWriteTool::InsertLines => (
            "【做什么】\n在指定 Workspace 文本文件的行号前插入内容。\n\n【关键约束】\n必须提供 expectedSha256；行号为 1-based，完整复用既有 OCC、UTF-8、newline 和原子替换校验。",
            source_write_schema(
                &[
                    "workspaceId",
                    "relative_path",
                    "expectedSha256",
                    "beforeLine",
                    "content",
                ],
                json!({
                    "relative_path":{"type":"string"}, "expectedSha256":expected_sha256_schema(),
                    "beforeLine":{"type":"integer","minimum":1}, "content":{"type":"string"}
                }),
            ),
        ),
        SourceWriteTool::DeleteLines => (
            "【做什么】\n删除指定 Workspace 文本文件的闭合行范围。\n\n【关键约束】\n必须提供 expectedSha256；行号为 1-based inclusive，完整复用既有 OCC、边界和原子替换校验。",
            source_write_schema(
                &[
                    "workspaceId",
                    "relative_path",
                    "expectedSha256",
                    "startLine",
                    "endLine",
                ],
                json!({
                    "relative_path":{"type":"string"}, "expectedSha256":expected_sha256_schema(),
                    "startLine":{"type":"integer","minimum":1}, "endLine":{"type":"integer","minimum":1}
                }),
            ),
        ),
        SourceWriteTool::ReplaceLines => (
            "【做什么】\n替换指定 Workspace 文本文件的闭合行范围。\n\n【关键约束】\n必须提供 expectedSha256；行号为 1-based inclusive，完整复用既有 OCC、UTF-8、newline 和原子替换校验。",
            source_write_schema(
                &[
                    "workspaceId",
                    "relative_path",
                    "expectedSha256",
                    "startLine",
                    "endLine",
                    "content",
                ],
                json!({
                    "relative_path":{"type":"string"}, "expectedSha256":expected_sha256_schema(),
                    "startLine":{"type":"integer","minimum":1}, "endLine":{"type":"integer","minimum":1}, "content":{"type":"string"}
                }),
            ),
        ),
        SourceWriteTool::ReplaceContent => (
            "【做什么】\n按 literal 内容替换指定 Workspace 文本文件中的一个或全部匹配项。\n\n【关键约束】\n必须提供 expectedSha256；完整复用既有 OCC、UTF-8、newline、匹配计数和原子替换校验。",
            source_write_schema(
                &[
                    "workspaceId",
                    "relative_path",
                    "expectedSha256",
                    "oldContent",
                    "newContent",
                    "mode",
                ],
                json!({
                    "relative_path":{"type":"string"}, "expectedSha256":expected_sha256_schema(),
                    "oldContent":{"type":"string"}, "newContent":{"type":"string"},
                    "mode":{"type":"string","enum":["first","all"]},
                    "expectedMatches":{"type":"integer","minimum":1}, "maxReplacements":{"type":"integer","minimum":1}
                }),
            ),
        ),
    };
    let mut descriptor = Tool::new(tool_name.code(), description, input_schema);
    descriptor.annotations = Some(
        ToolAnnotations::default()
            .read_only(false)
            .destructive(true)
            .idempotent(false)
            .open_world(false),
    );
    descriptor.output_schema = Some(source_write_output_schema().into());
    descriptor
}

/// 统一注入受服务端解析的 Workspace authority，绝不公开 caller-provided root。
fn source_write_schema(required: &[&str], properties: Value) -> serde_json::Map<String, Value> {
    let mut properties = properties
        .as_object()
        .expect("source write properties must be an object")
        .clone();
    properties.insert(
        "workspaceId".into(),
        json!({"type":"string","description":"必须由服务端解析为 WorkspaceLease 的目标 Workspace。"}),
    );
    json!({
        "type":"object",
        "properties":properties,
        "required":required,
        "additionalProperties":false
    })
    .as_object()
    .expect("source write schema must be an object")
    .clone()
}

/// 既有 OCC token 的冻结格式。
fn expected_sha256_schema() -> Value {
    json!({"type":"string","pattern":"^[0-9a-f]{64}$"})
}

/// 六个既有 Handler 共用的成功结果序列化形状。
fn source_write_output_schema() -> serde_json::Map<String, Value> {
    json!({
        "type":"object",
        "properties":{
            "path":{"type":"string"}, "workspaceId":{"type":"string"},
            "generation":{"type":"integer","minimum":0},
            "beforeSha256":expected_sha256_schema(), "afterSha256":expected_sha256_schema(),
            "changedRange":{"type":"object","properties":{"startLine":{"type":"integer","minimum":1},"endLine":{"type":"integer","minimum":1}},"required":["startLine","endLine"],"additionalProperties":false},
            "changedCount":{"type":"integer","minimum":0}
        },
        "required":["path","workspaceId","generation","afterSha256"],
        "additionalProperties":false
    })
    .as_object()
    .expect("source write output schema must be an object")
    .clone()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn agent_output_contract_stable_fields() {
        let schema = agent_output_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["oneOf"][0]["properties"]["ok"]["const"], true);
        assert_eq!(schema["oneOf"][1]["properties"]["ok"]["const"], false);
        for field in [
            "requestAccepted",
            "providerInvoked",
            "dispatchCertainty",
            "nextAction",
        ] {
            assert!(
                schema["$defs"]["control"]["properties"]
                    .get(field)
                    .is_some()
            );
        }
        assert_eq!(
            schema["$defs"]["control"]["properties"]["providerInvoked"]["type"],
            json!(["boolean", "null"])
        );
        for field in ["threadName", "errorCode", "errorMessage"] {
            assert_eq!(
                schema["$defs"]["execution"]["properties"][field]["type"],
                json!(["string", "null"])
            );
            assert!(
                schema["$defs"]["execution"]["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(field))
            );
        }
        for field in [
            "executionId",
            "agentId",
            "workspaceId",
            "status",
            "dispatchState",
            "revision",
            "controlRevision",
            "activityRevision",
            "resultAvailable",
            "progress",
            "nextAction",
            "availableActions",
            "providerTerminalStatus",
            "resultCompleteness",
            "attention",
            "usage",
        ] {
            assert!(
                schema["$defs"]["execution"]["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(field))
            );
        }
        assert_eq!(
            schema["$defs"]["execution"]["properties"]["wakeReason"]["enum"],
            json!([
                "initial_mismatch",
                "control",
                "activity",
                "terminal",
                "result",
                "timeout"
            ])
        );
        assert_eq!(
            schema["$defs"]["execution"]["properties"]["mismatchKind"]["enum"],
            json!(["control", "activity"])
        );
        assert_eq!(
            schema["$defs"]["execution"]["properties"]["progress"]["properties"]["summaryCode"]["type"],
            json!(["string", "null"])
        );
        assert!(
            schema["$defs"]["execution"]["properties"]["progress"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("summaryCode"))
        );
        let usage = &schema["$defs"]["execution"]["properties"]["usage"];
        for field in [
            "inputTokens",
            "cachedInputTokens",
            "cacheWriteInputTokens",
            "outputTokens",
            "reasoningTokens",
            "totalTokens",
            "modelContextWindow",
            "completeness",
            "usageRevision",
            "updatedAt",
        ] {
            assert!(
                usage["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(field)),
                "usage {field} must be required"
            );
        }
        for field in [
            "inputTokens",
            "cachedInputTokens",
            "cacheWriteInputTokens",
            "outputTokens",
            "reasoningTokens",
            "totalTokens",
            "modelContextWindow",
            "updatedAt",
        ] {
            assert_eq!(
                usage["properties"][field]["type"],
                json!(["integer", "null"])
            );
        }
        assert_eq!(
            usage["properties"]["completeness"]["enum"],
            json!(["unknown", "partial", "complete"])
        );
        assert_eq!(usage["properties"]["usageRevision"]["format"], "uint64");
        assert_eq!(usage["additionalProperties"], false);
        println!("agent outputSchema bytes={}", schema.to_string().len());
    }

    #[test]
    fn orchestration_fingerprints_cover_each_descriptor_and_ignore_object_key_order() {
        fn reverse(value: Value) -> Value {
            match value {
                Value::Object(map) => Value::Object(
                    map.into_iter()
                        .rev()
                        .map(|(k, v)| (k, reverse(v)))
                        .collect(),
                ),
                Value::Array(values) => Value::Array(values.into_iter().map(reverse).collect()),
                value => value,
            }
        }
        let tools = super::super::orchestration::descriptors();
        // P3-004/005 原子公开契约的固定指纹，输入与闭合输出变化都必须显式更新。
        for (name, expected) in [
            (
                "agent_query",
                "67f707ccd1b8cc2853b1e48160ad6185ec064d4a3e8e2ce60edc8e50e3f7f60c",
            ),
            (
                "agent_execute",
                "96bf4a39676c38b508b76d95d7e61290a8a51f323f4d70af9b73957a7f51878b",
            ),
        ] {
            let tool = tools.iter().find(|tool| tool.name == name).unwrap();
            assert_eq!(tool_contract_hash(tool), expected, "{name}");
        }
        for tool in &tools {
            let hash = tool_contract_hash(tool);
            assert_eq!(hash.len(), 64);
            for field in [
                "name",
                "description",
                "inputSchema",
                "outputSchema",
                "annotations",
            ] {
                let mut value = serde_json::to_value(tool).unwrap();
                match field {
                    "name" | "description" => value[field] = json!("changed"),
                    "annotations" => {
                        value[field]["openWorldHint"] =
                            json!(!value[field]["openWorldHint"].as_bool().unwrap())
                    }
                    _ => value[field]["description"] = json!("changed"),
                }
                assert_ne!(
                    hash,
                    tool_contract_hash(&serde_json::from_value(value).unwrap()),
                    "{} {field}",
                    tool.name
                );
            }
            let reordered: Tool =
                serde_json::from_value(reverse(serde_json::to_value(tool).unwrap())).unwrap();
            assert_eq!(hash, tool_contract_hash(&reordered));
            assert!(
                orchestration_contract_diagnostic(true, &tools)
                    .contains(&format!("{}={hash}", tool.name))
            );
        }
    }
    #[test]
    fn orchestration_discovery_is_typed_and_action_specific() {
        for tool in super::super::orchestration::descriptors() {
            assert_eq!(tool.input_schema["type"], "object");
            let branches = tool.input_schema["oneOf"].as_array().unwrap();
            assert_eq!(
                branches.len(),
                match tool.name.as_ref() {
                    "work_query" => 2,
                    "agent_execute" => 4,
                    _ => 3,
                }
            );
            for branch in branches {
                assert_eq!(branch["additionalProperties"], false);
                assert!(
                    branch["required"]
                        .as_array()
                        .unwrap()
                        .contains(&json!("action"))
                );
            }
            let schema = serde_json::to_string(&tool.input_schema).unwrap();
            for private in [
                "delegationContextJson",
                "delegation_context_json",
                "wakeOn",
                "knownControlRevision",
                "agentId",
                "acceptedAt",
            ] {
                if tool.name == "agent_query"
                    && matches!(private, "wakeOn" | "knownControlRevision")
                {
                    continue;
                }
                assert!(!schema.contains(private), "{} {private}", tool.name);
            }
            if tool.name == "agent_query" {
                for field in [
                    "knownRevision",
                    "knownControlRevision",
                    "knownActivityRevision",
                    "wakeOn",
                ] {
                    assert!(schema.contains(field), "agent_query {field}");
                }
            }
            assert!(tool.output_schema.is_some());
            let read_only = tool.name.ends_with("query");
            assert_eq!(
                json!(tool.annotations),
                json!({"readOnlyHint":read_only,"destructiveHint":!read_only,"idempotentHint":read_only,"openWorldHint":tool.name=="agent_execute"})
            );
            if tool.name == "agent_execute" {
                assert_eq!(tool.input_schema["$defs"]["Context"]["type"], "object");
                assert_eq!(
                    tool.input_schema["$defs"]["Context"]["additionalProperties"],
                    false
                );
            }
        }
    }
    #[test]
    fn orchestration_toggle_only_adds_four_tools() {
        let disabled = list(false);
        let enabled = list(true);
        assert!(!disabled.iter().any(|t| t.name == "agent"));
        assert_eq!(
            enabled
                .iter()
                .filter(|t| super::super::orchestration::contains(&t.name))
                .count(),
            4
        );
        assert_eq!(disabled.len(), 17);
        assert_eq!(enabled.len(), 21);
        assert_eq!(
            enabled
                .iter()
                .map(|t| &t.name)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            enabled.len()
        );
        assert_eq!(
            enabled
                .into_iter()
                .filter(|t| !super::super::orchestration::contains(&t.name))
                .collect::<Vec<_>>(),
            disabled
        );
    }

    #[test]
    fn command_toggle_only_adds_query_and_execute_descriptors() {
        let disabled = list_with_capabilities(true, false, false);
        let enabled = list_with_capabilities(true, false, true);
        let disabled_names = disabled
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<std::collections::HashSet<_>>();
        let added = enabled
            .iter()
            .filter(|tool| !disabled_names.contains(tool.name.as_ref()))
            .collect::<Vec<_>>();

        assert_eq!(added.len(), 2);
        assert_eq!(
            added
                .iter()
                .map(|tool| tool.name.as_ref())
                .collect::<std::collections::HashSet<_>>(),
            super::super::command::NAMES.into_iter().collect()
        );
        for tool in added {
            let annotations = tool.annotations.as_ref().unwrap();
            if tool.name == "command_query" {
                assert_eq!(annotations.read_only_hint, Some(true));
                assert_eq!(annotations.destructive_hint, Some(false));
            } else {
                assert_eq!(annotations.read_only_hint, Some(false));
                assert_eq!(annotations.destructive_hint, Some(true));
            }
        }
    }

    #[test]
    fn source_write_toggle_only_adds_the_six_existing_write_descriptors() {
        let disabled = list_with_source_write(true, false);
        let enabled = list_with_source_write(true, true);
        let disabled_names = disabled
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<std::collections::HashSet<_>>();
        let enabled_names = enabled
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<std::collections::HashSet<_>>();

        for write_tool in SourceWriteTool::ALL {
            assert!(
                !disabled_names.contains(write_tool.code()),
                "{}",
                write_tool.code()
            );
            let descriptor = enabled
                .iter()
                .find(|tool| tool.name == write_tool.code())
                .unwrap();
            assert_eq!(
                descriptor.annotations.as_ref().unwrap().read_only_hint,
                Some(false)
            );
            assert_eq!(
                descriptor.annotations.as_ref().unwrap().destructive_hint,
                Some(true)
            );
            assert_eq!(descriptor.input_schema["additionalProperties"], false);
            assert!(
                descriptor.input_schema["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("workspaceId"))
            );
            assert!(descriptor.input_schema["properties"].get("root").is_none());
            assert_eq!(
                descriptor.output_schema.as_ref().unwrap()["properties"]["afterSha256"]["pattern"],
                "^[0-9a-f]{64}$"
            );
        }
        assert_eq!(
            enabled_names.len(),
            disabled_names.len() + SourceWriteTool::ALL.len()
        );
        for name in disabled_names {
            assert!(enabled_names.contains(name), "{name}");
        }
        assert!(authorize_source_write(false, "source_write_text_file").is_err());
        assert!(authorize_source_write(false, "source_read_file").is_ok());
        assert!(authorize_source_write(true, "source_write_text_file").is_ok());
    }

    #[test]
    fn public_schema_root_audit() {
        let mut findings = Vec::new();
        for tool in list(true) {
            for (kind, schema) in [
                ("input", Some(&tool.input_schema)),
                ("output", tool.output_schema.as_ref()),
            ] {
                if let Some(schema) = schema {
                    assert_eq!(
                        schema.get("type"),
                        Some(&json!("object")),
                        "{} {kind}",
                        tool.name
                    );
                    for key in ["oneOf", "anyOf", "allOf", "$ref", "$defs", "definitions"] {
                        if schema.contains_key(key) {
                            findings.push(format!("{} {kind} {key}", tool.name));
                        }
                    }
                }
            }
        }
        // Typed union schemas are intentional for these public tools.
        assert_eq!(
            findings,
            [
                "codegraph_explore output oneOf",
                "work_query input oneOf",
                "work_query output anyOf",
                "work_query output $defs",
                "work_update input oneOf",
                "work_update input $defs",
                "work_update output anyOf",
                "work_update output $defs",
                "agent_query input oneOf",
                "agent_query input $defs",
                "agent_query output anyOf",
                "agent_query output definitions",
                "agent_execute input oneOf",
                "agent_execute input $defs",
                "agent_execute output oneOf",
                "agent_execute output $defs"
            ]
        );
    }

    #[test]
    fn fixed_surface() {
        let tools = list(true);
        assert_eq!(tools.len(), 21);
        let mut expected = vec![
            "work_query",
            "work_update",
            "agent_query",
            "agent_execute",
            "workspace_list",
            "workspace_get",
            "media_read_image",
            "codegraph_explore",
        ];
        expected.extend(SOURCES.iter().map(|s| s.0));
        expected.extend(GITS.iter().copied());
        let names: std::collections::HashSet<_> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(names, expected.into_iter().collect());
        assert!(!names.contains("workspace_register"));
        assert!(!names.contains("workspace_select"));
        assert!(!names.contains("workspace_rename"));
        assert!(!names.contains("workspace_reorder"));
        assert!(!names.contains("workspace_remove"));
        assert!(!names.contains("workspace_import_serena"));
        let media = tools.iter().find(|t| t.name == "media_read_image").unwrap();
        assert!(media.output_schema.is_none());
        assert_eq!(
            media.annotations.as_ref().unwrap().read_only_hint,
            Some(true)
        );
        assert_eq!(media.input_schema["additionalProperties"], false);
        assert_eq!(
            media.input_schema["required"],
            json!(["workspaceId", "path"])
        );
        assert_eq!(
            media.input_schema["properties"],
            json!({
                "workspaceId":{"type":"string","description":"必须由服务端解析为 Lease 的目标 Workspace。"},
                "path":{"type":"string","description":"只能相对本次请求解析出的 Workspace 根目录。"}
            })
        );
        assert!(
            validate(
                "media_read_image",
                &json!({"workspaceId":"x","path":"a.png"})
            )
            .is_ok()
        );
        for args in [
            json!({}),
            json!({"path":"a.png"}),
            json!({"workspaceId":null,"path":"a.png"}),
        ] {
            assert_eq!(
                validate("media_read_image", &args),
                Err("WORKSPACE_CONTEXT_REQUIRED".into())
            );
        }
        for args in [
            json!({"workspaceId":3,"path":"a.png"}),
            json!({"workspaceId":"","path":"a.png"}),
            json!({"workspaceId":" \t","path":"a.png"}),
        ] {
            assert!(
                validate("media_read_image", &args)
                    .unwrap_err()
                    .starts_with("INVALID_PARAMS")
            );
        }
        for args in [
            json!({"workspaceId":"x","path":null}),
            json!({"workspaceId":"x","path":3}),
            json!({"workspaceId":"x","path":"a.png","root":"x"}),
            json!({"workspaceId":"x","path":"a.png","absolutePath":"x"}),
        ] {
            assert!(validate("media_read_image", &args).is_err());
        }
        assert_eq!(
            tools
                .iter()
                .map(|t| &t.name)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            21
        );
        assert!(
            validate(
                "source_read_file",
                &json!({"relative_path":"x","projectPath":"elsewhere"})
            )
            .is_err()
        );
        for name in [
            "workspace_current",
            "workspace_activate",
            "workspace_deactivate",
        ] {
            assert!(!names.contains(name), "{name} must not be advertised");
        }
        let codegraph = tools
            .iter()
            .find(|tool| tool.name == "codegraph_explore")
            .expect("P2D-009 must advertise the new CodeGraph Adapter route");
        assert_eq!(
            codegraph.input_schema["required"],
            json!(["workspaceId", "query"])
        );
        assert!(codegraph.input_schema["properties"].get("root").is_none());
        assert!(
            codegraph.input_schema["properties"]
                .get("canonicalRoot")
                .is_none()
        );
        assert_eq!(
            validate(
                "codegraph_explore",
                &json!({"query":"symbol", "maxFiles":2})
            ),
            Err("WORKSPACE_CONTEXT_REQUIRED".into())
        );
        let output = codegraph.output_schema.as_ref().unwrap();
        let success = &output["oneOf"][0];
        assert_eq!(
            success["required"],
            json!(["workspace", "text", "truncated"])
        );
        assert_eq!(
            success["properties"]["workspace"]["required"],
            json!(["id", "generation"])
        );
        assert!(
            success["properties"]["workspace"]["properties"]
                .get("root")
                .is_none()
        );
        assert_eq!(success["properties"]["text"]["type"], "string");
        assert_eq!(success["properties"]["truncated"]["type"], "boolean");
        assert!(!serde_json::to_string(output).unwrap().contains("\"root\""));
    }

    #[test]
    fn workspace_discovery_tools_expose_registry_contracts() {
        let tools = list(true);
        let list = tools
            .iter()
            .find(|tool| tool.name == "workspace_list")
            .unwrap();
        let get = tools
            .iter()
            .find(|tool| tool.name == "workspace_get")
            .unwrap();

        for tool in [list, get] {
            assert_eq!(
                tool.annotations.as_ref().unwrap().read_only_hint,
                Some(true)
            );
            let description = tool.description.as_deref().unwrap();
            assert!(description.contains("Workspace Registry"));
            assert!(!description.contains("从 Serena 同步"));
            assert!(!description.contains("activate"));
            assert!(!description.contains("当前活动项目"));
        }

        assert_eq!(get.input_schema["required"], json!(["workspaceId"]));
        assert_eq!(
            get.input_schema["properties"]["workspaceId"]["type"],
            "string"
        );
        assert_eq!(get.input_schema["additionalProperties"], false);

        for (tool, field) in [(list, "workspaces"), (get, "workspace")] {
            let output = tool.output_schema.as_ref().unwrap();
            assert_eq!(
                output["required"],
                json!(["registryRevision", field, "truncated"])
            );
            let workspace = if field == "workspaces" {
                &output["properties"][field]["items"]
            } else {
                &output["properties"][field]
            };
            assert_eq!(
                workspace["required"],
                json!(["id", "name", "root", "generation"])
            );
            assert_eq!(workspace["properties"]["generation"]["type"], "integer");
            assert_eq!(workspace["properties"]["generation"]["minimum"], 0);
        }
    }

    #[test]
    fn workspace_get_validates_required_context_before_registry_lookup() {
        for args in [json!({}), json!({"workspaceId": null})] {
            assert_eq!(
                validate("workspace_get", &args),
                Err("WORKSPACE_CONTEXT_REQUIRED".into())
            );
        }
        for args in [
            json!({"workspaceId": 3}),
            json!({"workspaceId": ""}),
            json!({"workspaceId": " \t"}),
        ] {
            assert!(
                validate("workspace_get", &args)
                    .unwrap_err()
                    .starts_with("INVALID_PARAMS"),
                "{args}"
            );
        }
    }

    #[test]
    fn workspace_id_foundation_preserves_parameter_and_provenance_contracts() {
        assert_eq!(
            parse_workspace_id(&json!({"workspaceId":"unknown"})),
            Ok("unknown".into())
        );
        for args in [json!({}), json!({"workspaceId": null})] {
            assert_eq!(
                parse_workspace_id(&args),
                Err("WORKSPACE_CONTEXT_REQUIRED".into())
            );
        }
        for args in [
            json!({"workspaceId": 3}),
            json!({"workspaceId": ""}),
            json!({"workspaceId": " \t"}),
        ] {
            assert!(
                parse_workspace_id(&args)
                    .unwrap_err()
                    .starts_with("INVALID_PARAMS"),
                "{args}"
            );
        }

        let workspace_id = schema::<WorkspaceIdArgs>();
        assert_eq!(workspace_id["required"], json!(["workspaceId"]));
        assert_eq!(workspace_id["properties"]["workspaceId"]["type"], "string");
        assert_eq!(workspace_id["additionalProperties"], false);

        let provenance = workspace_provenance_schema();
        assert_eq!(provenance["required"], json!(["id", "generation"]));
        assert_eq!(provenance["properties"]["id"]["type"], "string");
        assert_eq!(provenance["properties"]["generation"]["type"], "integer");
        assert!(provenance["properties"].get("root").is_none());
    }

    #[test]
    fn all_sources_require_workspace_context_and_keep_their_declared_fields() {
        let tools = list(true);
        for tool in tools.iter().filter(|tool| tool.name.starts_with("source_")) {
            let properties = tool.input_schema["properties"].as_object().unwrap();
            let required = tool.input_schema["required"].as_array().unwrap();
            assert_eq!(properties["workspaceId"]["type"], "string", "{}", tool.name);
            assert!(required.contains(&json!("workspaceId")), "{}", tool.name);
        }
        let codegraph = tools
            .iter()
            .find(|tool| tool.name == "codegraph_explore")
            .unwrap();
        assert_eq!(
            codegraph.input_schema["properties"]["workspaceId"]["type"],
            "string"
        );
        assert!(
            codegraph.input_schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("workspaceId"))
        );
    }

    #[test]
    fn sources_validate_workspace_context_before_business_arguments() {
        for &(name, _, _) in SOURCES {
            for args in [json!({}), json!({"workspaceId": null})] {
                assert_eq!(
                    validate(name, &args),
                    Err("WORKSPACE_CONTEXT_REQUIRED".into()),
                    "{name}: {args}"
                );
            }
            for args in [
                json!({"workspaceId": 3}),
                json!({"workspaceId": ""}),
                json!({"workspaceId": " \t"}),
            ] {
                assert!(
                    validate(name, &args)
                        .unwrap_err()
                        .starts_with("INVALID_PARAMS"),
                    "{name}: {args}"
                );
            }
        }

        assert!(
            validate(
                "source_symbols_overview",
                &json!({"workspaceId":"known","relative_path":"src/lib.rs"})
            )
            .is_ok()
        );
        assert!(
            validate(
                "source_read_file",
                &json!({"workspaceId":"known","relative_path":"src/lib.rs"})
            )
            .is_ok()
        );
    }

    #[test]
    fn local_source_schemas_keep_the_frozen_public_contracts() {
        let tools = list(true);
        for tool in tools
            .iter()
            .filter(|t| t.name.starts_with("source_") || t.name.starts_with("git_"))
        {
            let output = tool.output_schema.as_ref().unwrap();
            let required = output["required"].as_array().unwrap();
            assert!(required.contains(&json!("workspace")));
            assert!(required.contains(&json!("text")));
            assert!(required.contains(&json!("truncated")));
            assert!(output["properties"].get("content").is_none());
            for field in ["path", "sha256"] {
                if tool.name == "source_read_file" {
                    assert!(required.contains(&json!(field)));
                    assert_eq!(output["properties"][field]["type"], "string");
                } else {
                    assert!(!required.contains(&json!(field)), "{}", tool.name);
                    assert!(output["properties"].get(field).is_none(), "{}", tool.name);
                }
            }
            if tool.name == "source_read_file" {
                assert_eq!(output["properties"]["sha256"]["pattern"], "^[0-9a-f]{64}$");
                let properties = tool.input_schema["properties"].as_object().unwrap();
                assert_eq!(properties.len(), 5);
                for field in [
                    "workspaceId",
                    "relative_path",
                    "start_line",
                    "end_line",
                    "max_bytes",
                ] {
                    assert!(properties.contains_key(field));
                }
                assert!(
                    tool.input_schema["required"]
                        .as_array()
                        .unwrap()
                        .contains(&json!("workspaceId"))
                );
            }
            if tool.name == "source_list_dir" {
                let properties = tool.input_schema["properties"].as_object().unwrap();
                assert_eq!(properties.len(), 4);
                for field in ["workspaceId", "relative_path", "recursive", "max_bytes"] {
                    assert!(properties.contains_key(field));
                }
                assert!(
                    tool.input_schema["required"]
                        .as_array()
                        .unwrap()
                        .contains(&json!("relative_path"))
                );
                assert!(output["properties"].get("entries").is_none());
            }
            if tool.name == "source_find_file" {
                let properties = tool.input_schema["properties"].as_object().unwrap();
                assert_eq!(properties.len(), 4);
                for field in ["workspaceId", "relative_path", "file_mask", "max_bytes"] {
                    assert!(properties.contains_key(field));
                }
                let required = tool.input_schema["required"].as_array().unwrap();
                assert!(required.contains(&json!("workspaceId")));
                assert!(required.contains(&json!("file_mask")));
                assert!(!required.contains(&json!("relative_path")));
                assert!(output["properties"].get("files").is_none());
            }
            if tool.name == "source_search_pattern" {
                let properties = tool.input_schema["properties"].as_object().unwrap();
                assert_eq!(properties.len(), 4);
                for field in [
                    "workspaceId",
                    "relative_path",
                    "substring_pattern",
                    "max_bytes",
                ] {
                    assert!(properties.contains_key(field));
                }
                let required = tool.input_schema["required"].as_array().unwrap();
                assert!(required.contains(&json!("workspaceId")));
                assert!(required.contains(&json!("substring_pattern")));
                assert!(!required.contains(&json!("relative_path")));
            }
        }
    }

    /// P2B-005 后四个基础 Source Read 均在本地执行，兼容 facade 清理由后续任务负责。
    #[test]
    fn source_locality_classification_keeps_read_list_and_find_local() {
        assert_eq!(
            LOCAL_SOURCES,
            [
                "source_read_file",
                "source_list_dir",
                "source_find_file",
                "source_search_pattern"
            ]
        );
        assert_eq!(
            SEMANTIC_SOURCES,
            [
                "source_symbols_overview",
                "source_find_symbol",
                "source_find_references"
            ]
        );
    }

    #[test]
    fn git_tools_require_explicit_workspace_context_and_minimal_provenance() {
        let tools = list(true);
        for &name in GITS {
            let tool = tools.iter().find(|tool| tool.name == name).unwrap();
            let properties = tool.input_schema["properties"].as_object().unwrap();
            assert_eq!(properties["workspaceId"]["type"], "string", "{name}");
            assert!(
                tool.input_schema["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("workspaceId")),
                "{name}"
            );
            let description = tool.description.as_deref().unwrap();
            assert!(description.contains("workspaceId 必填"), "{name}");
            assert!(!description.contains("必须先激活"), "{name}");
            assert!(!description.contains("当前活动仓库"), "{name}");

            let output = tool.output_schema.as_ref().unwrap();
            assert_eq!(
                output["properties"]["workspace"]["required"],
                json!(["id", "generation"])
            );
            assert!(
                output["properties"]["workspace"]["properties"]
                    .get("name")
                    .is_none()
            );
            assert!(
                output["properties"]["workspace"]["properties"]
                    .get("root")
                    .is_none()
            );

            for args in [json!({}), json!({"workspaceId":null})] {
                assert_eq!(
                    validate(name, &args),
                    Err("WORKSPACE_CONTEXT_REQUIRED".into())
                );
            }
            for args in [
                json!({"workspaceId":3}),
                json!({"workspaceId":""}),
                json!({"workspaceId":" \t"}),
            ] {
                assert!(
                    validate(name, &args)
                        .unwrap_err()
                        .starts_with("INVALID_PARAMS"),
                    "{name} {args}"
                );
            }
        }
    }

    #[test]
    fn source_descriptions_are_local_stable_and_complete() {
        let tools = list(true);
        for &(name, _, _) in SOURCES {
            let description = tools
                .iter()
                .find(|tool| tool.name == name)
                .and_then(|tool| tool.description.as_deref())
                .unwrap();
            for section in ["【做什么】", "【什么时候使用】", "【关键约束】"] {
                assert!(description.contains(section), "{name}: {section}");
            }
            assert!(!description.contains("Original"), "{name}");
        }
        let search_description = tools
            .iter()
            .find(|tool| tool.name == "source_search_pattern")
            .and_then(|tool| tool.description.as_deref())
            .unwrap();
        assert!(search_description.contains("正则表达式"));
        assert!(search_description.contains("大小写敏感"));
        assert!(!search_description.contains("字面子串"));
        for tool in tools.iter().filter(|t| !t.name.starts_with("source_")) {
            let description = tool.description.as_deref().unwrap();
            let sections: Vec<_> = description
                .lines()
                .filter(|line| line.starts_with('【'))
                .collect();
            let expected = &["【做什么】", "【什么时候使用】", "【关键约束】"];
            assert_eq!(sections, expected, "{}", tool.name);
        }
    }

    #[test]
    fn execution_descriptions_route_known_commands_and_iterative_coding() {
        // 从公开的 Tool 列表检查路由提示，避免只验证模块内的文案常量。
        let tools = list_with_capabilities(true, false, true);
        let agent = tools
            .iter()
            .find(|tool| tool.name == "agent_execute")
            .and_then(|tool| tool.description.as_deref())
            .unwrap();
        for phrase in [
            "方案已明确",
            "阅读代码、修改实现、根据中间结果调整并完成验证",
            "优先使用 command_execute",
            "command_execute 能完成的确定性命令不应转交 Agent",
        ] {
            assert!(agent.contains(phrase), "agent_execute: {phrase}");
        }

        let command = tools
            .iter()
            .find(|tool| tool.name == "command_execute")
            .and_then(|tool| tool.description.as_deref())
            .unwrap();
        for phrase in [
            "Git 写操作、构建、测试、包管理、脚本、本地诊断",
            "executable/args 或 shell command 已确定时优先使用",
            "自主读代码、修改实现并根据结果迭代，应使用 agent_execute",
        ] {
            assert!(command.contains(phrase), "command_execute: {phrase}");
        }
    }

    #[test]
    fn list_succeeds_without_upstream_or_serena() {
        let tools = list(true);
        for &name in LOCAL_SOURCES.iter().chain(SEMANTIC_SOURCES) {
            assert!(tools.iter().any(|tool| tool.name == name), "{name}");
        }
    }
}

#[cfg(test)]
mod agent_contract_tests {
    #[test]
    fn agent_query_compact_schema_is_separate_and_execute_retains_existing_contract() {
        let tools = super::super::orchestration::descriptors();
        let agent = tools.iter().find(|t| t.name == "agent_execute").unwrap();
        assert_ne!(
            agent.output_schema,
            tools
                .iter()
                .find(|t| t.name == "agent_query")
                .unwrap()
                .output_schema
        );
        assert_eq!(
            serde_json::to_value(agent.output_schema.as_ref().unwrap()).unwrap(),
            super::agent_output_schema()
        );
        let query_descriptor = tools.iter().find(|t| t.name == "agent_query").unwrap();
        let query_description = query_descriptor.description.as_deref().unwrap();
        for contract in [
            "默认 15000ms",
            "范围 0..=20000",
            "knownRevision 是 knownControlRevision 的 legacy alias",
            "activity 模式",
            "不能只看 unchanged",
            "latest snapshot",
            "coalesce",
            "旧 v1 token",
        ] {
            assert!(
                query_description.contains(contract),
                "agent_query {contract}"
            );
        }
        let query = serde_json::to_value(query_descriptor.output_schema.as_ref().unwrap()).unwrap();
        for (branch, field, ok) in [(0, "data", true), (1, "error", false)] {
            let envelope = &query["anyOf"][branch];
            assert_eq!(envelope["properties"]["ok"]["const"], ok);
            assert_eq!(envelope["properties"].as_object().unwrap().len(), 2);
            assert!(envelope["properties"].get("control").is_none());
            assert_eq!(envelope["required"], serde_json::json!(["ok", field]));
            assert_eq!(envelope["additionalProperties"], false);
            assert!(
                agent.output_schema.as_ref().unwrap()["oneOf"][branch]["required"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!("control"))
            );
        }
        let defs = &query["definitions"];
        assert_eq!(defs["QueryData"]["anyOf"].as_array().unwrap().len(), 3);
        let observation = &defs["QueryObservation"];
        for field in [
            "executionId",
            "usage",
            "status",
            "revision",
            "activityRevision",
            "unchanged",
            "resultAvailable",
            "resultCompleteness",
            "progress",
        ] {
            assert!(
                observation["required"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!(field))
            );
        }
        assert_eq!(observation["required"].as_array().unwrap().len(), 9);
        assert_eq!(observation["properties"].as_object().unwrap().len(), 15);
        assert_eq!(observation["additionalProperties"], false);
        for field in [
            "prompt",
            "canonicalWorkspaceRoot",
            "controlRevision",
            "threadId",
            "dispatchState",
        ] {
            assert!(observation["properties"].get(field).is_none());
        }
        assert_eq!(
            defs["WakeReason"]["enum"],
            serde_json::json!([
                "initial_mismatch",
                "control",
                "activity",
                "terminal",
                "result",
                "timeout"
            ])
        );
        assert_eq!(
            defs["MismatchKind"]["enum"],
            serde_json::json!(["control", "activity"])
        );
        let summary = &defs["QuerySummary"];
        assert_eq!(summary["properties"].as_object().unwrap().len(), 13);
        assert_eq!(summary["required"].as_array().unwrap().len(), 10);
        assert_eq!(summary["additionalProperties"], false);
        for field in ["provider", "usage"] {
            assert!(
                summary["required"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!(field))
            );
        }
        assert_eq!(
            defs["UsageCompletenessProduct"]["enum"],
            serde_json::json!(["unknown", "partial", "complete"])
        );
        for field in [
            "prompt",
            "canonicalWorkspaceRoot",
            "controlRevision",
            "activityRevision",
            "threadId",
            "finalResult",
        ] {
            assert!(summary["properties"].get(field).is_none());
            if field != "finalResult" {
                assert!(
                    defs["ExecutionView"]["required"]
                        .as_array()
                        .unwrap()
                        .contains(&serde_json::json!(field))
                );
            }
        }
        assert_eq!(
            defs["QueryProgress"]["required"],
            serde_json::json!(["phase", "summaryCode"])
        );
        assert_eq!(
            defs["QueryProgress"]["properties"]
                .as_object()
                .unwrap()
                .len(),
            4
        );
        assert_eq!(defs["QueryProgress"]["additionalProperties"], false);
        let silence_level_description = agent.output_schema.as_ref().unwrap()["$defs"]["execution"]
            ["properties"]["progress"]["properties"]["silenceLevel"]["description"]
            .as_str()
            .expect("silenceLevel description must be present");
        for contract in [
            "silenceLevel 是由 activityAgeMs 派生的展示桶",
            "fresh <30s",
            "quiet 30s–<120s",
            "prolonged >=120s",
            "null 表示尚无可观察 Activity",
            "quiet/prolonged 不表示 stalled、timeout、失败或卡死",
            "不能单独作为控制行为依据",
        ] {
            assert!(
                silence_level_description.contains(contract),
                "missing silenceLevel schema contract: {contract}"
            );
        }
        let description = agent.description.as_deref().unwrap();
        for contract in [
            "复合编码任务",
            "continue",
            "新 Execution",
            "Thread",
            "context",
            "requestKey",
            "resume_pending",
        ] {
            assert!(description.contains(contract));
        }
    }
}
