# P6-RG-09B MCP Source 局部 lint expectation

## Goal

仅为三个 MCP Source 测试 seam 或流式状态聚合函数添加局部 clippy 参数数量 expectation。

## Requirements

- 仅处理 `src-tauri/src/mcp/source_read.rs::read_and_recapture_with_hooks`、`src-tauri/src/mcp/source_search.rs::search_with_limits_and_hooks` 与 `src-tauri/src/mcp/source_search.rs::submit_line` 的 `clippy::too_many_arguments`。
- 每个目标函数仅添加一个局部 `#[expect(clippy::too_many_arguments, reason = "...")]`。前两处 reason 说明测试 seam 的显式 hook、取消与预算边界；`submit_line` 的 reason 说明流式搜索状态聚合的必要性。
- 不改参数、调用点、hook 语义、取消或版本捕获、搜索预算、CRLF、regex、JSON byte budget、match limit、全局 lint 配置或其他 lint；不引入 DTO 或 wrapper。
- 保留既有工作区改动；不 commit、不 push。
- 验证 Source Read/Search focused tests、`cargo fmt --all -- --check`、`cargo check --locked`、`cargo clippy --locked --all-targets -- -D warnings`、`git diff --check`；记录 strict Clippy 新暴露诊断，并完成独立只读 P0/P1/P2 评审。

## Acceptance Criteria

- [ ] 三个指定函数各有且仅有一个带准确 reason 的局部 `clippy::too_many_arguments` expectation。
- [ ] 参数、调用点、hook 语义、取消或版本捕获、搜索预算、CRLF、regex、JSON byte budget、match limit 与全局 lint 配置均未变。
- [ ] 指定验证已如实记录，三条目标诊断消失，并记录 strict Clippy 新首批剩余诊断。
- [ ] 已完成独立只读 P0/P1/P2 交付评审；未提交或推送。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
