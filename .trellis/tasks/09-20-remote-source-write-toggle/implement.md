# 实施计划

1. 定位现有 Source Write descriptor 和 Dispatcher 的统一入口，加入一处可复用的六工具
   capability 判定，不触碰具体 Handler。
2. 在 Rust/TypeScript 配置模型和设置组件中加入字段、默认值及本地 Switch。
3. 扩展已有 MCP server/registry 与 config 测试，覆盖 OFF/ON、旧 catalog 和不受影响工具。
4. 运行 focused 验证后，再运行用户列出的全量验证；冻结变更并交由独立只读 reviewer。
5. 删除项目选择弹层中的管理区及其专属状态/测试；在侧栏菜单打开态显式保留工作区背景，并调整菜单的右对齐和宽度。
6. 为 HTTP 与工具调用日志附加仅供详情面板解析的安全结构化诊断；前端从列表消息中剥离诊断并在详情面板展示，覆盖参数脱敏、错误、历史日志兼容和复制行为。
