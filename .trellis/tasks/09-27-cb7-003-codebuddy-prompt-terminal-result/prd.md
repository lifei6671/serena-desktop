开始实施 CB7-003 — Prompt / Terminal / Result Mapping。当前 branch=feat/codebuddy，基线提交 8499fef（CB7-002 已完成并提交），工作区 clean。新建 task：.trellis/tasks/09-27-cb7-003-codebuddy-prompt-terminal-result/。最终独立只读 full review；不要 commit/push。

权威：implementation-task-breakdown CB7-003；technical-design §15.3、§19.2、§21/21.1、§22 terminal semantics；CB5-003/004 Host sanitized真实wire；CB6-004 private state；CB7-002 PreparedFreshSession。

目标：在内部 PreparedFreshSession 上真正发送一次 typed `session/prompt`，验证 exact session + prompt correlation，聚合 Provider-private agent_message result，验证 exact PromptResponse terminal，并映射为 ProviderRunResult。不要在本卡做 Runtime termination/finalize/Claim release（CB7-005），不要做公共 Activity（CB7-004）、Usage、Continue、Cancel implementation。

架构边界：
- `AgentProvider.execute` 仍保持 unsupported、`canExecute=false`。CB7-003只建立内部可由CB7-005消费的 prompt/terminal primitive；不能让Product开始新执行。
- Prompt terminal本身绝不授权Claim release；ProviderRunResult不得包含 safeToReleaseWorkspace/jobEmpty/releaseEvidence/runtimeTerminated。
- 成功/terminal后的对象必须继续持有同一个 Runtime，建议 `TerminalFreshSession` / 等价：包含 PreparedFreshSession/Runtime + private state + ProviderRunResult + terminal evidence metadata，供CB7-005随后 terminate Job + generic atomic finalize。不要在返回result后把Runtime悄悄泄漏或销毁而丢失后续evidence authority。
- 非terminal/unknown prompt failure也必须持有或安全cleanup Runtime，并将 private prompt state收敛为 uncertain；不能伪造completed/cancelled。

Prompt identity / send：
1. Prompt text只能来自 frozen Execution.prompt；不要使用UI临时文本或拼接system prompt。
2. 使用 official SDK v1 `PromptRequest::new(exact_session_id, vec![ContentBlock::Text...])`；不自造JSON-RPC。
3. `_meta` 必须写 `codebuddy.ai/conversationRequestId = private.conversation_request_id`，32位小写无连字符UUIDv7已由CB6-004 durable创建；不得由prompt推导。
4. exact session id必须同时匹配 PreparedFreshSession catalog.response.session_id、private.session_id、generic execution/provider/R1 ownership；任一不一致 => no prompt。
5. 发送前 private state必须仍 Prepared，R1/protocol/session完整；使用CB6-004 typed OCC。
6. `Mutation::MarkSent{rpc_id: None}` 可以作为保守 send-boundary intent：conversation identity已durable，随后同步accepted()并调用SDK request。prompt_rpc_id是optional；不要为了填它自造id或重写SDK router。若你能在不阻塞/不破坏SDK authority的前提下精确捕获并typed持久化，可新增窄mutation，但不是本卡必需。
7. 为避免把本地frame/pending拒绝误称Provider side effect，可在MarkSent前对完整PromptRequest做与Requests.request一致的size/domain preflight；一旦MarkSent后任何发送/transport/timeout/remote不确定失败，转 `MarkUncertain`，不得自动retry同一prompt。
8. acceptance冻结顺序：CB7-002 ACCEPTANCE_READY -> durable MarkSent(send intent) -> `ProviderAcceptanceSink.accepted()` -> exactly one `session/prompt` request。不能prompt在accepted之前。

Prompt response / exact correlation：
- SDK request/response id router继续是JSON-RPC response authority。
- PromptResponse `_meta.codebuddy.ai/conversationRequestId`：Serena发送后，只有response exact回显本地conversation id才允许 `ObserveTerminal`。缺失/错值 => terminal identity不可靠，MarkUncertain，不写terminal。
- `_meta.codebuddy.ai/requestId` 若存在且非空，按 `Mutation::ExactProviderRequest` 独立持久化；不得假定等于conversationRequestId。缺失不伪造。
- PromptResponse本身不含sessionId，因此session authority来自发出请求的PreparedFreshSession exact session + SDK exact request id；不得从其它update补造。
- `ObserveTerminal` 使用 exact persisted sessionId + conversationRequestId + typed StopReason + observed_at；OCC冲突fail-closed。

Provider-private result assembly：
1. `agent_message_chunk` 允许用于 private final-result assembly，但不得在本卡投影公共Activity。
2. 必须持续消费当前 exact session route的session/update，不能等PromptResponse后才开始：CB6-003 early queue TTL/count/bytes有限，长prompt期间不消费会导致假QueueExpired。实现一个bounded collector，与prompt request并行周期性/信号驱动 drain exact route。
3. 只拼接 `sessionUpdate == agent_message_chunk` 且 frame.sessionId exact、`update._meta.codebuddy.ai/conversationRequestId` exact匹配本地conversation id的Text content。不要拼agent_thought_chunk、tool rawOutput、command/stdout、usage。
4. exact session但wrong conversation =>视为foreign/stale，drop且不能污染result；若agent_message_chunk缺失/畸形conversation metadata，不能归属当前prompt：drop并把result completeness降级（至少partial/unknown），不要猜。
5. agent_message_chunk content只接受ACP typed/已验证text形状；malformed exact-correlated chunk明确失败或taint，不能静默当完整。
6. PromptResponse到达后再做最后一次exact-route drain，冻结result；之后late update不得改变已冻结ProviderRunResult/private terminal。运行时仍由TerminalFreshSession持有等待CB7-005。
7. result建议使用稳定Provider-owned JSON，例如 `{ "text": assembledText }`；不要包含sessionId/conversationRequestId/tool details/safety evidence。若已有公共result惯例更合适可采用，但要有测试快照且不得泄漏private identity。

StopReason mapping（按ACP v1语义 + 保守completeness冻结）：
- `EndTurn` => `ProviderOutcome::Completed`。如果collector未taint，则 `Complete`；result可为exact assembled text，允许合法空文本时result=None但terminal仍Complete。若collector有无法归属/缺失chunk等taint，则不能声称Complete：降为`Partial`（有text）或`Unknown`（无text），并给稳定diagnostic。
- `Cancelled` => `ProviderOutcome::Cancelled`；result completeness默认 `Partial`（已有exact text）或 `Unknown`（无text），不能因为cancel terminal声称完整回答。
- `MaxTokens` => `ProviderOutcome::Interrupted`；Partial/Unknown，稳定diagnostic `CODEBUDDY_PROMPT_MAX_TOKENS`。
- `MaxTurnRequests` => `ProviderOutcome::Interrupted`；Partial/Unknown，稳定diagnostic `CODEBUDDY_PROMPT_MAX_TURN_REQUESTS`。
- `Refusal` => `ProviderOutcome::Failed`；exact terminal是完整失败事实，但回答正文仍只来自exact chunks。若collector clean可 `Complete`（包括None），taint则Partial/Unknown；diagnostic `CODEBUDDY_PROMPT_REFUSED`。
- StopReason是non_exhaustive：未来未知variant编译/解析路径必须fail-closed，不通过string default映射成Completed。

Unknown/transport behavior：
- Prompt request 5xx/JSON-RPC error/EOF/timeout after send intent => no automatic retry；private Sent -> Uncertain；返回/形成 conservative interrupted+Unknown diagnostic或内部PromptFailure供CB7-005 reconciliation，绝不ObserveTerminal。
- malformed terminal meta / wrong conversation / wrong session update / duplicate terminal => no overwrite；private terminal冲突fail-closed。
- permission deny本身不是terminal，不映射cancelled/failed/completed。只有exact PromptResponse stopReason=cancelled才可Cancelled。

Private state：
- MarkSent前重新read private/generic ownership，expected revision准确。
- 若response meta有provider request id，ExactProviderRequest与ObserveTerminal按OCC顺序写；不要丢revision更新。
- terminal后private prompt_state=TerminalObserved，terminal_stop_reason exact；result assembly只在内存，本卡不改schema。
- 不写thread_id/turn_id；不调用session/load/recovery result。

ProviderRunResult：
- execution_id exact。
- outcome按上表。
- result仅Provider final-result payload；无安全evidence/private ids。
- result_completeness保守。
- diagnostic_code只用稳定CodeBuddy adapter codes，不放Provider原文/路径/prompt。

Tests至少覆盖：
- exact PromptRequest wire：sessionId、单一文本prompt、conversationRequestId meta；无额外system/tool/private字段。
- accepted()严格早于prompt physical request；MarkSent durable早于request；request count=1。
- sent/uncertain/terminal private state transition与OCC。
- CB5-003 end_turn sanitized fixture：exact conversation/request meta -> Completed；agent_message_chunk按序合并；thought/tool/usage不进result。
- CB5-004 exact cancelled terminal -> Cancelled；permission deny notification本身不产生terminal。
- Refusal / MaxTokens / MaxTurnRequests typed fixture mapping。
- wrong/missing conversation response meta => no ObserveTerminal，Uncertain/Interrupted Unknown。
- wrong-session frames隔离；same-session wrong-conversation agent chunks不污染result。
- malformed/missing chunk identity taints completeness；late chunk after terminal不改变frozen result。
- unknown/duplicate response id仍由CB6-003 exact router处理，不错误完成prompt。
- timeout/EOF/Remote后不retry，request count exact1，private Uncertain。
- ProviderRunResult serialization不含safeToRelease/jobEmpty/runtime/private ids。
- TerminalFreshSession仍持Runtime；显式shutdown/drop走CB6-005 Job evidence，不释放Claim。
- AgentProvider.execute仍unsupported；canExecute/activity/cancel/continue/tokenUsage矩阵不提前变化。
- CodeBuddy full regressions、CB7-002 fresh-session、CB6-005 recovery、Codex relevant regressions。

严格禁止：
- `finalize_and_release_execution` / Claim release / generic terminal finalization（CB7-005）；
- Activity publish（CB7-004）；
- Usage ledger/projector；
- session/cancel implementation；
- Continue/session/load；
- schema/migration；
- canExecute=true；
- real CodeBuddy/CB5 probe。

验证：focused prompt/result mapping tests + CodeBuddy suite；相关 TaskManager/Codex regressions；fmt/check；clippy（既有usage_tests.rs:987 baseline精确记录）；diff/scope；freeze + independent read-only FULL_SCOPE review。若发现SDK typed PromptResponse/meta无法保留Host真实correlation字段或result chunk无法安全关联，报告Material Contract Difference并停止受影响部分，不猜字段。
