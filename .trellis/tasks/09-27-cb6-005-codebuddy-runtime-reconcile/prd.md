# CB6-005

## 目标与权威
严格实现用户本轮 CB6-005 冻结要求。权威为 implementation-task-breakdown CB6-005、technical-design §6.3/§23/§31/CB-006 Gate、CB6-002 launcher 与 CB6-004 private state。用户已授权建卡并开始实施，不需要再次规划批准。

仅证明并持久化旧 CodeBuddy Runtime 已停止，投影现有 ProviderReconcileKind。禁止 CB7 fresh execute、session/new、prompt、session/load/result recovery、Cancel、Usage、UI/MCP、schema/migration、commit/push。

## 验收
- sealed CodeBuddy evidence 仅来自验证原 ownership/session/job name/policy 后的 Job exact zero 或 ERROR_FILE_NOT_FOUND；PID 仅诊断。
- typed runtime persistence 固定 codebuddy 与 CB6-002 policy；SQL 留在 agent::store，schema 保持 v13。
- startup authority 注入 StateStore + Host owner；disabled/missing CLI/health refresh 不丢失恢复能力。
- execution/provider/private R1 冲突、缺失 evidence、OS 或持久化故障均 fail-closed，Claim retained。
- complete 后只通过 generic RuntimeTerminated authority interrupted + release，不进入 Codex result recovery；保留 generic ClaimRecovery outcomes。
- orphan 也收敛；不扫描/修改 Codex rows；不改 Codex runtime/recovery 语义。
- 覆盖用户指定 recovery matrix、native Windows fake Job/process tree、TaskManager/Registry 路径、Codex Windows regression。
- Gate 完成才开启 canRecover，其余五项 false，availableForNewExecution false。
- focused tests、fmt/check/clippy、scope 验证以及最终独立只读 full review；如 baseline clippy 阻塞，精确记录。

## 基线
branch feat/codebuddy；HEAD 0d96524599189b541bbcb4fc9a961ee2456f8a19；初始 git status --short 为空。CodeGraph connector 被 never approval 策略阻止，使用本地源码。Python 使用 C:/Users/lifei/AppData/Local/Programs/Python/Python312/python.exe。
