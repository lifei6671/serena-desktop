# 实施计划

1. 冻结基线：确认工作树、HEAD 与 Host 指定 SHA256。
2. 在 `windows_launcher.rs` 增加本地 file path projection + canonical identity revalidation，并在 Node LaunchRequest 构造处接线。
3. 在 `windows_launcher/tests.rs` 增加 deterministic path/identity/rejection tests 与独立 Node fixture。
4. 运行聚焦测试；若代码改动影响 discovery 断言，最小更新其测试但不改变 canonical resolved spec 契约。
5. 运行 `cargo check --lib`、`cargo clippy --lib` 和 `git diff --check`；记录首个实质错误并区分既有 blocker。
6. 冻结最终 diff/hash，执行全范围 Trellis check 与代码交付审查；仅修复本任务 P0/P1。
7. 不执行 commit/push；报告修改、验证边界及 P0/P1 结果。
