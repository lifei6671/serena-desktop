# Implementation and verification

1. 核对 CB5-003 sanitized activity shape 与 typed SDK，记录研究与来源 hash。
2. 实现私有 mapper 与内部 prompt telemetry；更新 prompt 调用，增加行为与集成测试。所有新增函数/核心逻辑中文注释。
3. Windows cargo focused tests、CodeBuddy suite、telemetry/TaskManager/Codex regressions、fmt/check/clippy。记录 cwd、命令、exit、counts、首个错误；不得 WSL；无 Docker runner 不声称 Linux 通过。
4. Baseline 已知但仍需本轮核对：usage_tests.rs:987 await_holding_lock；CB7-003 记录 TaskManager/Codex 各一个 orphan Claim regression。不得越界修复/忽略测试。
5. 冻结所有 delivery-owned source/test/fixture hash，context 独立 hash；主 Agent 发起独立只读 FULL_SCOPE review，检查 freshness，再完成报告。不 commit/push/archive。
