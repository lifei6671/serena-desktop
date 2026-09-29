# 实施计划

1. 冻结基线：记录 HEAD、完整 dirty status，并将 Windows script-path 改动排除出 delivery-owned scope。
2. 在 `protocol.rs` 的 `Limits` 增加 1 小时默认 `prompt_timeout`。
3. 在 `client.rs` 仅按 method 为 request 选择 `request_timeout` 或 `prompt_timeout`，保留 cancel override。
4. 在 `client_tests.rs` 增加 paused-time 确定性分层测试；最小调整既有 Prompt timeout 与 native cancel 测试，使两种 authority 显式。
5. 运行直接相关 CodeBuddy client/prompt/failure/recovery/cancel 测试，再运行用户指定的 compile/lint/format/diff 检查。
6. 冻结 delivery-owned 文件 hash/diff，委派独立 `trellis-check` 只读审查完整目标；只修复本任务 P0/P1，修复后重新验证与复审。
7. 不提交、不推送；报告修改文件、default authority、验证结果、Windows path touch 与 P0/P1 findings。
