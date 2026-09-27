# 设计边界

CodeBuddy recovery.rs 持有 sealed Job observation 与窄 Win32 recovery；agent/store/codebuddy_runtime.rs 持有 SQL。完成写入必须校验 observation 原 runtime identity，不能触碰 Codex usage。所有 Win32 blocking 操作使用现有 blocking worker 模式与有界 timeout。

启动按 codebuddy scoped durable work 选择。需要 generic scoped claim recovery 时保留现有 recover_claims 对外语义，共享现有 outcome 逻辑而非 Provider ID release 特判。CodeBuddy private state 存在则必须验证 R1 一致；缺失不得制造 session，也不得以缺失为 release 理由。generic ownership 是否足以安全停止 Job 与是否允许释放 Claim 分开验证并记录。

优先直接调用已有 generic finalize_and_release_execution，Finalization 为 Interrupted / RuntimeTerminated / Unknown completeness / no result；若需 helper，仅提取无 Codex result dependency 的窄 helper。Claim release 事务保持 provider-neutral。

CodeBuddyProvider 构造/注册/TaskManager build/refresh 全路径携带 recovery authority，discovery 只决定 health。单 execution 不确定转 summary，store/global contract failure 才 provider Err。只使用现有 ProviderReconcileKind。

遇到 Material Contract Difference 停止受影响部分并报告，不扩大到 CB7。
