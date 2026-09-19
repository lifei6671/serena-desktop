# P5-004 实施计划

1. 核对 `ExecutionView` 的已有 summary 字段与 `ProjectTaskNavigation` 的请求流。
2. 在既有 display-only presentation helper 中增加最小的列表 Provider/Usage 格式化，保留 detail DTO 逻辑不变。
3. 在任务行与 Hover 复用该格式化输出；不添加 request、Effect 或详情调用。
4. 扩展列表/Hover jsdom 测试，覆盖 unknown、partial、complete、0、历史无 Usage、分页和无详情/N+1。
5. 运行聚焦测试和要求的 `npm test`、lint、build、diff check；完成只读交付审查。
