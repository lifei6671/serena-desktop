# Agent Management Surface Cleanup Design

## Information architecture

Agent 管理主页面只承担管理职责：
1. Agent 接入
2. 角色分工
3. 当前工作区上下文

任务创建和任务历史不再占用主内容区。现有 ProjectTaskNavigation 继续作为已存在任务的导航面；ExecutionDetails 继续承载任务控制。

## Code boundary

优先只改 AgentPanel.tsx / AgentPanel.test.mjs / App.test.mjs（如导航测试需要）。
不要删除 api.agent / agentRequests / history API，因为详情刷新、左侧任务导航及后续其它入口仍可能使用。
如果 AgentPanel 内部为了 ProjectTaskNavigation/详情仍需要 rows/history polling，应保留数据加载，但不渲染主区 history list。

## Safety

Provider pending Claim 卡片中的 查看任务 继续调用 openDetails(row.executionId,row)。
取消任务继续 operate(cancel)。
不要因为隐藏主区历史而改变 Claim、Execution、Provider 的任何 backend state。

## Tests

- main management surface has no composer/history section
- sidebar task navigation still rendered/clickable
- provider pending view/cancel remains
- task detail controls regressions remain
- provider/role management regressions remain