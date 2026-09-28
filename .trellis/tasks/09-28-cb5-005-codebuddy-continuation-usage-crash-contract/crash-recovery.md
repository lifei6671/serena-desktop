# Crash / Restart Contract Freeze

## Evidence authority

Host attempt 4 在 SerenaDesktop command Host 环境完成四个独立临时 Workspace 场景。每个场景都使用 installed CodeBuddy 2.158.0、exact CB5-004 initialize、唯一 `session/load`、bounded timeout 和独立 R1/R2。Attempt 1/2/3 只保留为 diagnostic history。

## Observed windows

| Window | Window observed | Session recovery | Result recovery | resultCompleteness | Marker |
|---|---:|---|---|---|---|
| after `session/new`, before prompt | yes | exact S1 via `session/load` | none | unknown | not applicable |
| prompt sent/flushed, before terminal | yes | exact S1 via `session/load` | none | unknown | not applicable |
| side effect after marker, before terminal | yes | exact S1 via `session/load` | partial text | partial | preserved with exact size/hash |
| terminal received, before summary persist | yes | exact S1 via `session/load` | partial text | partial | not applicable |

所有场景都记录 `exactPromptResponseRecovered=false`。最后一个窗口在 R1 被停止前已收到 `stopReason=end_turn`，但 R2 replay 仍未恢复 original target Prompt 的 exact typed PromptResponse；因此它仍只能是 partial result recovery。

Side-effect marker `cb5-crash-marker.txt` 在 recovery 前后均存在，size 24，SHA256 `a050db044fad75cc10989f0cc6d774e793b944abf200b603ed88ee8fba0fd685`。这只证明 Workspace 文件副作用保留，不证明 Prompt terminal/result 完整。

## Result boundary

Session continuation 与 exact result recovery 是两个维度：

- Session recovery：四窗口均 `OBSERVED`。
- Result recovery：前两窗口 `unknown`；后两窗口 `partial`。
- Exact PromptResponse recovery：四窗口均未观察到。
- Replay update 文本不能替代 original RPC id 对应的 typed `result.stopReason`。

## Production safety boundary

Attempt 4 的 direct child reap、PID absence 和 R2 successful load 只用于 task-local 诊断。它们不构成 original R1 Windows Job termination、RuntimeTerminated、Interrupted 收敛或 Claim release authority。

生产安全仍必须依赖 original Runtime 的受验证 Windows Job evidence（例如 `job_active_processes_zero` / `managed_job_destroyed`）并遵守既有原子状态转换。新 Runtime 不能证明旧 Runtime 已死，也不能仅凭 session/result recovery 释放 Claim。
