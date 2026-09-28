# Design

## Data flow

`CodeBuddy Failure` → provider-owned deterministic classifier → adapter runtime diagnostic → provider-neutral Registry effective admission → Product Catalog `diagnosticCode` → existing frontend exact-code presentation.

## Minimal architecture

1. `AgentProvider::admission_diagnostic()` 默认返回 `None`，不改变 Codex/fake provider。
2. `ProviderRegistry` 继续保存 discovery health；effective health 在 adapter diagnostic 存在时覆盖为 unavailable。`get_registered()` 不读取 effective health。
3. `CodeBuddyProvider` 使用线程安全的 adapter-local state，只做 `None → exact incompatible code`；新 adapter 初始为 `None`，因此 `replace_registered()` 自然清除旧状态。
4. `Failure::health_change()` 是唯一 deterministic health classifier；`execute::run` 在仍持有 typed `Failure` 时调用 provider-owned recorder，再映射到稳定错误码。TaskManager 不识别 CodeBuddy 字符串。
5. Product 只投影 Registry 的 provider-neutral health/diagnostic，不增加 provider-specific 分支；`None` 使用 `skip_serializing_if`。

## Concurrency and lifecycle

Diagnostic 只单调设置且不持久化；refresh 通过新 adapter 清除。当前 execution 的 cleanup/finalization 代码保持原样。Registry Cancel/Recovery 路径继续使用 `get_registered()`，不受 effective admission health 阻断。
