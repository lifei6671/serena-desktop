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
fn agent_output_schema() -> Value {
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
              "type": "string"
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
pub fn agent_tool() -> Tool {
    let mut agent = Tool::new(
        "agent",
        "【定位】\nAgent 是 ChatGPT 的本地执行器。ChatGPT 负责读取代码、调查问题、分析、设计和 Review；Agent 负责按照 ChatGPT 已确定的目标实施修改并执行工程任务。\n\n【什么时候使用】\n仅当需要实际执行时使用，例如：修改/创建文件、实现代码、运行命令、lint、build、单元测试、集成测试、E2E、Native 测试或真实运行验证。\n\n【不要使用】\n不要把只读调查委托给 Agent，包括源码阅读、搜索、Symbol/Reference 查询、Git 查看、调用链分析、Bug 根因分析、架构设计、影响分析和代码 Review。这些应由 ChatGPT 使用 workspace/source/git/codegraph/media 工具自行完成。\n任务复杂、多步骤或跨文件，不是使用 Agent 的理由；是否需要实际执行才是判断依据。\n\n【生命周期】\nfresh 执行使用 start；同一 lineage 的后续执行使用 continue；resume_pending 仅用于已经 durable 创建、可靠证明尚未跨越 Provider side-effect boundary 且未建立 Runtime attempt 的 pending Execution；Host crash、Provider/backend 不可用或 binary discovery failure（均在 Runtime 创建前）都可能产生此状态；observe/list 用于查看状态；cancel 仅取消指定 executionId。agentId 不跨 Workspace 或 fresh Thread。start/continue/resume_pending 接受后立即返回，执行由本地 Worker 继续；使用 observe 携带 knownRevision 等待有意义状态变化，waitMs 默认 20000、最大 25000，0 表示立即读取。连接中断不取消执行，可按 executionId 重连观察。默认不返回结果正文；resultAvailable=true 时使用 observe(includeResult=true) 获取已持久化结果，可重复读取。nextAction 仅为提示，操作资格仍由 availableActions 和后端校验决定。control.requestAccepted 表示当前请求已获得 durable Execution identity；providerInvoked 为 true/false/null，dispatching/uncertain 不能证明已派发。错误优先按 error.code 与 control.nextAction 处理，不根据 message 推断。control=null 或连接结果不明时，仅原样重试同一 action 和全部原始参数；start/continue 保留原 requestKey，其他 action 不新增 requestKey。不得自动生成新 key、replay Provider 或夺取 Claim。\n\n【进度提示】\nprogress.activityPhase、toolCategory、lastActivityAt、activityAgeMs、silenceLevel 仅表示最近观察到的非权威 Activity；toolCategory 是 best-effort 分类。silenceLevel 仅由 activityAgeMs 派生的固定展示桶：fresh <30s、quiet 30s–<120s、prolonged >=120s；尚无可观察 Activity 时为 null。quiet/prolonged 本身不表示 stalled、timeout、失败或卡死；长时间没有 Activity 或 activityAgeMs 较大不代表 stalled、失败或卡死。不得仅因 quiet/prolonged 或 Activity 时间而 cancel、重新 start/continue、重放 Provider 请求或夺取 Workspace Claim。Execution lifecycle、providerTerminalStatus 和 availableActions 才是控制行为的权威依据。\n\n【action 参数】\nstart(agentId, requestKey, prompt, workspaceId)\ncontinue(executionId, requestKey, prompt)\nresume_pending(executionId)\nresume_pending 仅用于已经 durable 创建、可靠证明尚未跨越 Provider side-effect boundary、未建立 Runtime attempt 的 pending Execution。Host crash、Provider/backend 不可用、binary discovery/resolution failure（均在 Runtime 创建前）都可能产生此状态，并允许 explicit resume 或 cancel-before-dispatch。\n\n必须同时满足 dispatch_pending + not_dispatched + runtime_instance_id=NULL + provider_terminal_status=NULL、原 Execution 拥有 Workspace Claim、无 persisted Runtime attempt。拒绝 dispatching/dispatched/uncertain、已绑定 Runtime、已有 Runtime attempt、已有 Provider terminal、running/finalizing/reconciling/unknown、completed/failed/cancelled/interrupted，以及 Claim missing/mismatch。\n\n只接受 exact executionId；不创建 Execution、不生成 requestKey、不 replay uncertain Provider request、不重新绑定旧 Runtime、不夺取其他 Claim。继续复用原首次 Provider pipeline；并发 duplicate resume 不得产生第二个 Runtime/Thread/Turn。\nobserve(executionId, knownRevision?, waitMs?, includeResult?)\ncancel(executionId)\nlist(agentId?, workspaceId?, limit?)\nstart.workspaceId 是调用方期望执行的当前 Workspace 身份；ActiveWorkspace 变化时必须拒绝，不得执行到新的 Workspace。括号内为 action 之外的参数，? 表示可选。只传当前 action 对应的字段；不要携带其他 action 的参数。",
        // MCP discovery uses a flat compatibility schema. Product DTO parsing
        // remains the authority for action-specific requirements and validation.
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "action": {"type": "string", "enum": ["start", "continue", "resume_pending", "observe", "cancel", "list"]},
                "agentId": {"type": "string"},
                "executionId": {"type": "string"},
                "requestKey": {"type": "string"},
                "prompt": {"type": "string"},
                "workspaceId": {"type": "string", "description": "start 必填：调用方期望的当前 Workspace ID；list 可选：筛选 Workspace。"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 100},
                "knownRevision": {"type": "string"},
                "waitMs": {"type": "integer", "minimum": 0, "maximum": 25000, "default": 20000},
                "includeResult": {"type": "boolean", "default": false}
            },
            "required": ["action"]
        })
            .as_object()
            .unwrap()
            .clone(),
    );
    agent.annotations = Some(
        ToolAnnotations::default()
            .read_only(false)
            .destructive(true)
            .idempotent(false)
            .open_world(true),
    );
    agent.output_schema = Some(agent_output_schema().as_object().unwrap().clone().into());
    agent
}
pub fn agent_contract_hash(descriptor: &Tool) -> String {
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
pub fn agent_contract_diagnostic(enabled: bool, descriptor: &Tool) -> String {
    let mut properties = descriptor.input_schema["properties"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    properties.sort();
    format!(
        "agentEnabled={enabled} agent tool contract sha256={} properties={} annotations={}",
        agent_contract_hash(descriptor),
        properties.join(","),
        json!(descriptor.annotations)
    )
}
pub fn list(upstream: &[Tool], agent_enabled: bool) -> Result<Vec<Tool>, String> {
    let agent = agent_tool();
    let mut list = vec![
        agent,
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
    if !agent_enabled {
        list.retain(|tool| tool.name != "agent");
    }
    Ok(list)
}
pub fn validate(name: &str, args: &Value) -> Result<(), String> {
    if name == "agent" {
        return Ok(());
    } // Product Service owns DTO errors and its stable envelope.
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
    fn agent_contract_fingerprint_covers_descriptor_and_ignores_object_key_order() {
        let agent = agent_tool();
        let hash = agent_contract_hash(&agent);
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, agent_contract_hash(&agent_tool()));
        let description = agent.description.as_deref().unwrap();
        for signature in [
            "start(agentId, requestKey, prompt, workspaceId)",
            "continue(executionId, requestKey, prompt)",
            "resume_pending(executionId)",
            "observe(executionId, knownRevision?, waitMs?, includeResult?)",
            "cancel(executionId)",
            "list(agentId?, workspaceId?, limit?)",
            "只传当前 action 对应的字段；不要携带其他 action 的参数。",
        ] {
            assert!(description.contains(signature), "{signature}");
        }
        assert!(!description.contains("仅 crash 前"));
        for clause in ["Provider side-effect boundary", "未建立 Runtime attempt", "binary discovery failure", "Claim missing/mismatch", "不重新绑定旧 Runtime"] {
            assert!(description.contains(clause), "{clause}");
        }
        let mut old = agent.clone();
        old.description = Some(
            description
                .split("\n\n【action 参数】")
                .next()
                .unwrap()
                .to_owned()
                .into(),
        );
        assert_ne!(hash, agent_contract_hash(&old));
        assert_eq!(
            json!(agent.annotations),
            json!({"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":true})
        );
        for field in [
            "name",
            "description",
            "inputSchema",
            "annotations",
            "outputSchema",
        ] {
            let mut value = serde_json::to_value(&agent).unwrap();
            match field {
                "name" => value["name"] = json!("changed"),
                "description" => value["description"] = json!("changed"),
                "inputSchema" => {
                    value["inputSchema"]["properties"]["waitMs"]["maximum"] = json!(24999)
                }
                "annotations" => value["annotations"]["openWorldHint"] = json!(false),
                "outputSchema" => value["outputSchema"]["description"] = json!("changed"),
                _ => unreachable!(),
            }
            assert_ne!(
                hash,
                agent_contract_hash(&serde_json::from_value(value).unwrap()),
                "{field}"
            );
        }
        let mut reordered = agent.clone();
        let mut properties = serde_json::Map::new();
        for (key, value) in agent.input_schema["properties"]
            .as_object()
            .unwrap()
            .iter()
            .rev()
        {
            properties.insert(key.clone(), value.clone());
        }
        let mut schema = (*agent.input_schema).clone();
        schema.insert("properties".into(), Value::Object(properties));
        reordered.input_schema = schema.into();
        assert_eq!(hash, agent_contract_hash(&reordered));
        let diagnostic = agent_contract_diagnostic(true, &agent);
        assert!(diagnostic.contains(&hash));
        assert!(diagnostic.contains("knownRevision"));
        assert!(!diagnostic.contains("【定位】"));
    }
    fn upstream() -> Vec<Tool> {
        SOURCES.iter().map(|(_, name, _, _)| {
            Tool::new(*name, format!("  Original {name}\n\nDetailed usage, constraints, and examples.\n中文说明。\n"), serde_json::Map::new())
        }).collect()
    }
    #[test]
    fn agent_discovery_schema_is_flat_and_stable() {
        let tools = list(&upstream(), true).unwrap();
        let agent = tools.iter().find(|t| t.name == "agent").unwrap();
        let schema = &agent.input_schema;
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["action"]));
        assert_eq!(
            schema["properties"],
            json!({
                "action": {"type":"string", "enum":["start","continue","resume_pending","observe","cancel","list"]},
                "agentId": {"type":"string"},
                "executionId": {"type":"string"},
                "requestKey": {"type":"string"},
                "prompt": {"type":"string"},
                "workspaceId": {"type":"string", "description":"start 必填：调用方期望的当前 Workspace ID；list 可选：筛选 Workspace。"},
                "limit": {"type":"integer", "minimum":1, "maximum":100},
                "knownRevision": {"type":"string"},
                "waitMs": {"type":"integer", "minimum":0, "maximum":25000, "default":20000},
                "includeResult": {"type":"boolean", "default":false}
            })
        );
        fn assert_simple(value: &Value) {
            match value {
                Value::Object(map) => {
                    for (key, value) in map {
                        assert!(
                            ![
                                "oneOf", "anyOf", "allOf", "$ref", "$defs", "if", "then", "else"
                            ]
                            .contains(&key.as_str()),
                            "{key}"
                        );
                        assert_simple(value);
                    }
                }
                Value::Array(items) => items.iter().for_each(assert_simple),
                _ => {}
            }
        }
        assert_simple(&serde_json::to_value(schema).unwrap());
        assert!(agent.output_schema.is_some());
        assert!(
            serde_json::to_value(agent)
                .unwrap()
                .get("outputSchema")
                .is_some()
        );
    }

    #[test]
    fn agent_toggle_only_adds_one_tool() {
        let disabled = list(&upstream(), false).unwrap();
        let enabled = list(&upstream(), true).unwrap();
        assert!(!disabled.iter().any(|t| t.name == "agent"));
        assert_eq!(enabled.iter().filter(|t| t.name == "agent").count(), 1);
        assert_eq!(enabled.len(), disabled.len() + 1);
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
                .filter(|t| t.name != "agent")
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
        // Existing compatibility finding, deliberately outside the Agent fix.
        assert_eq!(
            findings,
            [
                "agent output oneOf",
                "agent output $defs",
                "codegraph_explore output oneOf"
            ]
        );
    }

    #[test]
    fn fixed_surface() {
        let tools = list(&upstream(), true).unwrap();
        assert_eq!(tools.len(), 20);
        let mut expected = vec![
            "agent",
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
            20
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
            let expected: &[&str] = if tool.name == "agent" {
                &[
                    "【定位】",
                    "【什么时候使用】",
                    "【不要使用】",
                    "【生命周期】",
                    "【进度提示】",
                    "【action 参数】",
                ]
            } else {
                &["【做什么】", "【什么时候使用】", "【关键约束】"]
            };
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
    use super::*;
    #[test]
    fn one_mutating_agent_tool_with_compatibility_schema() {
        let upstream = SOURCES
            .iter()
            .map(|(_, name, _, _)| Tool::new(*name, "upstream", serde_json::Map::new()))
            .collect::<Vec<_>>();
        let tools = list(&upstream, true).unwrap();
        let disabled = list(&upstream, false).unwrap();
        assert!(!disabled.iter().any(|tool| tool.name == "agent"));
        assert_eq!(tools.len(), disabled.len() + 1);
        let agent = tools
            .iter()
            .filter(|t| t.name == "agent")
            .collect::<Vec<_>>();
        assert_eq!(agent.len(), 1);
        let agent = agent[0];
        assert_eq!(
            agent.annotations.as_ref().unwrap().read_only_hint,
            Some(false)
        );
        let input = serde_json::to_string(&agent.input_schema).unwrap();
        for action in [
            "start",
            "continue",
            "resume_pending",
            "observe",
            "cancel",
            "list",
        ] {
            assert!(input.contains(action));
        }
        assert!(agent.output_schema.is_some());
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
            "非权威 Activity",
            "silenceLevel 仅由 activityAgeMs 派生的固定展示桶",
            "fresh <30s、quiet 30s–<120s、prolonged >=120s",
            "quiet/prolonged 本身不表示 stalled、timeout、失败或卡死",
            "activityAgeMs 较大不代表 stalled、失败或卡死",
            "不得仅因 quiet/prolonged 或 Activity 时间而 cancel、重新 start/continue、重放 Provider 请求或夺取 Workspace Claim",
            "Execution lifecycle、providerTerminalStatus 和 availableActions 才是控制行为的权威依据",
        ] {
            assert!(description.contains(contract), "missing contract: {contract}");
        }
    }
}
