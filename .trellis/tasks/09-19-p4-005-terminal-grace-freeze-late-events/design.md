# P4-005 设计

## Authority

遵循技术设计 §34～§36 与 revision003 P4-005。Execution terminal 不等于 Usage complete；Codex 0.153.4 只能保持 `unknown` 或 `partial`。

## 边界与状态

私有 `codex_execution_usage_state.telemetry_state` 只允许 `accepting -> terminal_grace -> frozen`。进入 grace 首次固定 `terminal_at`；deadline 是 `terminal_at + 2000ms`，边界 inclusive。public `execution_usage` 的 completeness、total 和 revision 不因 lifecycle transition 自身改变。

## 流程

`ProviderTerminal` 后 best-effort 持久化 grace，但不等待。现有 recover/cleanup/finish 原顺序不变，`finish` 所在事务先使 Execution terminal 并释放 Claim。返回后，若 grace 成功开始，按其原 deadline `select!` 接收；仅 exact Usage 映射并投影，错误/EOF 仅触发 best-effort freeze。deadline 或 runtime termination evidence 都 freeze；后者在 complete_runtime 的同一 SQLite 事务内处理同 runtime 未冻结 state。

## 禁止项

不为 grace 创建新的 runtime authority、不会刷新 deadline、不会将 Codex completeness 写为 `complete`，不会将 wrong late Usage 送入旧 turn baseline invalidation。
