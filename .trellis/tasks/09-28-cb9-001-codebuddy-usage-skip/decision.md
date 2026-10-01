# CB9-001 正式决策

## Gate 结果

`SKIPPED_UNSUPPORTED`。这不是 CodeBuddy Usage implementation PASS，也不表示 Provider 没有发出 Usage 事件。

CB8-004 已完成，dependency 满足；但 CB5-005 的 Usage Contract Gate 没有通过。CB9-001 的权威接受条件要求 Usage contract 与 implementation 同时 PASS 后才能令 `tokenUsage=true`，因此本条件任务以允许的 unsupported 终态收口，不实施 CodeBuddy Usage projector，也不进入 CB9-002。

## 为什么不能实现

- CB5-005 Host attempt 4 真实观察到 9 条 `usage_update`，字段为 `used`、`size`、`_meta`；这只能证明事件存在。
- 冻结分析为 `exactPromptBound=false`、`publicConclusion=INCONCLUSIVE`、`tokenUsageCapability=false`。逐字段 `scope`、`resetBehavior` 均为 `unknown`，`terminalCoverage`、`lateBehavior` 均为 `unknown_unbound`，`exactPromptIdentity=null`。
- Fresh、多轮、重启和 continuation 附近虽有样本，但无法把数值变化安全解释为 per-turn 或 cumulative，也无法证明 reset、terminal coverage、late event 或 exact Serena Prompt identity。
- 缺少已证明的公共合同，因此没有合法输入可供实现 Provider-owned CodeBuddy Usage projector；猜测映射会违反 null 不得转 0、能力不得超过 evidence 的约束。

## 冻结 public contract

- `publicUsage=EXPLICITLY_UNSUPPORTED_FOR_INITIAL_RELEASE`。
- `ProviderCapabilities.token_usage=false`；Product capability 为 `tokenUsage=false`。
- CodeBuddy public Usage 为 `unknown/null`；缺少事件或缺少绑定不能解释为 0。
- 不创建、读取或更新 `codex_execution_usage_state`，不复用 Codex Thread epoch、baseline、terminal grace 或 freeze 语义。
- 不把 CodeBuddy UsageEvent 写成 `provider_id='codex'`。
- Product 对非 Codex Provider 只读取 provider-neutral public Usage；不存在 public Usage 行时稳定返回 unknown/null。

## 未来重新开启所需的新 Usage Contract Gate

未来只有新的、独立冻结的 CodeBuddy Usage Contract Gate 同时满足下列条件，才能重新开启 CB9-001 implementation：

1. 在 fresh、multi-turn、Runtime restart、`session/load` continuation、terminal ordering 与 bounded late window 中取得真实 wire evidence。
2. 每个公开字段都能绑定 exact Session 与 exact Prompt identity，并钉死字段含义、单位、per-turn/cumulative scope、reset 和重复/迟到事件规则。
3. 明确无事件、缺字段、乱序、restart/continue 和 identity mismatch 时的 unknown/partial/complete 投影规则，继续禁止 null→0。
4. 通过 Provider-owned projector 写 `execution_usage.provider_id='codebuddy'`，不得调用任何 Codex private Usage helper。
5. Contract tests 与 implementation tests 均 PASS 后，才可把 `token_usage` 改为 `true`；任一项未通过仍保持本次冻结结论。

## 下游状态

CB9-002 的 dependency 条件为 `CB9-001 or SKIPPED_UNSUPPORTED`，因此本收口后 CB9-002 为 `UNBLOCKED`；本任务没有进入或修改 CB9-002。
