# 当前实施边界

Host 恢复指令已替代以下历史停止计划：原 MCD = RESOLVED_BY_HOST_DESIGN。当前实施完整 CB7-002，执行链与验证矩阵见 design.md/prd.md 及本卡 verification.md。最初 diagnostic 日志作为历史保留，不能当成当前功能验收。

1. 已核验 clean feat/codebuddy，HEAD=5e2ff1e21999a8888b29af02e4dca2773d27efe5；baseline.json 冻结源码、设计和相关既有 evidence。
2. 先验证 SDK/Host 目录契约；发现 models 无法表达后停止依赖目录的生产 vertical slice，不留半成品执行接线。
3. 添加 isolated fake-peer 诊断，仅发送一次 session/new；无 initialize capability 修改、无 prompt、无真实 CLI/Job。
4. 运行 focused diagnostic 与现有 CodeBuddy/Codex/TaskManager 回归、fmt/check/clippy、diff/scope；Windows 结果不冒充 Linux。不存在项目 Docker runner时 Linux 标 UNAVAILABLE，绝不 WSL。
5. 冻结 executable target 与 review context，独立只读 full review。本卡保持 MATERIAL_CONTRACT_DIFFERENCE，不 archive/commit/push。

待差异解决后才可实施的原卡目标：Runtime prepare/launch/policy/process/init/private R1/protocol/session durable、同一 ExternalProcessPath、early route replay、required typed config ACK、acceptance-ready ownership/drop 与 Job evidence。本轮这些未实现，不宣称测试通过。
