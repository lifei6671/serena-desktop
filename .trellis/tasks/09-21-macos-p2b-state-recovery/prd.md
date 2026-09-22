# Phase 2B：State Store 与恢复集成

## Goal

在 Phase 2A 已冻结的 macOS 进程身份与 containment 契约上，完成 StateStore migration、应用重启后的 Runtime 恢复以及 Claim 安全释放。恢复只能在身份和 containment 证据充分时终止进程或释放 Claim；证据不足必须进入 `unknown` 并保留 Claim。

## Requirements

### 数据模型与迁移

- 在现有单事务 migration 链中新增 schema v10，表达 Runtime 平台、containment 类型、身份方案及 macOS Process Group/Session 证据。
- 历史 Runtime 行按 Windows Job 语义迁移；Windows 现有字段、约束、证据类型和行为保持不变。
- `proc_pidinfo(PROC_PIDTBSDINFO)` 的原生类型只能存在于 macOS 私有 identity adapter，持久化层只保存版本化、不透明的 start token 字符串。
- 使用冻结的 v9 数据库 fixture 验证升级；migration 任一步失败时，schema、数据与 `user_version` 必须完整回滚。

### macOS Runtime 持久化

- Phase 2A 的 `macos_launcher.rs` 与 `macos_runtime.rs` 保持独立，不引入跨平台 Runtime trait。
- 成功创建 Runtime 后持久化 `PID == PGID == SID`、版本化 start token、平台与 containment 元数据。
- Runtime shutdown 继续使用 `SIGTERM → bounded grace → SIGKILL`；业务 Execution cancellation 继续使用既有 turn/interrupt，不与 Runtime shutdown 混用。
- 当前 Host 连续持有 ownership 且 direct child 已回收、Process Group 已为空时，才能写入 live group-empty 终止证据。
- spawn 后的 Store 写入失败不得遗失 Runtime ownership；调用方仍须拿回可终止的 Runtime。

### Startup recovery 与 Claim release

- macOS startup recovery 使用独立实现，不修改 Windows Runtime、Windows launcher 或 Windows recovery 行为。
- 恢复前必须重新观察 leader 身份，并精确匹配 PID、PGID、SID、start token，同时确认 leader 仍属于记录的 Process Group。
- 只有上述身份与 containment evidence 完整时才能发送 `SIGTERM`；grace 到期时，仅当 leader 仍保持同一身份时才能发送 `SIGKILL`。
- leader 消失但 Process Group 仍存在、PID/token/PGID/SID 不匹配、查询失败、Store 提交失败或其他证据不足，一律进入 `unknown` 并保留 Claim。
- 恢复开始前 leader 已不存在，即使 Process Group 已为空，也不能补造身份连续性证据，必须进入 `unknown`。
- 只有平台、containment、身份方案与 group-empty evidence 全部匹配，才能把 Runtime 判定为完整终止并通过既有事务释放 Claim。
- 人工收口继续只使用既有 Local Human Authority 入口。

### 能力边界

- Phase 2B 只打开 macOS startup recovery 能力；macOS Codex provider 的 execute/continue/cancel 仍保持 unavailable。
- Process Group 仅为 macOS managed containment scope，不宣称与 Windows Job Object 等价，也不保证约束主动通过 `setsid`/`setpgid` 脱离的后代。
- CLI discovery、环境继承和产品级 macOS 执行路径留待后续阶段。

## Acceptance Criteria

- [ ] v9 Windows fixture 可完整升级为 v10，历史 Runtime 仍保持 Windows Job 语义。
- [ ] migration 失败注入后 v9 schema、数据和 `user_version` 均保持原样。
- [ ] macOS live Runtime 能持久化身份，并覆盖正常退出、SIGTERM、SIGKILL 与 Store 失败后的 ownership 保留。
- [ ] 应用强杀后，身份完全匹配的 Runtime 可恢复终止，并以 recovered group-empty evidence 原子释放 Claim。
- [ ] leader 消失但 group 存在、PID 复用、token/PGID/SID 不匹配和观察失败时均不误发信号，Execution 进入 `unknown`，Claim 保留。
- [ ] recovery 中 leader 在 TERM 后消失但 group 仍存在时不发送 SIGKILL。
- [ ] group 查询失败、终止证据提交失败和最终 Claim release 失败均保持 fail-closed。
- [ ] startup recovery 可重复执行且不产生重复释放或状态回退。
- [ ] Windows Runtime、launcher、recovery 的既有测试与行为不变。

## Out of Scope

- 跨平台 Runtime trait 或统一 Job Object/Process Group 抽象。
- macOS Codex discovery、真实 provider 执行、continue/cancel 产品路径。
- 约束主动脱离 Process Group 的后代进程。
- 新增人工 authority、自动重试、后台守护进程或新的持久化系统。

## Notes

- Phase 2A 是本任务的前置契约；Phase 2B 不重新设计其进程创建与信号语义。
- `unknown` 是证据不足时的稳定安全状态，不是可自动推断完成的过渡态。
