# Implementation

1. 冻结 HEAD/status 基线，排除现有 CB10-002 manual evidence；读取 provider、registry、Product、CodeBuddy typed Failure 与 refresh 路径。
2. 增加 provider-neutral admission diagnostic hook、Registry effective health/diagnostic 投影及 generic tests。
3. 增加 Product Catalog optional `diagnosticCode`，补 provider-neutral serialization/availability tests 与 Codex regression。
4. 将 CodeBuddy typed `Failure::health_change()` 接入 production execute，在 managed fake peer 覆盖 mismatch 与 transient EOF/timeout/malformed；验证同一 adapter admission/Catalog 及 refresh replacement。
5. 执行 `cargo fmt --all -- --check`、`cargo check --lib`、`cargo clippy --lib -- -D warnings`、focused registry/product/codebuddy/control tests、Product relevant full tests、前端 AgentPanel full、`git diff --check`。all-target Clippy 仅在需要确认历史 blocker 时运行并独立记录。
6. 冻结完整 executable diff/hash，交未参与实现的独立只读 FULL_SCOPE reviewer；P0/P1/P2 均修复并重审，最多三轮。
7. 记录 spec-sync 判断与交付证据，完成本任务交付。
