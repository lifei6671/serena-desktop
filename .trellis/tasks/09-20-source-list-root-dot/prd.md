# 支持 source_list_dir 根目录枚举

## Goal

允许 source_list_dir 在已捕获 workspaceId 的 WorkspaceLease 内以 relative_path='.' 枚举工作区根目录；保持路径越界防护，并添加回归测试。

## Requirements

- `source_list_dir` 在 `workspaceId` 已解析为请求级 `WorkspaceLease` 后，必须接受 `relative_path: "."`。
- `"."` 只表示该 Lease 的 canonical Workspace root，绝不能读取或枚举其他 Workspace、调用方当前目录或绝对路径。
- 除 `"."` 外，既有相对路径规范化、父目录穿越、绝对路径、UNC 路径和 junction/reparse 越界防护保持不变。
- 保持既有输出结构、递归、预算和取消语义，不改变其他 Source Tool 的行为。
- 为根目录枚举增加回归测试，证明结果来自请求 Lease 的根目录。

## Acceptance Criteria

- [ ] 带有效 `workspaceId` 和 `relative_path: "."` 的 `source_list_dir` 成功返回该 Workspace 根目录的直接条目。
- [ ] 测试证明根目录请求不依赖进程当前目录，且响应保留正确的 Workspace provenance。
- [ ] 既有路径拒绝和 `source_list_dir` 行为测试继续通过。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
