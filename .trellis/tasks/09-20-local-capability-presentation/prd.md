# 本机能力展示边界

## Goal

移除项目页按工作区探测和展示能力提供者；服务状态页仅展示本机命令或服务是否已发现，不把工作区 Runtime、准备或可调用性投影为本机状态。

## Requirements

- 首页只展示项目与连接上下文；删除按选中工作区读取的能力提供者、准备动作和本机服务区块。
- 首页不得调用 `workspace_capability_observe`、`workspace_capability_prepare` 或 `workspace_capability_cancel`，也不得订阅 capability activity。
- “服务状态”页展示本机可发现的命令或服务：Git、Serena、CodeGraph、Codex CLI；MCP Broker 仍展示 Serena Desktop 自己的监听状态。
- Git、Serena、CodeGraph 和 Codex 的展示只能依据本机检测结果，不得读取或显示 active workspace、Workspace RuntimeSlot、索引准备度、Provider 可调用性或进程生命周期。
- 不修改 Workspace 请求授权、Capability Manager、MCP 路由或本机检测的后端实现。
- 移除侧栏顶部的 `NAVIGATION` 分组标签；工作区任务区域标题改为“工作区”，且标题在工作区列表滚动时保持可见。
- 工作区行悬浮或获得焦点时，在折叠图标前显示三点菜单；菜单提供带图标的“编辑”和“删除”。编辑可修改工作区名称；删除须保留本机目录和仓库，并使用已有移除语义。

## Acceptance Criteria

- [ ] 进入、离开或切换首页不会发出任何 `workspace_capability_*` IPC 调用。
- [ ] 首页不存在能力提供者与本机服务区块；项目页保留项目管理和连接配置。
- [ ] 服务状态页将 Git、Serena、CodeGraph、Codex 明确呈现为本机命令或服务发现，不显示工作区绑定或 readiness/runtime 状态。
- [ ] 服务状态页仅为 MCP Broker 显示真实运行/监听状态。
- [ ] 侧栏不显示 `NAVIGATION`，工作区标题不会随工作区列表滚动离开可视区域。
- [ ] 每个工作区的三点菜单可编辑名称或确认删除，操作后由现有状态刷新同步侧栏。
- [ ] 相关前端测试与 TypeScript 构建通过。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
