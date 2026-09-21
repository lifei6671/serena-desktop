# Journal - codex (Part 1)

> AI development session journal
> Started: 2026-09-21

---

## 2026-09-21｜macOS Phase 1 可编译基线

- 将 Agent 产品层从 Windows Runtime 的模块裁剪中解耦；非 Windows Codex Provider 显式 unavailable，不启动、不恢复、不伪造 Runtime 证据。
- 保留 Store/List/Observe/History 与纯业务测试；Windows Runtime、recovery、Fake wire 测试使用精确函数级 `cfg(windows)`。
- macOS Serena 长驻 Runtime 在 `Command::spawn` 前 fail-closed；Unix Source Write 与平台相关 lint 已修复。
- 产品动作契约：Start/ResumePending 为 `BACKEND_UNAVAILABLE`，Continue 为 `AGENT_CONTINUE_NOT_ALLOWED`，Cancel 为 `AGENT_PROVIDER_UNAVAILABLE`；缺少终止证据时 Claim 保留。
- 前端 Gate：`npm ci`、lint、build、116 tests 通过。`npm ci` 报告当前 Node 22.22.0 比 jsdom 声明的 22.22.2 低一个 patch，但命令与后续 Gate 均 exit 0。
- Rust Gate：fmt、check、test no-run、clippy all-targets `-D warnings` 通过；完整测试 927 passed、0 failed、12 ignored。
- Spec 判断：临时 unavailable 契约已记录在 task design/implement 与 checklist；当前无 backend spec layer，不新增全局 spec。
- 限制：这只是 macOS 可编译基线，不代表 Codex Agent Runtime 已可用；真实 Runtime/恢复进入 Phase 2。


## Session 1: macOS Phase 1 可编译基线
<!-- trellis-session: v=2 fp=16995548f4c5d229 -->

**Date**: 2026-09-21
**Task**: macOS Phase 1 可编译基线
**Branch**: `dev/macos`

### Summary

完成 macOS Rust 可编译基线：非 Windows Codex 能力显式不可用，Runtime 动作稳定失败且不启动进程；Windows 行为保持不变。前端 116 项与 Rust 927 项测试通过，fmt、check、clippy 全部通过。

### Git Commits

| Hash | Message |
|------|---------|
| `349e34f` | fix(macos): establish rust compile baseline |

### Status

[OK] **Completed**

## 2026-09-21｜macOS Phase 2A Runtime 进程契约验收

- 目标：验收当前 Host 从创建到终止连续持有 ownership 时的 macOS Codex launcher、进程身份、Session/Process Group containment 与有界 shutdown；本阶段不接入产品 Provider、StateStore、Workspace Claim 或 Startup Recovery。
- 关键实现边界：固定 executable 与逐项 argv，不经过 shell；三条 stdio 显式 pipe；`pre_exec` 调用 `setsid()` 并在 child/parent 两侧验证 `SID == PGID == PID`；私有 identity adapter 绑定 PID、PGID、SID 与启动令牌；shutdown 仅执行 `SIGTERM → bounded grace → SIGKILL` 进程收口，完整 evidence 同时要求直接 child 已回收且原 Process Group 为空。
- Rust Gate：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` exit 0；`cargo check --manifest-path src-tauri/Cargo.toml --locked` exit 0；`cargo test --manifest-path src-tauri/Cargo.toml --locked --no-run` exit 0；`cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings` exit 0；`cargo test --manifest-path src-tauri/Cargo.toml --locked` exit 0。Rust library 共发现 953 项测试：941 passed、0 failed、12 ignored；main 与 doc-tests 均为 0 项。
- 真机 Gate：`uname -m` exit 0，输出 `arm64`；`sw_vers -productVersion` exit 0，输出 `26.5.2`。因此本轮通过 Apple Silicon、macOS 12+ 当前主机 Gate；证据只覆盖实际 macOS 26.5.2，不声称最低 macOS 12.0 已实测。
- 前端 Gate：`npm run lint` exit 0；`npm run build` exit 0，Vite 转换 2244 个 modules；`npm test` exit 0，116 passed、0 failed、0 skipped。
- 范围审计：`git diff --exit-code 9153995 -- src-tauri/src/agent/codex/windows_launcher.rs src-tauri/src/agent/codex/runtime.rs` exit 0；`git diff --name-only 9153995` exit 0；`git diff --check` exit 0。Windows Runtime 两个源文件相对基线无变化；结合 `git status --short --untracked-files=all` 复核，没有 StateStore、schema、Claim、Provider、Pool、Discovery 或 Startup Recovery 文件变化。Windows 真机行为仍由既有 Windows CI 负责，本阶段没有修改其源文件或错误码。
- Fixture 清理：`pgrep -lf macos-runtime-child` exit 1 且无输出，未发现残留 fixture 进程。
- Trellis check：完整阅读 `.trellis/workflow.md` Phase 2.2 与 Phase 3.3；`python3 ./.trellis/scripts/get_context.py --mode packages` exit 0，项目未配置 package、仅有 `frontend` spec layer；当前变更是 macOS 私有 Rust 进程实现，不跨前端层。`python3 ./.trellis/scripts/task.py validate .trellis/tasks/09-21-macos-p2a-runtime-contract` exit 0，`implement.jsonl` 与 `check.jsonl` 各 3 个条目且全部有效。全量 Gate、任务契约与受影响文件复核无发现。
- Spec 判断：不更新 `.trellis/spec`。本任务的 launcher、identity、Process Group 与 live-host ownership 决策已冻结在当前 task 的 PRD/design/research/implement 中；仓库现有 spec 结构只有 frontend，没有对应 backend/Rust layer。为单个 macOS 私有模块新建不一致层会扩大范围，也不会形成已有项目级惯例。
- 状态：源码已提交为 `452dc61`，Phase 2A task 已归档并记录 Session 2；Trellis 收口改动进入本次独立 bookkeeping 提交，不推送远端。
- 已知限制：完整 evidence 仅适用于 live-host continuous ownership；Process Group 不约束主动调用 `setsid()` 或 `setpgid()` 逃逸的后代；Phase 2A 不包含 StateStore migration、Workspace Claim release 或 Startup Recovery，这些仍属于 Phase 2B。


## Session 2: macOS Phase 2A Runtime 进程契约
<!-- trellis-session: v=2 fp=ee77a7f104ab5721 -->

**Date**: 2026-09-21
**Task**: macOS Phase 2A Runtime 进程契约
**Branch**: `dev/macos`

### Summary

实现并验证 macOS live-host launcher、进程身份和有界终止契约，保持 Windows 与持久化恢复边界不变。

### Main Changes

- 新增 macOS setsid launcher、stdio ownership、私有 libproc identity adapter 与 Process Group 查询。
- 新增 SIGTERM 到 grace 到 SIGKILL 的有界 shutdown、unknown ownership 和完整终止证据。

### Git Commits

| Hash | Message |
|------|---------|
| `452dc61` | feat(macos): add live runtime process contract |

### Testing

- [OK] Rust 941 passed、0 failed、12 ignored；前端 116 passed；Clippy、构建与范围审计通过。
- [OK] Apple Silicon arm64、macOS 26.5.2 真机进程测试通过且无 fixture 残留。

### Status

[OK] **Completed**

### Next Steps

- Phase 2B 设计 StateStore migration、startup recovery 与 Claim release；证据不足时保持 unknown。
