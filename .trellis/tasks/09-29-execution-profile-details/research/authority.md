# Authority 与基线

- `execution_profile_json` 只保存创建时冻结的请求配置；本轮明确禁止用实际配置或 Provider 默认改写其语义。
- 新的 effective authority 只能来自 exact Execution 的原 managed Runtime/session，并必须在模型执行副作用前完成 immutable 持久化。
- 2026-09-29 基线 HEAD：`9bc892cdc8bd8a5e69e3d81fe9eacd6c44f54ee2`。
- 用户给出的 `product.rs`、`ExecutionDetails.tsx`、`types.ts` SHA256 均与基线一致。
- `ExecutionProfile` 已存在于 `src-tauri/src/agent/execution.rs`，字段为 `Option<String>` 的 `model`、`reasoning`，历史 `{}` 合法。
- `docs/ui/DESIGN.md` 要求紧凑开发者工具密度；本功能复用现有执行信息网格。
- 用户授权新增 v14 nullable evidence schema 与最小 CAS 写入口；同时明确禁止改变状态机、Claim、Recovery、Runtime safety、request identity、terminal、usage 以及 commit/push。
- 当前环境 `python.exe`/`py -3` 均无可用 Python，因此 Trellis helper 脚本不可执行；任务记录按现有文件格式手工维护。
- Host Review P1-A 冻结补充：effective evidence 允许 model/reasoning 独立 nullable，但非 null 对象至少包含一个可靠字段；CodeBuddy 必须从 exact raw `availableModels` 判断 `supportsReasoning`，false 时不得读取残留 `thought_level`。
- Host Review P1-B 冻结补充：Codex 允许提前解析配置，但 fresh CAS 只能紧邻 `thread/start` 前执行，continue CAS 必须晚于 resume/history/bind/baseline/name 等可失败前置并位于 `turn/start` 前；前置失败不得留下 evidence。
- 独立复核补充：CodeBuddy raw current model 的 `_meta` 只有完全缺失或合法 object 缺字段时才允许沿用 default-true；`null`、非 object 或非 boolean `supportsReasoning` 均 fail closed，且不得发送 prompt 或写 effective evidence。
