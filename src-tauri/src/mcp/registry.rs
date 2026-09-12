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
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphArgs {
    pub query: String,
    #[serde(rename = "maxFiles")]
    pub max_files: Option<u32>,
}
pub const SOURCES: &[(&str, &str, &[&str], &[&str])] = &[
    (
        "source_read_file",
        "read_file",
        &["relative_path", "start_line", "end_line", "max_bytes"],
        &["relative_path"],
    ),
    (
        "source_list_dir",
        "list_dir",
        &["relative_path", "recursive", "max_bytes"],
        &["relative_path"],
    ),
    (
        "source_find_file",
        "find_file",
        &["relative_path", "file_mask", "max_bytes"],
        &["file_mask"],
    ),
    (
        "source_search_pattern",
        "search_for_pattern",
        &["relative_path", "substring_pattern", "max_bytes"],
        &["substring_pattern"],
    ),
    (
        "source_symbols_overview",
        "get_symbols_overview",
        &["relative_path", "depth", "max_bytes"],
        &["relative_path"],
    ),
    (
        "source_find_symbol",
        "find_symbol",
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
        "find_referencing_symbols",
        &["relative_path", "name_path", "max_bytes"],
        &["relative_path", "name_path"],
    ),
];
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
        "git_diff" => &["scope", "path", "max_bytes"],
        "git_log" => &["reference", "path", "count", "max_bytes"],
        "git_show" => &["reference", "path", "max_bytes"],
        _ => &["max_bytes"],
    }
}
fn schema<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).unwrap()
}
fn tool(name: &'static str, desc: &'static str, value: Value) -> Tool {
    let mut t = Tool::new(name, desc, value.as_object().unwrap().clone());
    t.annotations = Some(ToolAnnotations::default().read_only(!matches!(
        name,
        "workspace_activate" | "workspace_deactivate"
    )));
    let workspace = json!({"type":"object", "properties":{"id":{"type":"string"},"name":{"type":"string"},"root":{"type":"string"}}, "required":["id","name","root"]});
    let mut output = json!({"type":"object","properties":{"truncated":{"type":"boolean"}},"required":["truncated"]});
    if name == "workspace_list" {
        output["properties"]["workspaces"] = json!({"type":"array","items":workspace});
        output["required"] = json!(["workspaces", "truncated"]);
    } else if name.starts_with("workspace_") {
        output["properties"]["activeWorkspace"] = json!({"anyOf":[workspace,{"type":"null"}]});
        output["properties"]["status"] = json!({"type":"string"});
        output["required"] = json!(["activeWorkspace", "truncated"]);
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
        output = json!({"type":"object", "oneOf":[output, {
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
// Stable core only; open execution fields preserve dynamic Product data.
pub(crate) fn agent_output_schema() -> Value {
    json!({
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
    })
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
pub fn list(upstream: &[Tool], agent_enabled: bool) -> Result<Vec<Tool>, String> {
    let mut list = vec![
        tool(
            "workspace_list",
            "【做什么】\n列出 Desktop 已从 Serena 同步的项目，返回项目 ID、名称和根目录。\n\n【什么时候使用】\n查找可激活的项目，或在调用 workspace_activate 前获取项目 ID。\n\n【关键约束】\n只读取已同步列表，不扫描目录、不初始化项目，也不切换当前活动项目。新项目需先在 Serena 初始化，再由 Desktop 同步。",
            schema::<Empty>(),
        ),
        tool(
            "workspace_current",
            "【做什么】\n查询所有客户端共享的当前活动项目，返回其 ID、名称和根目录；没有有效活动绑定时返回 null。\n\n【什么时候使用】\n执行 source_* 或 git_* 操作前确认目标项目，或检查切换后的共享工作区。\n\n【关键约束】\n只读查询，不激活项目。活动状态由所有客户端共享，其他客户端可能切换或取消该绑定。",
            schema::<Empty>(),
        ),
        tool(
            "workspace_activate",
            "【做什么】\n激活一个已登记项目，并将其设置为所有客户端共享的当前活动项目。\n\n【什么时候使用】\n当后续 source_* 或 git_* 操作需要切换到指定项目时使用。\n\n【关键约束】\n这是全局共享状态变更，会影响所有连接到本服务的客户端。id 必须来自 workspace_list；项目须已初始化且 Serena 正在运行。切换失败可能清空活动绑定，应查询 workspace_current 后重新激活。",
            schema::<ActivateArgs>(),
        ),
        tool(
            "workspace_deactivate",
            "【做什么】\n取消 Broker 中所有客户端共享的当前活动项目绑定。\n\n【什么时候使用】\n结束当前工作区的使用，或希望后续操作必须先明确激活项目时使用。\n\n【关键约束】\n影响所有客户端；取消后 source_* 和 git_* 不可用，直到重新激活。不会停止 Serena 或 Broker，也不会删除项目文件、配置或登记信息。",
            schema::<Empty>(),
        ),
    ];
    for &(name, remote_name, allowed, required) in SOURCES {
        let mut s = schema::<SourceArgs>();
        s["properties"]
            .as_object_mut()
            .unwrap()
            .retain(|key, _| allowed.contains(&key.as_str()));
        s["required"] = json!(required);
        let remote = upstream
            .iter()
            .find(|t| t.name == remote_name)
            .ok_or_else(|| format!("BACKEND_INCOMPATIBLE: missing {remote_name}"))?;
        let mut source = tool(name, "", s);
        // Preserve the upstream description exactly, including whitespace and absence.
        source.description = remote.description.clone();
        list.push(source);
    }
    for &name in GITS {
        let mut s = schema::<super::git::GitArgs>();
        s["properties"]
            .as_object_mut()
            .unwrap()
            .retain(|key, _| git_fields(name).contains(&key.as_str()));
        let description = match name {
            "git_status" => {
                "【做什么】\n查看当前活动仓库的工作区状态，包括分支、已暂存、未暂存和未跟踪文件。\n\n【什么时候使用】\n开始修改前确认工作区状态，或检查哪些文件需要审查、暂存或提交。\n\n【关键约束】\n必须先激活项目；只读，不暂存或修改文件。返回 Git porcelain v1 格式。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_diff" => {
                "【做什么】\n查看当前活动仓库已跟踪文件的差异，可按相对路径过滤。\n\n【什么时候使用】\n审查未暂存修改、待提交修改，或比较整个工作区与 HEAD 的差异。\n\n【关键约束】\n必须先激活项目；只读。scope 为 unstaged（默认，工作区对暂存区）、staged（暂存区对 HEAD）或 all（工作区对 HEAD），不包含未跟踪文件内容。path 相对仓库根且不得越界。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_log" => {
                "【做什么】\n查看当前活动仓库的提交历史，返回提交哈希、时间和标题，可按引用和相对路径筛选。\n\n【什么时候使用】\n追踪某个文件的变更历史，寻找相关提交，或了解最近的开发记录。\n\n【关键约束】\n必须先激活项目；只读。reference 默认 HEAD；count 默认 20，范围 1–100。path 相对仓库根且不得越界。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_show" => {
                "【做什么】\n查看当前活动仓库指定 Git 引用的内容，例如提交详情和补丁，或通过 HEAD:相对路径读取历史文件。\n\n【什么时候使用】\n深入检查某次提交，或读取指定版本中的文件内容。\n\n【关键约束】\n必须先激活项目；只读。reference 默认 HEAD；path 可过滤提交涉及的路径，相对仓库根且不得越界。不会切换分支或检出文件。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_branch" => {
                "【做什么】\n列出当前活动仓库的本地分支及 Git 的当前分支标记。\n\n【什么时候使用】\n确认当前分支，或查看仓库中已有的本地分支。\n\n【关键约束】\n必须先激活项目；只读，不创建、删除或切换分支，不列出远程跟踪分支。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            "git_worktree_list" => {
                "【做什么】\n列出当前活动仓库关联的 Git 工作树及其路径、HEAD 和分支信息。\n\n【什么时候使用】\n确认同一仓库有哪些工作树，或检查分支与工作目录的对应关系。\n\n【关键约束】\n必须先激活项目；只读，返回 Git porcelain 格式。不创建、删除或切换工作树，也不将这些路径自动登记或激活。max_bytes 默认 65536，范围 1–262144；超限时返回截断标记。"
            }
            _ => unreachable!("GITS contains only locally implemented tools"),
        };
        list.push(tool(name, description, s));
    }
    list.push(tool(
        "codegraph_explore",
        "【做什么】\n探索当前活动 Workspace 的代码结构、跨函数/文件/模块调用路径、依赖关系和潜在影响范围。\n\n【什么时候使用】\n理解功能或模块如何工作、追踪完整调用链、分析修改影响或探索架构。已知文件路径、Symbol 名称，或只需读文件、直接 references 时优先使用 source_*。\n\n【关键约束】\n只查询当前活动项目；query 必填，maxFiles 默认 12。不接受 projectPath，不跨项目回退，不自动初始化索引。保留索引陈旧提示。启动中返回 CODEGRAPH_STARTING；运行故障每次调用最多恢复一次、查询最多重试一次，恢复有 30 秒冷却。错误以 error.code/message/workspace/recoverable 返回。",
        schema::<GraphArgs>(),
    ));
    let mut media = tool(
        "media_read_image",
        "【做什么】\nRead an image from the active workspace and return it as MCP image content for visual inspection.\n\n【什么时候使用】\n查看当前项目中的截图或图片。\n\n【关键约束】\n仅支持 Workspace 相对路径；只读 PNG/JPEG/WebP。输入最多 20 MiB、40 MP，最长边缩至 2560 px，重编码并剥离原始 metadata；输出最多 6 MiB。WebP 返回 PNG。",
        schema::<super::media::MediaReadImageArgs>(),
    );
    media.output_schema = None;
    list.push(media);
    if agent_enabled {
        list.extend(super::orchestration::descriptors());
    }
    Ok(list)
}
pub fn validate(name: &str, args: &Value) -> Result<(), String> {
    if super::orchestration::contains(name) {
        return super::orchestration::validate(name, args);
    }
    if let Some((_, _, allowed, required)) = SOURCES.iter().find(|t| t.0 == name) {
        let object = args.as_object().ok_or("INVALID_PARAMS: 参数必须是对象")?;
        if object.keys().any(|k| !allowed.contains(&k.as_str()))
            || required
                .iter()
                .any(|k| !object.contains_key(*k) || object[*k].is_null())
        {
            return Err("INVALID_PARAMS: 缺少必要参数或存在未公开参数".into());
        }
        serde_json::from_value::<SourceArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
    } else if GITS.contains(&name) {
        if args
            .as_object()
            .ok_or("INVALID_PARAMS")?
            .keys()
            .any(|k| !git_fields(name).contains(&k.as_str()))
        {
            return Err("INVALID_PARAMS: 此工具不支持该参数".into());
        }
        serde_json::from_value::<super::git::GitArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
    } else if name == "media_read_image" {
        serde_json::from_value::<super::media::MediaReadImageArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
    } else if name == "codegraph_explore" {
        let parsed = serde_json::from_value::<GraphArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
        if parsed.query.trim().is_empty() || parsed.max_files == Some(0) {
            return Err("INVALID_PARAMS: query 不能为空，maxFiles 必须为正整数".into());
        }
    } else if name == "workspace_activate" {
        let parsed = serde_json::from_value::<ActivateArgs>(args.clone())
            .map_err(|e| format!("INVALID_PARAMS: {e}"))?;
        if parsed.id.is_empty() {
            return Err("INVALID_PARAMS: id 不能为空".into());
        }
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
        ] {
            assert!(
                schema["$defs"]["execution"]["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(field))
            );
        }
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
    fn upstream() -> Vec<Tool> {
        SOURCES.iter().map(|(_, name, _, _)| {
            Tool::new(*name, format!("  Original {name}\n\nDetailed usage, constraints, and examples.\n中文说明。\n"), serde_json::Map::new())
        }).collect()
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
                assert!(!schema.contains(private), "{} {private}", tool.name);
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
        let disabled = list(&upstream(), false).unwrap();
        let enabled = list(&upstream(), true).unwrap();
        assert!(!disabled.iter().any(|t| t.name == "agent"));
        assert_eq!(
            enabled
                .iter()
                .filter(|t| super::super::orchestration::contains(&t.name))
                .count(),
            4
        );
        assert_eq!(disabled.len(), 19);
        assert_eq!(enabled.len(), 23);
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
    fn public_schema_root_audit() {
        let mut findings = Vec::new();
        for tool in list(&upstream(), true).unwrap() {
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
                    for key in ["oneOf", "anyOf", "allOf", "$ref", "$defs"] {
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
                "agent_query output oneOf",
                "agent_query output $defs",
                "agent_execute input oneOf",
                "agent_execute input $defs",
                "agent_execute output oneOf",
                "agent_execute output $defs"
            ]
        );
    }

    #[test]
    fn fixed_surface() {
        let tools = list(&upstream(), true).unwrap();
        assert_eq!(tools.len(), 23);
        let mut expected = vec![
            "work_query",
            "work_update",
            "agent_query",
            "agent_execute",
            "workspace_list",
            "workspace_current",
            "workspace_activate",
            "workspace_deactivate",
            "codegraph_explore",
            "media_read_image",
        ];
        expected.extend(SOURCES.iter().map(|s| s.0));
        expected.extend(GITS.iter().copied());
        let names: std::collections::HashSet<_> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(names, expected.into_iter().collect());
        let media = tools.iter().find(|t| t.name == "media_read_image").unwrap();
        assert!(media.output_schema.is_none());
        assert_eq!(
            media.annotations.as_ref().unwrap().read_only_hint,
            Some(true)
        );
        assert_eq!(media.input_schema["additionalProperties"], false);
        assert_eq!(media.input_schema["required"], json!(["path"]));
        assert_eq!(
            media.input_schema["properties"],
            json!({"path":{"type":"string","description":"Image path relative to the active workspace root."}})
        );
        assert!(validate("media_read_image", &json!({"path":"a.png"})).is_ok());
        for args in [
            json!({}),
            json!({"path":null}),
            json!({"path":3}),
            json!({"path":"a.png","root":"x"}),
            json!({"path":"a.png","workspaceId":"x"}),
            json!({"path":"a.png","absolutePath":"x"}),
        ] {
            assert!(validate("media_read_image", &args).is_err());
        }
        assert_eq!(
            tools
                .iter()
                .map(|t| &t.name)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            23
        );
        assert!(
            validate(
                "source_read_file",
                &json!({"relative_path":"x","projectPath":"elsewhere"})
            )
            .is_err()
        );
        assert!(validate("workspace_deactivate", &json!({})).is_ok());
        let graph = tools
            .iter()
            .find(|t| t.name == "codegraph_explore")
            .unwrap();
        assert!(
            graph.input_schema["properties"]
                .get("projectPath")
                .is_none()
        );
        assert!(
            validate(
                "codegraph_explore",
                &json!({"query":"symbol", "maxFiles":2})
            )
            .is_ok()
        );
        for args in [
            json!({}),
            json!({"query":" "}),
            json!({"query":"x","maxFiles":0}),
            json!({"query":"x","projectPath":"elsewhere"}),
        ] {
            assert!(validate("codegraph_explore", &args).is_err());
        }
    }

    #[test]
    fn source_read_file_alone_requires_local_file_version_output() {
        let tools = list(&upstream(), true).unwrap();
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
                assert_eq!(properties.len(), 4);
                for field in ["relative_path", "start_line", "end_line", "max_bytes"] {
                    assert!(properties.contains_key(field));
                }
            }
        }
    }

    #[test]
    fn forwarded_descriptions_are_exact_and_local_descriptions_have_expected_sections() {
        let upstream = upstream();
        let tools = list(&upstream, true).unwrap();
        for (name, remote, _, _) in SOURCES {
            let exposed = tools.iter().find(|t| t.name == *name).unwrap();
            let original = upstream.iter().find(|t| t.name == *remote).unwrap();
            assert_eq!(exposed.description, original.description, "{name}");
        }
        for tool in tools.iter().filter(|t| !t.name.starts_with("source_")) {
            let description = tool.description.as_deref().unwrap();
            let sections: Vec<_> = description
                .lines()
                .filter(|line| line.starts_with('【'))
                .collect();
            let expected = &["【做什么】", "【什么时候使用】", "【关键约束】"];
            assert_eq!(sections, expected, "{}", tool.name);
        }
        let activate = tools
            .iter()
            .find(|t| t.name == "workspace_activate")
            .unwrap();
        assert!(
            activate
                .description
                .as_deref()
                .unwrap()
                .contains("这是全局共享状态变更，会影响所有连接到本服务的客户端。")
        );
    }

    #[test]
    fn missing_upstream_tool_fails_instead_of_using_a_placeholder() {
        assert!(list(&[], true).unwrap_err().contains("missing read_file"));
        let mut upstream = upstream();
        upstream[0].description = None;
        let tools = list(&upstream, true).unwrap();
        assert!(
            tools
                .iter()
                .find(|t| t.name == "source_read_file")
                .unwrap()
                .description
                .is_none()
        );
    }
}

#[cfg(test)]
mod agent_contract_tests {
    #[test]
    fn agent_query_and_execute_retain_the_existing_execution_output_contract() {
        let tools = super::super::orchestration::descriptors();
        let agent = tools.iter().find(|t| t.name == "agent_execute").unwrap();
        assert_eq!(
            agent.output_schema,
            tools
                .iter()
                .find(|t| t.name == "agent_query")
                .unwrap()
                .output_schema
        );
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
            "ChatGPT",
            "Review",
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
