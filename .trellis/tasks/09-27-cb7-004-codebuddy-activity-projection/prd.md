# CB7-004 ACP Activity Projection

用户已授权开始实施；baseline feat/codebuddy c890919ff8cfedab6f73682f257ff1cc889d9027 clean。不 commit/push，不进入后续卡。

权威：当前用户冻结映射优先；implementation-task-breakdown CB7-004；technical-design §19；公共 AgentTelemetryEvent/ExecutionTelemetryProjector；CB5-003 sanitized evidence；CB7-003 prompt collector。

- 使用 pinned SDK v1 SessionUpdate/ToolCall/ToolCallUpdate/ToolKind。typed 失败 drop，不读文本补救。若 SDK 无法保留 Host kind/meta，Material Contract Difference 停止。
- publish 前 exact frame session + typed update meta conversationRequestId；private provider_request_id 存在且 update requestId 存在时 exact 匹配。Activity 不写任何 private identity。wrong/missing/malformed/stale/late 不 publish。
- Read→Read，Edit/Delete/Move→Edit，Execute→Command，其余/未来 kind→Tool。严禁 title/name/command/rawInput 推断 Test/Build；other+Read、execute+cargo test/build 必测。
- tool_call 与 tool_call_update 均按 typed status：Pending/InProgress→Tool(category)，Completed/Failed→Provider。初始 tool_call 无论上述哪种状态都记忆结构化 kind 分类，后续无 kind 增量沿用同 id；update 有 kind 则替换分类。有界单 Prompt map（queue_count），Prompt terminal/failure/drop 清空。仅流式 content/raw 无 status 不 spam；unknown id 无 kind 无法分类则 drop。
- exact agent_message_chunk/agent_thought_chunk→Provider，绝不携带文本。其余 variant 保守忽略；Usage 严格不投影。
- prompt 增加窄 AgentEventSink 入口；每批 collector drain 同步映射，不改变 private result authority。async publish 不持 DB/protocol mutex；slow/dropped sink 不改变 prompt terminal/result；observed_at 使用 now()。
- PromptResponse terminal 冻结后不 publish；允许 final drain 既有帧。forged terminal-like 字段和 permission 不影响 terminal/Claim。
- public execute 仍 unsupported；canExecute/activity/cancel/continue/tokenUsage 不变；无 schema/migration、cancel/load、Usage、Claim release、真实 probe。

验证：typed 全 kind snapshot、增量 category/状态/无 spam、有界清理、消息隐私、所有 identity 错配、late/forged terminal、recording sink 四字段快照、真实 projector activity columns 与 wrong execution 隔离、slow/dropped sink 有界测试、public result/Claim/release/Usage 不变、capability 冻结。focused mapper/telemetry + CodeBuddy 全套（含 prompt/fresh/recovery）、TaskManager/Codex regression、fmt/check/clippy、scope/diff，最终 freeze + independent read-only FULL_SCOPE review。

Host 窄修复（同一 delivery unit）：初始 tool_call Completed/Failed 不能产生 Tool Activity，与增量 lifecycle 一致发布 Provider；新增 focused 状态/缓存测试，保留 execute+cargo test→Command、other+Read→Tool。重跑 focused activity、prompt activity integration、full CodeBuddy、telemetry、含现有 Node PATH 的 broad activity、fmt/check/clippy（只记录旧 baseline）。新 target 必须独立只读 FULL_SCOPE，P0–P3=0 才 completed；不 commit/push。
