# CB8-002 Permission Deny Convergence

用户明确授权创建并实施。基线 feat/codebuddy 6749972982388cbd47633fe170e0d3797bfc05f3 clean。只做 CB8-002，不 commit/push，不进入 CB8-003。

权威：task breakdown CB8-002；technical design §18/§28；CB5-004 acceptance.json。起始 SHA256 全部与用户 pinned refs 匹配：7af023d5e5a84e3106b12db14ac44eb9b4ba6e003cd3fdc2027c2fe29176f334 / a80a5a6fa0b3d263b194f7d4604933a805475ca15761eeae86c3c7029e0e6e15 / 85e796bd3c74bfc4e839772fd2617f3e65a6c0690378830d74bea831217b6c9d。

官方 pinned ACP v1 typed request/response + SDK responder；选择唯一 advertised typed RejectOnce 的实际 optionId，不写死 reject，不选择 allow。缺失/重复/歧义/malformed options fail-closed。

只接受原 Runtime/Session/Execution/active Prompt/known toolCall exact identity；before Prompt、after terminal、stale/wrong identity 拒绝。handler bounded，只验证、typed response、安全事件 handoff，不等待 SQLite/telemetry。owner-scoped runtime-local context 清理。

flush 后安全 Activity 仅表达权限未获批准，不泄漏 command/prompt/source/env/argv。deny/Activity/flush 不是 terminal 或 release evidence。先前文件副作用保留。后续真实 PromptResponse 决定结果；无 terminal 则原 Job cleanup，经 approved evidence 才 Interrupted，否则 Unknown + Claim retained。

permission 与 user cancel 独立，permission 不生成 cancel/InterruptAck，exact terminal 优先。保持 CB8-001、Fresh/Activity/Recovery/Store/Product/MCP/Codex/Usage 契约。不新增 schema、跨 Runtime registry、UI、permission manager、永久允许、public capability。不改 CLI permission mode 或 set_mode/config。canContinue/tokenUsage 保持 false。

验收：用户完整 typed/options/identity/physical-flush/bounded-error/native fake ACP before-after-side-effect/terminal-no-terminal/evidence-failure/cancel-race/safety 矩阵及指定回归。Material Contract Difference 停止受影响实现并报告；不重跑真实 CodeBuddy probe。所有必要 Gate PASS 才 completed；最终独立只读 FULL_SCOPE review，记录 P0/P1/P2、coverage/freshness。
