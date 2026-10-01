# CB7-002

用户授权目标：生产级 Fresh Session preparation vertical slice，持有 managed Runtime/session 的内部 PreparedFreshSession，完成 workspace identity、durable R1/private identity、typed new/config、early routing 后才允许 acceptance。保持 canExecute=false、execute unsupported，不进入 CB7-003，不 commit/push。

权威：implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md CB7-002；technical-design-multi-agent-provider-codebuddy-v0.1.md §14.1.1/14.1.2/14.4/15.1/17/18；CB5-003/004 sanitized Host evidence；冻结 CB6-002~005。

原始 Material Contract Difference 已按用户停止条件报告，随后 Host 批准极窄 models capture，状态 RESOLVED_BY_HOST_DESIGN。继续完整卡片；仅当已批准 shim 仍无法安全保证 exact id/method/session 关系时才再次停止。

本轮产物：正向 SDK+capture contract tests、生产内部 PreparedFreshSession、durable Runtime/preparation binding、显式 desired configuration policy、native fake-peer/Store/Job/OCC/TaskManager 回归。禁止真实 CodeBuddy probe，所有目录仅在内存。最终冻结变更并做独立只读 full review。
