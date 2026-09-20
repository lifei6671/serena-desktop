# 修复 Source Read 末尾换行

## Goal

修复 source_read_file 完整读取丢失末尾换行，保持 SHA、行范围和截断契约。

## Requirements

- 仅修复 `source_read_file` 的完整读取路径：未提供 `start_line` 和 `end_line` 时，返回的 `text` 必须保留原 UTF-8 文本的末尾换行语义。
- 保持既有正文 CRLF -> LF 规范化；LF 与 CRLF 文件的尾部换行均不得在读取中丢失。
- 不修改 `replace_content` 或其他 Source Write 语义，不改变完整原始 bytes 的 SHA-256 契约。
- 保持 0-based、两端包含的行范围语义；行范围读取不得因本修复引入额外行。
- 保持既有 `max_bytes` 与 `truncated` 契约。

## Acceptance Criteria

- [ ] 完整读取覆盖空文件、无尾换行、单个尾部 LF、多个尾部空行与 CRLF，且尾部换行语义正确。
- [ ] 行范围读取的现有输出语义不变，不额外追加行。
- [ ] 预算截断仍只返回完整 UTF-8 code point，并准确标记 `truncated`。
- [ ] 聚焦 `source_read` 测试、`cargo fmt --check` 与 `cargo check` 已执行并如实记录结果。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
