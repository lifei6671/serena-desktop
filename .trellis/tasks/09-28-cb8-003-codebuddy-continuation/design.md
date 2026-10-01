# Design

保持 generic continuation authority 不变：TaskManager 创建 child 并传入 `ProviderContinuationContext { source_execution_id }`，CodeBuddy `validate_continuation` 只补 provider-private source eligibility，`execute` 只按是否存在 `parent_execution_id` 选择 fresh/continued prepare。

Fresh 与 Continue 各自拥有窄 prepare primitive。两条路径在 prepare 完成后汇合到现有 Runtime/client、mode/config、durable prompt、acceptance、terminal/finalization、cancel/permission/activity 和 Job convergence 流程。

Continue prepare 顺序：读取 source generic/private snapshot并验证 exact provider/task/workspace/generation/parent lineage；生成 child 独立 private identity；创建 R2；initialize；确认 `loadSession`; 在 load 前安装 exact-S1 replay route；发送唯一 typed `session/load`; 校验 response sessionId（若存在）、所有 replay sessionId、required non-empty usable history；把 child durable recovery 生命周期绑定到 `session/load` / R2 / exact S1；随后才进入现有 prompt durable send-intent与 acceptance。

所有失败均关闭当前 child Runtime并返回稳定 Continue/provider error；不得调用 fresh session/new。source Runtime 是否存活、source provider_request_id 是否存在均不构成资格条件。Recovery replay 仅证明 continuation lineage，不合成原 Prompt terminal/result，不改变 Claim release authority。

Store 只允许添加组合读取或既有字段事务 helper，不新增 migration。public catalog 的 Windows `canContinue` 仅随完整生产实现开放；Product `canContinue` 仍由 core eligibility、当前 provider policy/health/capability 和 provider validate 共同决定。

若实际代码缺少完成合同所必需的公共 authority，先记录 Material Contract Difference，停止受影响实现，不自行扩大公共 API。
