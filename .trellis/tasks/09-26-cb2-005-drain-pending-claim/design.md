# Design
以现有 CB2-002 admission 和 CB2-003 local mutation 为入口，复用现有测试 fixture；只填六项真实可观察断言缺口。测试运行态必须证明停用前已运行，持有真实 Claim，并观察取消/Runtime identity；pending 恢复必须验证同一 Execution 以及派发计数。
来源哈希已核验匹配 Host 提供值。技术设计 §6.2、§6.3、§10.2、§10.3、§25.2～§25.3 是冻结契约。
若使用 fake boundary，证据明确范围；不得将仅 registry 调用计数当作真实 Claim 恢复证据。
