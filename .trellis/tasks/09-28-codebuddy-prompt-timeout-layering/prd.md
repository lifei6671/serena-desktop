# CodeBuddy Prompt timeout 分层

## 目标

修复真实 CodeBuddy `session/prompt` 被统一 15 秒控制 RPC timeout 误杀的问题，同时保持取消、持久化、恢复、terminal/result 与 Runtime termination 既有语义。

## 需求

- 控制 RPC 继续使用现有 `request_timeout = 15s`。
- `session/prompt` 使用独立、长但有界的生产 timeout，默认 1 小时。
- Prompt timeout 仍返回 `Failure::Timeout`，由既有路径收敛到 Uncertain/recovery。
- cancel 一旦开始，仍由 cancel physical flush 与基于 `request_timeout` 的绝对 deadline 接管；不得等待长 Prompt timeout。
- 不扩大 `prompt.rs` 中 physical flush、permission publication、cancel deadline/final flush 的 timeout。
- 不改变 permission、activity、Prompt send-intent、durable MarkSent、recovery、terminal/result、Runtime termination 或 Claim release 契约。
- 保留并排除当前未提交的 Windows script-path 修复；不提交、不推送。

## 验收标准

- [ ] 普通非 Prompt request 仍在 `request_timeout` 到期时返回 `Failure::Timeout`。
- [ ] `session/prompt` 在 `request_timeout` 到期时仍保持 pending。
- [ ] `session/prompt` 在 `prompt_timeout` 到期时返回 `Failure::Timeout`。
- [ ] Prompt cancel 已开始时在既有短 cancel deadline 内收敛，不等待 `prompt_timeout`。
- [ ] 既有 Prompt failure/recovery 测试通过。
- [ ] 直接相关 client/prompt tests、`cargo check --lib`、`cargo clippy --lib`、`cargo fmt --check` 与 `git diff --check` 有明确结果。
- [ ] 最终 delivery-owned diff 完成独立只读审查，P0/P1 为零。

## 非目标

- 不全局放大 `request_timeout`。
- 不改变 Windows launcher/script-path 逻辑。
- 不新增 retry、fallback、配置项、数据库字段或公共 API。
- 不运行真实 CodeBuddy，也不把 Host 检查冒充真实 Provider acceptance。
