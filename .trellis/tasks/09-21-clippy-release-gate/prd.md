# 修复 Clippy 发布 Gate

## Goal

最小修复 agent_notification 测试的 Clippy warning，并执行指定发布 Gate。

## Requirements

- 仅修改 `src-tauri/src/agent_notification.rs` 中
  `terminal_policy_respects_independent_capability_toggles` 的
  `clippy::field_reassign_with_default`。
- 初始化 `ManagerConfig` 时以 struct update syntax 设置
  `agent_system_notification_enabled: false`，保留测试后续 capability 切换赋值。
- 不使用 `#[allow(...)]`，不改变业务或测试语义，不重构无关代码。
- 先通过 `cargo clippy --locked --all-targets -- -D warnings`；如该分支仍有
  Clippy warning，只做逐项最小语义等价修复直至通过。
- 随后运行用户指定的前端、Rust 与 Git 发布 Gate。
- 不修改既有 vendor 自动生成权限文件，不提交、推送、切换分支、改版本号或创建 tag/release。

## Acceptance Criteria

- [ ] 严格 Clippy 命令退出码为 0。
- [ ] `npm run lint`、`npm run build`、`npm test`、`cargo fmt --check`、
  `cargo check --locked`、`cargo test --locked` 与 `git diff --check` 均已执行并如实记录结果。
- [ ] 本次交付的源码修改仅在授权范围内，既有 vendor 修改未被触碰。

## Execution Record

- `cargo clippy --locked --all-targets -- -D warnings`: PASS。
- `npm run lint`: PASS；`npm run build`: PASS；`npm test`: PASS（116 passed）。
- `cargo fmt --check`: PASS；`cargo check --locked`: PASS；`git diff --check`: PASS。
- `cargo test --locked`: FAIL（1125 passed、4 failed、28 ignored）；首个失败为
  `src/agent/codex/provider/adapter_tests.rs:1338` 的 `Option::unwrap()`。
- 当前任务因完整 Rust 测试 Gate 失败保持 `in_progress`；失败不在本次
  `agent_notification` 初始化修复的调用链内，未扩大授权范围修复。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
