# Request-sequence evidence

生产顺序由 `recovery::reconcile_execution_with_launch` 与 Store 事务共同约束：

1. 读取 original execution、R1 private state 与 generic ownership。
2. `recover(R1)`，随后 `approved_runtime(R1)` 从 SQLite 重读完整 Windows Job identity/evidence。
3. `ResumeRecovery(RuntimeTermination)` 后重读 execution/private；staged exact terminal直接进入最终双证据检查。
4. 非 staged 且 eligible 时，`begin_codebuddy_result_inspection` 在单一 IMMEDIATE transaction 内再次校验 R1、Claim、execution/private ownership，然后登记独立 R2 attempt/runtime/private provenance。
5. R2 wire 仅 `initialize -> session/load`。typed `LoadSessionRequest` 携带 exact S1、canonical cwd、`mcpServers: []`。
6. exact route replay 只接收 exact S1；assistant text还必须匹配 exact local conversation request id，以及存在时的 exact provider request id，并受 frame byte limit约束。
7. R2 shutdown 后忽略返回值，重新读取 durable R2 evidence；evidence 不完整即返回 `Unknown` 并保留 Claim。
8. Claim release 前再次重读 R1 与任何独立 R2 的 complete approved evidence，再调用 provider-neutral atomic finalize/release。

Native fake 的原始 method 日志断言每个实际 R2 只有：

```text
initialize
session/load
```

测试同时断言不存在 `session/new`、`session/resume`、`session/prompt` 或工具请求；若出现其他 method，fake 会写入 `forbidden-method` 并立即失败。Workspace marker 位于 R2 canonical cwd，恢复前后 SHA256 完全一致。

