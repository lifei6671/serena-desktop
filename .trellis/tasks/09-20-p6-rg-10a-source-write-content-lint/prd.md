# P6-RG-10A Source Write Content 参数 lint expectation

## Goal

仅处理 `src-tauri/src/mcp/source_write_content.rs` 当前 `clippy::too_many_arguments` 条目：为测试内 `replace` helper 添加最小的局部 expectation。

## Requirements

- 先确认 helper 的完整定义、所有调用点与 `cfg(test)` 边界；若存在 production 调用或更自然的既有参数对象，停止并报告，不重构。
- 仅添加 `#[expect(clippy::too_many_arguments, reason = "...")]`；reason 说明 helper 显式承载测试输入、故障注入与写入边界。
- 不改变参数、调用点、Source Write 行为、`expectedSha256`、取消/故障注入/原子替换语义、全局 lint 配置。
- 不处理 `permissions_set_readonly_false` 或其他 lint；不提交、不推送。

## Acceptance Criteria

- [ ] `replace` helper 有且仅有一条准确的局部 `clippy::too_many_arguments` expectation。
- [ ] 参数和调用点字节保持不变，生产调用与 Source Write 语义未变。
- [ ] 完成 Source Write Content focused tests、`cargo fmt --all -- --check`、`cargo check --locked`、`cargo clippy --locked --all-targets -- -D warnings` 与 `git diff --check`；记录目标条目消失和剩余权限 lint。
- [ ] 已完成独立只读 P0/P1/P2 评审。
