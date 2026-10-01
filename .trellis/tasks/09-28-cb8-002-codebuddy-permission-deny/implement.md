# Implementation and verification

1. 核对 pinned SDK typed permission API / frozen wire。
2. owner-scoped context、typed deny、physical flush 后安全 Activity、lifecycle 清理。
3. typed/options/identity/transport tests；native fake ACP before/after marker、cancelled/end_turn/no-terminal/EOF、evidence failure、cancel race。
4. Native Windows cargo test --manifest-path src-tauri/Cargo.toml --lib codebuddy -- --test-threads=1；Store transactions、TaskManager、Product catalog、MCP catalog、Codex cancellation、telemetry、same_runtime、usage filters。
5. cargo fmt/check；clippy --lib --tests -- -D warnings 只允许既有 usage_tests.rs:987 baseline；git diff --check、freeze/scope。
6. freeze 文件/hash 与验证上下文；独立只读 FULL_SCOPE review；必要时修复、重验、重审。Gate 满足才 completed。

Linux 仅项目 Docker runner，无 runner 则 UNAVAILABLE，禁止 WSL。PATH 可局部添加 C:/Users/lifei/.cargo/bin 和 C:/nvm4w/nodejs。无 commit/push、真实 CodeBuddy probe。
