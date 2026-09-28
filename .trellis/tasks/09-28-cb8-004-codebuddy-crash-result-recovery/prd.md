# CB8-004 CodeBuddy Crash / Result Recovery Gate

用户已明确授权创建并实施本卡。基线为 `feat/codebuddy` / `a42176717c33ea08c7f5be3f6fc96107f85fe578`，起始工作区 clean。只做 CB8-004，不 commit/push，不进入 Phase9。

## 权威合同

- `docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md` 的 CB8-004。
- `docs/technical-design-multi-agent-provider-codebuddy-v0.1.md` §23。
- CB5-005 final `decision.md`、`crash-recovery.md`、`review-final.md`。
- CB8-003 production `session/load` Continue 已完成，但 Result Recovery 使用独立 authority 和独立 R2。

## 必须行为

1. 先验证 original execution、R1 private/generic ownership，再取得并重新读取 durable approved R1 Windows Job evidence；缺失时 `unknown + Claim retained`，不得创建 R2。
2. 只有 R1 termination 已 durable approved，才创建独立 Result-Recovery R2；R2 仅允许 `initialize → session/load(exact S1, exact canonical cwd, mcpServers:[])`。
3. R2 不允许 `session/new`、`session/resume`、`session/prompt`、工具或 parent/current prompt replay；只接收有界 history replay。
4. Result Recovery 必须绑定 source private exact S1 与 exact local `conversation_request_id`；optional `provider_request_id` 仅作额外 exact identity。
5. replay assistant text 至多 `Partial`；`Complete` 只来自 original R1 durable private + generic staged exact terminal/result。
6. R2 自己必须安全 shutdown，并持久化独立 complete Job termination evidence；若 R2 evidence 不明，Execution unknown + Claim retained。
7. R1/R2 均安全后，无 staged exact terminal 时最终 `Interrupted`；exact conversation 非空 assistant text可为 partial，否则 unknown；以 `ReleaseBasis::RuntimeTerminated` 原子释放 Claim。
8. R1/R2 runtime identity 独立：generic `execution.runtime_instance_id` 始终保持 R1，private `recovery_runtime_instance_id` 记录 R2，R2 必须登记为 durable runtime attempt。
9. startup reconcile 通过 registered CodeBuddy 历史 recovery 工作，不依赖当前 enabled/health/CLI admission；restart twice 幂等，orphan R2 仍由 provider orphan recovery 捕获。

## Crash 窗口与验收

- A：prompt 前。
- B：prompt physical flush 后、terminal 前。
- C：side effect 后、terminal 前；R2 不得修改 marker/manifest。
- D：exact terminal observed / generic staged terminal 后、final persist/release 前；优先恢复 staged exact terminal，不被 replay 降级。
- 覆盖 missing/invalid R1 proof、R2 load failure/empty/wrong session、exact/foreign conversation replay、R2 termination failure、disabled/unavailable startup、restart twice、request sequence 和 dual-evidence release assertions。

## 禁止项

- schema/migration、Phase9 Usage、真实 Provider 重跑、fallback/retry、从 replay 合成 original terminal、PID/direct reap authority。
- 改绑 generic R1、以 session/load success 授权 release、以 `RecoveryState` 替代 lifecycle terminal。
- commit、push、部署或扩大到相邻卡。

最终冻结全部本卡差异，由未参与实现的独立只读 `CHILD_AGENT` 做 `FULL_SCOPE` review；P0/P1/P2 全部修复并完整复审。
