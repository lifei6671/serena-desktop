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
