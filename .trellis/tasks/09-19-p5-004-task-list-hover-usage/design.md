# P5-004 Task List/Hover Usage 设计

## 边界

仅改动任务列表/Hover 前端展示、现有 presentation helper 和其测试。后端 Product DTO、history API、详情加载与 Provider Runtime 保持不变。

## 数据流

`agentHistory` 已返回的 `ExecutionView` summary row 直接传给 `ProjectTaskNavigation`、`ProjectTasks` 和 `TaskItem`。Hover 同一行使用 `providerId`、`usageTotalTokens`、`usageCompleteness` 渲染，不新增状态、Effect 或请求。

## 展示规则

- Provider 由既有公共 Provider 展示 helper 接收仅含 `providerId` 的列表形状，并保留其安全 fallback；不得写死 Codex。
- Token 显示只使用 `usageTotalTokens`。null 或 unknown 显示 `—`；0 保持 `0`；partial 在数值后加“统计不完整”；complete 只显示数值。
- 历史行的 Usage 字段缺失按 unknown/null 处理；不读取详情，不合成默认零。

## 验证策略

扩展现有 jsdom 前端测试，验证 Hover 内容、分页后行的展示和对 `api.agent` / detail 的零调用；使用已有 `agentHistory` mock 作为唯一数据源。
