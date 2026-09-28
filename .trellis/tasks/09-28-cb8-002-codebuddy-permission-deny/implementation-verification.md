# CB8-002 Implementation Verification

## 实现边界

- pinned SDK `agent-client-protocol 2.2.0` / schema `1.9.1` 的 v1 `RequestPermissionRequest`、`RequestPermissionResponse`、`PermissionOptionKind::RejectOnce`、`SelectedPermissionOutcome` 与 frozen Host permission wire 一致。Host sanitized options 未保留 display name，不构成 Material Contract Difference。生产通过官方 typed request 反序列化及 `Responder::cast::<RequestPermissionResponse>().respond(...)` 回应；没有静态 permission JSON 响应或固定 `reject` ID。
- `Shared` 中仅有当前 Runtime/Prompt 的单槽 context；原 Prompt owner 先验证 durable Runtime/Execution/Session/conversation，再注册。真正 Prompt flush 才激活；Dispatcher 收到 exact typed 初始 tool_call 才登记 known tool。终态、失败、shutdown、owner lease drop 都撤销 context；completed/failed 增量或完整工具快照撤销工具授权。
- options 必须存在唯一 typed RejectOnce，所有 optionId 非空且唯一，发送实际 advertised ID。typed parse、identity、options 任一失败关闭原 transport，交原 owner 清理；没有批准、重试、session/cancel 或 release。
- handler 不 await SQLite/telemetry；SDK 写入 guard 在物理 flush 后才释放 server-request 读窗口并交付闭集 safe event。response deadline 由原 driver maintenance 检查；后续 terminal wait 保持现有 Prompt/cancel deadline。
- 私有 `PermissionDenied` 决策通知只在 CodeBuddy owner 内持有 Runtime/Session/conversation identity。owner 的 bounded publication 先经 Store transaction 验证 exact identity 并保存固定 `CODEBUDDY_PERMISSION_DENIED` / `Provider permission denied` 诊断；成功后才向原 sink 发送既有 `AgentActivityEvent::provider`，普通 projector 负责一次 Activity 写入。identity 失效无诊断/Activity 副作用；不覆盖已有高优先级故障，不更新 terminal/release/Claim。通用 telemetry 类型与 projector 均恢复基线，不携带任何私有身份。Product 对此 code 只输出固定消息。
- owner 对 terminal、EOF、timeout 均做有界 pending permission-event drain，避免立即到达的 terminal/EOF 经 biased select 丢失已经 flush 的决策。任何 telemetry publication 不在 Dispatcher 内执行。
- deny 不改 Provider outcome；后续 exact PromptResponse 仍走既有 typed terminal staging、原 whole Job shutdown、approved evidence、atomic finalization。无 terminal 依赖原 cleanup/reconcile，证据完整为 Interrupted，否则 Unknown + Claim retained。Workspace 既有副作用保留。

## 需求到测试映射

| 契约 | 测试/验证 |
|---|---|
| official typed request/response、advertised arbitrary ID、number/string JSON-RPC id、无 allow/cancel | `client::tests::permission_sdk_typed_exact_ids_and_safe_handoff` |
| 任意 option 顺序、唯一 typed reject_once | `permission_option_order_and_unique_reject_kind` |
| 无 reject、重复 reject、重复/空 ID、缺 name、malformed/unknown kind，拒绝且无 response/event | `permission_identity_and_options_fail_closed` |
| before active Prompt、wrong session、unknown/missing tool、late terminal、stale lease、completed tool update/full snapshot | `permission_identity_and_options_fail_closed` |
| exact conversation/provider_request；缺失/错误 meta；request 自带冲突；completed request；跨 Shared Runtime 工具不可复用 | `permission_tool_identity_metadata_and_runtime_isolation` |
| 原 owner Runtime/private/session/protocol/revision 无效时不发 Prompt | 既有 `prompt::tests::rejected_identity_preflight_and_stale_revision_send_nothing`、`fresh::tests::boundary::prepared_runtime_binding_and_private_identity_enforce_occ` |
| enqueue 不等于 flush；窗口与事件只在 inner.flush 后释放；重复 flush 无二次事件 | `permission_physical_flush_failure_and_deadline` |
| 实际 write/flush error、BrokenPipe vs Io、无事件、无成功/重试 | `write_and_flush_closed_pipe_keep_first_failure_and_health_local` 新增 permission 分支 |
| 实际 1-byte pipe backpressure；SDK response 已 enqueue、driver deadline 关闭原 client；无 Activity/cancel/重试 | `permission_blocked_pipe_closes_original_client_without_safe_event` |
| deny 前写入 zero delta；deny 后保留 marker；exact cancelled -> Cancelled；end_turn -> Completed | native `execute::tests::native_permission_terminal_and_cleanup_matrix` |
| deny 后 EOF/timeout；whole Job approved evidence -> Interrupted；evidence commit failure -> Unknown + retained Claim | 同一 native matrix；只用 fixture child，无 CodeBuddy CLI |
| malformed/no RejectOnce native cleanup 无批准或 response | 同一 native matrix |
| live Job 时 deny 不产生 provider terminal/release 且 Claim 保留 | fixture 在收到 exact deny 后查询真实 SQLite；`native_permission_cancel_race_and_safe_projection` 再验证 live row/Claim |
| safe Activity 不含 raw command/env/argv，exact Store Runtime/Session/conversation identity、错误 identity 无写入 | `native_permission_cancel_race_and_safe_projection` 和 native matrix 的 `PermissionTelemetry`；`product::observe_tests::diagnostic_projection_never_exposes_raw_provider_payload` |
| deny 后 user cancel；cancel 已发后才来 permission；各 wire 最多一次，deny 不触发 cancel | `native_permission_cancel_race_and_safe_projection`、`native_permission_after_user_cancel_remains_denied`；fixture 单次 wire assertions |
| permission 本身非 terminal；后续 cancelled/refusal/max_tokens/max_turn_requests 按真实 typed reason | `prompt::tests::permission_is_not_terminal_and_typed_responses_persist` |
| CB8-001、Fresh、Activity、Recovery、Store、Product catalog、TaskManager、MCP private state 回归 | `cargo test --manifest-path src-tauri/Cargo.toml --lib codebuddy -- --test-threads=1` 全量 |

## Delivery 文件清单

生产：
- `src-tauri/src/agent/codebuddy/permission.rs`（新增）
- `src-tauri/src/agent/codebuddy/protocol.rs`
- `src-tauri/src/agent/codebuddy/client.rs`
- `src-tauri/src/agent/codebuddy/prompt.rs`
- `src-tauri/src/agent/store/transactions.rs`
- `src-tauri/src/agent/product.rs`

测试：
- `src-tauri/src/agent/codebuddy/client_tests.rs`
- `src-tauri/src/agent/codebuddy/prompt/tests.rs`
- `src-tauri/src/agent/codebuddy/execute/tests.rs`
- `src-tauri/src/agent/product/observe_tests.rs`
- `src-tauri/tests/fixtures/codebuddy_execute_child.rs`

验证日志与准确 cwd/command/exit/count 见本 task `validation-results.jsonl` 及各 log。生产 launch mode、set_mode/config、descriptor/capability、schema、CLI dependencies、Usage/Continue 均未变。

## 验证记录和限制

- 初次 production `cargo check --lib` PASS；初次 `permission_` 11 PASS。
- 初次 CodeBuddy 全量 140 PASS / 1 FAIL：旧 Activity Debug 精确快照看见新私有 field `permission_identity: None`。修复为自定义安全 Debug，旧快照保持原样，deny 只显示闭集 `permission_denied: true`；未弱化测试。
- 修复后 CodeBuddy 全量 142 PASS。随后补完整 completed-tool 快照撤销和真实 blocked response edge coverage；最终运行 `codebuddy-complete.log`，退出结果由 `validation-results.jsonl` 追加。
- native Windows fake ACP child + 真实 Job/pipe/SQLite 属于实现证据；没有启动真实 CodeBuddy，没有重跑 frozen Host probe，不声称新的真实 CLI Host evidence。
- 其余指定非 CodeBuddy 回归、最终 fmt/check/clippy、freeze/hash/scope 与独立只读 FULL_SCOPE review 由主线程接手。本报告不是最终 delivery approval；未 commit/push，未进入 CB8-003。

首轮实现交接（后续 telemetry 契约修复取代此源码快照）：`codebuddy-complete.log` exit=0，143 PASS / 0 FAIL / 0 ignored（1299 filtered），test duration 56.02s。此后实现 Agent 未再修改生产/测试代码。


## 通用 telemetry 契约回归修复

主线程 `final-telemetry.log`：10 PASS / 1 FAIL，`telemetry_contract_excludes_private_identity_evidence_and_wire_derives` 命中 forbidden `payload`。其实际契约是通用 telemetry 不能携带 Provider 私有 identity，因此未通过改注释或弱化测试绕过。已彻底删除本卡对 `provider/telemetry.rs` 与 `telemetry_projector.rs` 的改动；私有 notification 保留在 `codebuddy/permission.rs`，原 owner 先 exact Store diagnostic 后普通 Provider Activity。此前自定义 Debug 方案已删除。native matrix 增加 SQLite diagnostic 轨迹，验证 immediate terminal/EOF 仍保存固定诊断且 live Claim/terminal/release 边界保持；stale identity 私有通知的 publish 也验证普通 sink 零事件。修复后的 `telemetry` 与 CodeBuddy 全量命令记录随后追加到 `validation-results.jsonl`。
修复后最终交接：`telemetry-private-repair.log` 11 PASS / 0 FAIL；`codebuddy-private-repair.log` 143 PASS / 0 FAIL（66.72s）；两个命令均 exit=0。`git diff --check` PASS。此后实现 Agent 不再修改生产/测试源码，待主线程其余 Gate 与独立 FULL_SCOPE review。


## FULL_SCOPE round0 P1 修复（取代上述普通 Provider Activity 方案）

独立 reviewer 发现 deny 诊断在正常 Running 隐藏，普通 Activity 只能产生 processing。改为 exact Store 同事务写固定诊断及显式 `provider.permission_denied` current/history/revision；不发送重复普通 Activity。通用 telemetry 保持原契约。新增 shared closed resolver，Provider/none 才可使用 denied，Product 拒绝未知/非法组合，finalizing/reconciling 仍优先。前端固定标签“Provider 权限未获批准”。

额外文件：`agent/activity.rs`、`codebuddy/prompt/activity_tests.rs`、`product/tests.rs`（仅测试显式 import）、`src/agentPresentation.ts`、`src/AgentPanel.test.mjs`。单调 notification sequence 与 flush cutoff 仅在原 Shared/owner 内排序 Activity，不影响正文 collector。

| 修复要求 | 验证 |
|---|---|
| live denied current/history/revision、安全 Product 序列化、原 Job/Claim held、无 terminal/release | `native_permission_cancel_race_and_safe_projection`：共享 Product operation JSON boundary（MCP/Tauri 调用同一路径），Store history 独立检查，未宣称 MCP 暴露内部 history/Claim |
| 旧排队帧不覆盖、新 wire 活动恢复、历史顺序 | `permission_activity_cutoff_preserves_new_notifications` 严格 cutoff；native child 文件握手发真实新 ToolCall，history denied -> tool.read -> provider.processing |
| 非法 denied/tool pair 和未知 summary 拒绝，processing 正常 | `permission_denied_activity_contract_is_explicit` |
| 高优先级 finalizing/reconciling | `permission_denied_summary_is_closed_and_respects_priority`，既有 native terminal matrix 与 Activity lifecycle 回归 |
| 正常 Running 无诊断区仍可见拒绝，Provider ID 无关 | AgentPanel `permission denied is a visible running activity without diagnostic` |

本轮第一次 focused 编译失败：Product tests 依赖父模块 derive_summary_code import；改为测试文件明确 import，保持原测试不变。后续准确结果追加 validation-results.jsonl。

第二次 focused：19 PASS / 1 FAIL，为测试误断言 Product JSON 私有 ownsClaim 字段；按真实 serde skip 契约改为断言不泄露，Claim 保留仍直接 StateStore 验证。AgentPanel 85 PASS / 0 FAIL（66.16s），新增拒绝可见性测试通过。

最终 CodeBuddy 144 PASS / 0 FAIL / 0 ignored，61.16s，exit0。Activity 首轮 47 PASS / 12 FAIL / 1 ignored：命令 PATH 缺 Node，Product schema 校验统一报 program not found；补 C:/nvm4w/nodejs 后运行，源码未改。1 ignored 为既有显式真实 Codex smoke，本卡不运行。

Round1最终：Activity 59 PASS / 0 FAIL / 1 ignored（20.47s）；Product全模块129 PASS / 0 FAIL / 5 ignored（189.22s），均exit0；ignored均既有显式真实Codex smoke。CodeBuddy144与AgentPanel85亦PASS。此后实现者停止源码/测试修改，交主线程静态Gate、最终freeze及独立只读FULL_SCOPE复审。本轮未运行真实CLI或浏览器UI；前端证据为JSDOM实际组件行为及主线程tsc/eslint。
