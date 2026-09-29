# 技术设计

## 边界

本功能扩展 Execution 持久化事实、Codex/CodeBuddy 派发前证据写入、Product 只读投影与现有详情 UI。生命周期、Claim、Recovery、terminal、usage 与 request identity 均保持不变。

## 数据流

```text
创建请求 -> executions.execution_profile_json -> ExecutionView.executionProfile
exact managed Runtime/session authority -> CAS 写 effective_execution_profile_json
-> ExecutionView.effectiveExecutionProfile -> ExecutionDetails 逐字段三层回退
```

## 契约

- requested 与 effective 使用同一个 provider-neutral model/reasoning 结构，但来源和 nullability 不同：requested object 永远存在；effective 整体可为 null，非 null 时两个字段独立 nullable且至少一个字段非空。
- Store 写入口接收至少包含一个可靠字段的 profile，并在事务内校验 Execution.provider、Execution.runtime_instance_id 与 runtime_instances.provider/exact id；只允许 NULL -> value 或 same-value retry。
- Provider 只使用本次真实 Runtime 内已经取得的 authority。Codex 用 managed Client 的 `model/list`；CodeBuddy 用 exact session 的 `SessionCatalog` config state。
- Provider 在产生模型执行副作用前等待 Store 提交；解析或写入失败均 fail closed。Codex fresh 在所有本地校验后、`thread/start` 前写入；continue 在 continuation/thread/history/bind/baseline 前置条件完成后、`turn/start` 前写入，前置失败不得留下 evidence。
- 前端直接显示历史字符串，不做 catalog 显示名映射；回退只在两个持久化层之间进行。
- Product 解析持久化非法 JSON 时沿用现有失败传播，不猜测默认值。
- effective profile 写入不参与 control/activity revision，不伪造 lifecycle evidence。

## 兼容性

`schema_v14.sql` 只为 `executions` 增加 nullable `effective_execution_profile_json`。历史合法 `{}` requested profile 仍投影两个 null；历史 effective evidence 保持整体 null。Continue 仍按既有规则继承 requested profile，但 child effective evidence 必须由 child Runtime 重新确认。

## UI

在 `.agent-detail-live-grid` 内复用现有 label/value 结构，不创建新卡片、不改变整体布局。每个字段独立执行 effective、requested、固定文案三层回退。

## 失败与恢复边界

- 缺少唯一默认 model、reasoning 不属于实际 model、CodeBuddy 支持 reasoning 却未确认 `thought_level`、Store identity 不一致或 immutable conflict 时终止派发；CodeBuddy raw model 明确 `supportsReasoning=false` 时允许只写 model。
- Recovery 不为历史 Execution 查询当前 catalog、不创建 Runtime、不补写证据。
- Provider 不支持或无法证明时保持 null，由 Product/UI 正常回退。
