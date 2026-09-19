# P5-001～003 前端展示设计

## 边界

后端 `ExecutionView` Product DTO 是冻结事实来源。本批次只消费其 provider、progress 与 usage 字段，不对它们做控制、生命周期或统计推断。

## 展示规则

- `providerLabel` 依次取已 trim 的 `displayName`、`id`、`未知 Provider`；version 非空时仅作为紧凑后缀。
- `activityLabel` 先取 `summaryCode` 的封闭映射，再退回 phase/category；`silenceLabel` 仅报告中性活动间隔。
- `formatTokenCount` 将 null 显示为破折号、0 保持 0、其他数值做中文 locale 千分位；Total 直接读取 DTO 的 totalTokens。
- Token 用量区块放在执行信息后、结果前，呈现主 Total 和六项紧凑事实，不引入图表或新依赖。

## 验证

现有 Node + jsdom 测试模式覆盖 presentation helpers 与详情页 DOM；全量 npm test、lint、build 和 diff check 作为最终验证。截图留给 P5-005。
