# 设计

## 唯一 recovery method

ACP v1 的顶层 `agentCapabilities.loadSession` 是 `session/load` 的显式 capability。官方 pinned SDK/schema 说明 `session/load` 重放历史，`session/resume` 是另一条不重放历史的路径；本机 CodeBuddy 2.158.0 bundle 自身的 continue/reconnect 路径也调用 `session/load`。因此本任务只发 `session/load({sessionId,cwd,mcpServers:[]})`，没有 retry/fallback method list。

## Harness

使用无第三方依赖的 Node ESM task-local harness：直接 spawn 已冻结的 absolute `node.exe` 和 CodeBuddy script，逐行处理 JSON-RPC。每条入/出 wire 先递增 sequence，再写 sanitized JSONL。prompt/result/thought、授权字段、环境与 stderr 正文不落盘；保留 method、RPC id、sessionId、结构、usage 数值与临时 Workspace 代称。

Initialize params 必须逐字段等于两份冻结 CB5-004 Host PASS wire：`clientInfo={name:"serena-desktop-gold-band-repro",title:"SerenaDesktop Gold Band Repro",version:"0.1"}`，`clientCapabilities={elicitation:{form:{}},_meta:{"subagent-transcript":true,"parameterizedModelPicker":true}}`。fixture 同时固定 source wire SHA256 并对 harness generator 做结构深比较。

每个新 attempt 先执行 fresh prerequisite `initialize → session/new`。若 fresh session 失败，立即保存 wire/result/process/workspace cleanup 并停止，不发送 prompt、不调用 recovery、不进入 crash windows。只有 prerequisite PASS 才继续下述完整流。

Attempt-4 只能通过 `probe-host-attempt-4` 进入，并在任何写入/Provider spawn 前要求 `SERENA_CB5_HOST_PROBE=1`。Evidence root 固定为独立 `evidence/attempt-4`，已存在则拒绝覆盖。该 mode 继承调用它的 SerenaDesktop command Host `process.env`，并另写 allowlist-only environment provenance：固定环境键只记录 present/absent，不记录值、PATH、完整 env 或 token。

Continuation 与 Usage 共用一次最小三 prompt 流：R1 的 P1/P2 覆盖 same-session multi-turn，R1 完整退出后 R2 只用 `session/load` 恢复 S1，P3 不携带 P1 marker，只请求返回已记住的 marker 与 cwd。terminal 后观察 3 秒 bounded grace。

Crash 每个窗口使用独立 Workspace、S1、R1、R2。R1 用 `taskkill.exe /PID <pid> /T /F` 终止 direct provider process 及当时可见 descendants；这只是 task-local crash action/cleanup，不是 Windows Job-at-creation 或生产 Host proof。R2 `session/load` 只判定 session/history replay；除非 R2 wire 含 exact original prompt RPC terminal/result identity，否则 result 只能 `partial/unknown`。

Workspace manifest 记录相对路径、类型、size、SHA256；拒绝 symlink/reparse-like entry。仅当路径 resolve 后位于 `os.tmpdir()` 且 basename 使用任务前缀时才递归清理。

## 结论规则

- Continue `PROVEN_SUPPORTED`：R1 已退出、R2 load 成功、所有 session update 为 exact S1、P3 无 P1 replay 且返回 exact marker + cwd、Workspace 无意外 delta。
- Continue `EXPLICITLY_UNSUPPORTED`：capability/API 缺失或唯一 method 明确 `-32601`/unsupported。
- 其他为 `INCONCLUSIVE`。
- Usage 只有在事件可绑定 exact Prompt 且字段 scope/reset/terminal coverage 稳定时才 `PROVEN`；有事件但缺 exact prompt identity 或口径不稳定为 `INCONCLUSIVE`。只有成功执行有效 prompt 并完成 bounded grace 后仍无事件，或 Provider 明确拒绝该能力，才可评估 `UNSUPPORTED`；未到达 prompt 时无事件必须是 `INCONCLUSIVE`。unknown 绝不写成 0。
- Crash recovery 强度仅为 `session`、`session+partial-result`、`exact-result` 或 `none`；R2 永不证明 R1 Runtime 已停止。
- `session/load` capability、session/cwd continuation 与 original Prompt result recovery 分开判定；历史 `session/update` replay 在没有 exact original RPC terminal/`stopReason` 时最多是 partial。
