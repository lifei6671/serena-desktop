# Phase 2A：macOS Runtime 进程契约

## Goal

在不改动 Windows Runtime 行为的前提下，冻结并实现当前 Host 连续持有 ownership 时的 macOS Codex 进程创建、stdio、Session/Process Group containment、进程身份与有界终止契约。

## Requirements

- 依赖：Phase 1 可编译基线已完成并归档。
- 新增独立的 `macos_launcher.rs` 与 `macos_runtime.rs`；不引入跨平台 Runtime trait，不重写 `runtime.rs` 或 `windows_launcher.rs`。
- 使用固定 executable + argv 启动 Codex，不经过 shell command string；只显式配置受管 child 的 stdin/stdout/stderr 管道。
- 在 `exec` 前调用 `setsid()`，并验证受管 leader 满足 `SID == PGID == PID`。
- 显式验证 launcher 的 executable、argv、cwd、空字节与超长输入，错误必须稳定且可测试。
- macOS 进程身份必须同时绑定 PID、PGID、SID 和启动令牌；PID 相同但启动令牌不匹配时不得通过身份验证。
- 如使用 `proc_pidinfo(PROC_PIDTBSDINFO)`，必须封装在 macOS 私有 identity adapter 中，不将 libproc 类型暴露为 Runtime 公共契约，并在目标 macOS 版本执行真实测试。
- Runtime shutdown 只负责进程收口：先向受管 Process Group 发送 `SIGTERM`，进行有界宽限等待，仍未退出时发送 `SIGKILL`，随后验证直接 child 已回收且 Process Group 为空。
- 业务 Execution cancellation 继续使用既有 `turn/interrupt`；本任务不以 OS signal 替代业务取消协议。
- Process Group 仅作为 macOS managed containment scope，不宣称与 Windows Job Object 等价，也不宣称能够约束主动通过 `setsid()` 或 `setpgid()` 脱离的后代。
- group-empty evidence 只适用于当前 Host 从创建到终止连续持有 ownership 的 Runtime。身份、containment 或观测证据不足时返回 `unknown` 并保留 Runtime ownership。
- 本任务不修改 StateStore、Workspace Claim、Startup Recovery、Provider 注册或数据库 schema。

## Acceptance Criteria

- [ ] macOS launcher 对固定 executable + argv、cwd、stdio 和输入边界具有自动化测试。
- [ ] launcher 在 `exec` 前建立独立 Session/Process Group，并验证 `SID == PGID == PID`。
- [ ] 私有 identity adapter 能产生并校验不暴露 libproc 类型的启动令牌；伪造 PID/token 或不完整身份不能通过。
- [ ] shutdown 按 `SIGTERM → bounded grace → SIGKILL` 收口，并验证直接 child 与受管 Process Group 均退出。
- [ ] 启动完成后立即 shutdown 和运行期 shutdown 均不遗留未主动脱离 containment 的 child/grandchild。
- [ ] 无法确认终止时只返回 `unknown` 并保留 Runtime ownership，不产生完整终止证据。
- [ ] Windows Runtime 源文件、行为和错误码保持不变，现有验证不回归。
- [ ] 在目标 macOS 12+、Apple Silicon 环境完成 launcher、identity 和 process-group 真实测试。

## Deferred to Phase 2B

- macOS Runtime 的 StateStore migration 和持久化证据结构。
- Startup Recovery、跨 Host/重启后的进程身份判断和收口策略。
- Workspace Claim release 查询和人工收口集成。
- leader 消失但 Process Group 仍存在、PID/token 不匹配或证据不足时的持久化 `unknown` 状态；Phase 2B 必须保留 Claim，且不得杀进程或伪造终止证据。
