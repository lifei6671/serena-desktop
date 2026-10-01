# Design

本任务不新增设计。Host 批准的唯一生产修复是把 `recover_claims_scoped` 的 Provider 过滤从 SQL 层移到既有 `load(tx, id)?` 之后、任何 Dispatch/Reconcile mutation 之前。这样悬空 Claim 继续走既有 Store error，合法其他 Provider 只读 skip，自身 Provider 复用原 generic classification。

验证按独立命令执行，原始 stdout/stderr 写入 `evidence/raw/`，摘要写入 `command-matrix.md`。Rust Cargo 默认串行执行；full lib tests 使用 `--test-threads=1`。偶发或时序疑似失败只有在精确 filter、单线程复跑后才允许分类。

非 PASS 分类限定为：

1. `REGRESSION_OWNED_BY_PHASE_X`
2. `PRE_EXISTING_BASELINE_FAILURE`
3. `ENVIRONMENT_UNAVAILABLE` / `ENVIRONMENT_MISCONFIGURED`
4. `FLAKY_OR_TIMING_SUSPECTED`

Gate 只有在新增回归为 0 且要求的自动化覆盖完整可审计时才能 PASS。已知 baseline blocker 单独列示，不描述为“全绿”。
