# P4-007 Usage Regression Gate

## Goal

执行 Phase 4 Usage Regression Gate：仅验证、证据与必要的 stale test fixture 修复，关闭 Phase 4 Gate，不改 production、不提交或推送。

## Requirements

- 按 Host 固定顺序运行 P4 targeted Rust Gate、Product full、full lib、前端测试/lint/build 与 Cargo/static checks；记录真实命令、退出码和测试总数。
- G01～G17 必须逐项映射至现有真实测试或最小 integration/static test，并在 `research/verification.md` 形成可审查证据。
- 完整 lib 仅可保留 Host 冻结的三个历史失败，且必须逐项核对名称与 failure signature；Product full 仅可保留 `public_vertical_work_source_start_continue_acceptance_e2e` 的冻结签名。
- 初始禁止 production 修改。只有 P4 的公开 schema/DTO 合法变化引起 stale fixture 时，才可进行最小 test-only 修复并重跑受影响 Gate；新的 production regression 必须停止并报告。
- 保留所有既有 dirty worktree 内容；不暂存、不提交、不推送。

## Acceptance Criteria

- [ ] G01～G17 均为 PASS。
- [ ] P4 targeted 与 Product full 满足冻结失败基线。
- [ ] Full lib 为 `PASS_WITH_FROZEN_HISTORICAL_BASELINE`，且仅三个指定历史失败的签名一致。
- [ ] 前端 test/lint/build、`cargo check --locked`、`cargo fmt --all -- --check`、`git diff --check` 均 PASS，或有明确 NOT_RUN 环境证据。
- [ ] 证据记录 descriptor hash、production source hash/变更声明、所有实际命令及失败归因。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
