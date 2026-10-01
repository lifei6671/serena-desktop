# CB5-005 Final Contract Decision

## Gate result

`CB5-005 PASS with Continue supported / Usage unsupported for initial release / Crash contract frozen`。Host attempt 4 是 Usage / Crash 主证据，Host attempt 5 是 Continuation 最终权威。Attempt 1/2 保留为 `NON_EQUIVALENT_INITIALIZE_PROBE`，attempt 3 保留为 `EXACT_INITIALIZE_NON_HOST_ENVIRONMENT_PROBE`；三者仅是 diagnostic history，不覆盖 Host evidence。

## Continuation — PROVEN_SUPPORTED

- 唯一 recovery method：`session/load`。没有调用或 fallback 到 `session/resume`。
- 固定参数：`{sessionId: S1, cwd: exactWorkspace, mcpServers: []}`。
- R1 完成 P1 `end_turn` 后必须完全退出并 reap direct child；独立 R2 才能 initialize 和 load S1。
- R2 返回 `loadSession=true`；`session/load` 成功。load response 如含 sessionId 必须等于 S1；本次 response 未提供该字段。
- replay 非空且全部 `session/update` 都属于 exact S1；没有 wrong-session update。
- Host 没有向 P3 重放 P1，也没有把 marker 或 cwd 字面量放进 P3。P3 `end_turn`，回答在不保存正文的前提下证明 marker 与规范化 exact Workspace path 均存在。
- Workspace before/after manifest 完全一致。
- 固定 lineage：`sessionLineage=EXACT_S1_HISTORY_REPLAY_AND_MARKER`；`cwdLineage=EXACT_WORKSPACE_PATH_OBSERVED_IN_P3`。

因此 Continue 为 `PROVEN_SUPPORTED`，不是由旧 Runtime 存活、parent prompt replay 或第二种 recovery API 模拟得到。

## Result recovery — partial，与 Continue 分离

`session/load` 的 history replay 能恢复 exact session 的历史上下文和部分文本，但 attempt 4/5 都没有恢复 target Prompt 的 exact typed `PromptResponse` / `stopReason`。固定结论：

- `recoveryStrength=session+partial-result`
- `resultCompleteness=partial`
- `exactTargetTerminalRecovered=false`

Continue PASS 不得把 result completeness 提升为 complete。

## Public Usage — EXPLICITLY_UNSUPPORTED_FOR_INITIAL_RELEASE

Attempt 4 wire 中真实出现 9 条 `usage_update`，所以本结论不表示 Provider 没有 Usage 能力。已观察字段为 `used`、`size` 与 `_meta`；但事件不能绑定 exact Prompt identity，逐字段 scope、reset、terminal coverage 与 late behavior 均无法安全冻结。

SerenaDesktop CodeBuddy initial release 固定为：

- `publicUsage=EXPLICITLY_UNSUPPORTED_FOR_INITIAL_RELEASE`
- `tokenUsage=false`
- 公共 Usage 为 `unknown/null`，绝不把无绑定或无事件解释为 0。
- 未来只有新的 Contract Gate 能重新开启公共 Usage。

## Crash / restart — OBSERVED with bounded recovery

Attempt 4 的四个窗口均真实观察。所有窗口都能经唯一 `session/load` 恢复 Session；side-effect 窗口的 marker 保留；只有部分窗口恢复 partial text；没有任何窗口恢复 exact PromptResponse。逐窗口事实见 `crash-recovery.md`。

R2、PID absence 或 task-local direct reap 都不是 R1 Windows Job termination / Claim release authority。生产安全仍必须依赖 original Runtime 的 Windows Job evidence，例如受验证的 `job_active_processes_zero` / `managed_job_destroyed`，再按既有原子收敛规则释放 Claim。

## Provider-private persistence

`EXISTING_FIELDS_SUFFICIENT`。当前 v13 `codebuddy_execution_state` 与通用 `executions` 已能表达 source sessionId、canonical Workspace/cwd、parent/child lineage、conversation/provider request identity 及 recovery method/runtime。无需新字段、migration 或 schema version；详见 `dcr.md`。

## Downstream gates

- CB8-003：`UNBLOCKED_TO_IMPLEMENT`。本任务不进入该实现卡。
- CB9-001：`SKIPPED_UNSUPPORTED` for initial release；`tokenUsage=false`，除非未来新 Contract Gate。
- CB8-004：可在 CB8-003 完成后继续，并必须复用 attempt 4 的 Crash / Result / Job / Claim 边界。
