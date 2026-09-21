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
- 继续诊断 `cargo test --locked --lib` 的六个已报告失败：源码形态断言须
  对 CRLF/LF 与 rustfmt 稳健；CodeGraph transport fixture 须不依赖本机运行时；
  Source Read/Write 冻结错误契约不得通过修改 expected 绕过。
- 不修改既有 vendor 自动生成权限文件，不提交、推送、切换分支、改版本号或创建 tag/release。

## Acceptance Criteria

- [x] 严格 Clippy 命令退出码为 0。
- [x] `npm run lint`、`npm run build`、`npm test`、`cargo fmt --check`、
  `cargo check --locked`、`cargo test --locked --lib` 与 `git diff --check` 均已执行并如实记录结果。
- [x] 本次交付的源码修改仅在授权范围内，既有 vendor 修改未被触碰。

## Execution Record

- `cargo clippy --locked --all-targets -- -D warnings`: PASS。
- `npm run lint`: PASS；`npm run build`: PASS；`npm test`: PASS（116 passed）。
- `cargo fmt --check`: PASS；`cargo check --locked`: PASS；`git diff --check`: PASS。
- `cargo test --locked`: FAIL（1125 passed、4 failed、28 ignored）；首个失败为
  `src/agent/codex/provider/adapter_tests.rs:1338` 的 `Option::unwrap()`。
- 当时任务因完整 Rust 测试 Gate 失败保持 `in_progress`；失败不在原
  `agent_notification` 初始化修复的调用链内，故当时未扩大授权范围修复。
- 修复更新：六个目标分别单独运行后，前三个因 Windows CRLF/rustfmt 造成的
  源码文本定位失败，后三个均 PASS；单线程与修改前并行 `cargo test --locked --lib`
  均为 1126 passed、3 failed、28 ignored，未发现共享全局状态污染。
- 最小修复：三个源码形态测试改为换行/空白稳健的架构不变量检查；CodeGraph
  transport 测试改用固定 `NotPrepared` fake，以验证既有 mapper 输出
  `CODEGRAPH_NOT_INITIALIZED`，不访问本机 CodeGraph。Source Read/Write 已在
  production 路径中分别先拒绝二进制和先校验文本快照，当前无法复现报告的偏差，未改
  production 语义或 expected。
- 修复后六个目标测试 6/6 PASS；`cargo test --locked --lib`: PASS（1129 passed、
  0 failed、28 ignored）；严格 Clippy、fmt、cargo check、npm lint/build/test
  （116 passed）均 PASS；记录写入后 `git diff --check` 亦为 PASS。
- 后续 CI 根因确认：两个二进制 fixture 使用 Windows 保留设备名 `NUL.*`，使
  `fs::write` 不保证建立预期普通文件。仅改为 `contains-nul.bin` 与
  `contains-nul.txt`，保留原始 bytes 与冻结错误码；两个真实目标各连续运行 5 次均
  PASS。随后 `cargo test --locked --lib`（1129 passed、0 failed、28 ignored）、
  严格 Clippy、fmt、cargo check、npm lint/build/test（116 passed）均 PASS。
- 新增 `.github/workflows/ci.yml`：仅在手动、`master` push 与指向 `master` 的 PR
  上运行 Windows 质量 Gate，权限仅 `contents: read`，不包含 tag、安装包或发布副作用。
  `scripts/ci-workflow.test.mjs` 固定其触发器、Gate 与禁止项；新旧 workflow 契约测试
  3/3 PASS，完整 `cargo test --locked`（1129 passed、0 failed、28 ignored）及其余
  指定 Gate 均 PASS。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
