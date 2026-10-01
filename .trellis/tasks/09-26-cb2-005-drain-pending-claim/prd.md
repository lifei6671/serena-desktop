# CB2-005
用户已授权本卡实现及必要规划。只做 contract-test，不进入 CB3-001 / Phase 3。
## 验收
证明 running survives disable（不 cancel/kill/release，provider/taskRole/state 冻结）；disabled Start 拒绝且零副作用；pending resumable 保留 Claim 且 Resume 拒绝不变；disabled/unavailable cancel 按 persisted provider registration-only 路由；重新启用同一 pending 可恢复且无重复 Runtime/Thread/Turn；disabled startup reconcile 仍执行并保持 fail-closed/Claim recovery。
证据必须包括 state/claim matrix 的各阶段 status/dispatchState/ownership/provider/taskRole/runtime attempt/identity，以及测试全名、命令、执行数量。
## 边界
只 backend focused tests、确有必要的只读 helper、此任务 artifacts。保留全部先前未提交变更；禁止 Force Unlock/UI/Remote mutation/routing 新行为/语义重写/commit/任务外 cleanup。发现冻结契约违例报告 Gate FAIL 与 owning defect，不修生产语义。
