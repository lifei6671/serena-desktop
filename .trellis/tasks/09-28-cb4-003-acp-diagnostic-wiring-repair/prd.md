# CB4-003 Phase6 CodeBuddy ACP Diagnostic Production Wiring Repair

## Goal

把真实受管 `initialize` 的确定性 ACP `protocolVersion` 不兼容结果接入生产 Registry admission 与 Provider Catalog，使前端既有 exact diagnostic 展示获得真实 Rust authority。

## Acceptance criteria

- `AgentProvider` 提供 provider-neutral、默认无值的只读 admission diagnostic hook；只允许会阻断未来新执行的确定性 Provider contract diagnostic。
- Registry 保留 stored discovery health，但 `get()` 与 `health()` 在 diagnostic 存在时返回 unavailable；`get_registered()` 仍可供 Cancel/Recovery 使用，并提供 provider-neutral diagnostic 查询。
- Product Catalog 序列化可选 `diagnosticCode`；effective health 为 unavailable，`availableForNewExecution=false`，不得按 provider id、版本、hash、error message 或 capability 猜测。
- CodeBuddy adapter 拥有线程安全、非持久化 runtime diagnostic；只有 `Failure::Incompatible` 的 provider-owned classifier 可设置 exact `CODEBUDDY_ACP_INCOMPATIBLE`。
- EOF、timeout、invalid JSON、malformed、remote/session、permission 等瞬态/局部失败不能设置 admission diagnostic 或污染 effective health。
- mismatch 的当前 Execution 仍沿用既有失败、Runtime、Claim、terminal/recovery 收敛；只影响后续 admission 与 Catalog。
- explicit refresh 只做 discovery 并替换 adapter；成功时清除旧 runtime diagnostic，下一次 execute 再由 initialize 判定；缺 binary 仍 unavailable。
- Codex 与通用 Provider 既有 admission/catalog/control 行为保持不变。
- 前端既有 exact/near-match/no-override 测试必须真实执行，文案不改。

## Scope and non-goals

允许修改 provider port/registry、Product Catalog、CodeBuddy private adapter/protocol execution wiring、对应 Rust 测试与 native fake peer，以及本任务证据。禁止 schema/migration、持久 compatibility cache、配置 whitelist、Force Unlock/override、Product 内 `provider == codebuddy` 分支、TaskManager 字符串比较和无关重构。

CB10-002 manual acceptance 仅作为本任务触发背景；本任务交付范围为实现与对应验证证据。历史 `usage_tests.rs:987 await_holding_lock` all-target Clippy blocker 如仍存在只记录。
