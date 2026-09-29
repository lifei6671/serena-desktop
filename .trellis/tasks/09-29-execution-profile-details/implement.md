# 实施计划

1. 冻结并保留上一轮 `executionProfile` Product/UI 改动；更新任务契约与交付清单。
2. 新增 schema v14、ExecutionRecord nullable 字段与 migration/future-version 回归。
3. 新增 StateStore 专用 immutable CAS 写入口及 identity/validation 测试，允许至少一个可靠字段的 partial profile，拒绝空 profile，不触碰 lifecycle/revision/Claim。
4. Codex 从 exact managed Client `model/list` 解析完整 effective profile；fresh 在 `thread/start` 前、continue 在全部可失败前置验证后且 `turn/start` 前持久化，并覆盖 validation-failure/no-evidence 与成功顺序。
5. CodeBuddy 从 exact session `SessionCatalog` ACK/current config 解析 effective profile；raw current model 的 `supportsReasoning=false` 只记录 model，否则要求 exact `thought_level`。在 fresh/continue `session/prompt` 前持久化并覆盖 partial/fail-closed/order。
6. 扩展 Product `ExecutionView` 与前端类型/详情显示，覆盖 get/list/observe 一致与 effective > requested > 固定默认。
7. 按模块运行聚焦 Rust 测试与前端测试，再运行 npm build/lint、cargo fmt/check、迁移测试和 `git diff --check`；分类记录任何 baseline/environment failure。
8. 冻结完整 target hash，使用独立 Trellis check 角色做全范围复核；若修复则重新验证、刷新 hash 并重新复核。
9. 按 Host Review P1-A/P1-B 做窄修复并重复步骤 7-8，保留上一轮未提交交付。

## 禁止项

- 不读取或调用 role defaults、Provider configuration catalog。
- 不改写 `execution_profile_json` 或其 hash/identity。
- 不从当前 UI/catalog/default 反推历史 effective 值。
- 不改变状态机、Claim、Recovery、Runtime safety、terminal 或 usage。
- 不 commit、不 push。
