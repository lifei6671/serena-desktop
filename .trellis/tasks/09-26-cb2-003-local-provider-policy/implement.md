# Implementation

1. 读取任务卡、设计 §26、Supervisor config/operation 锁、ProviderRegistry/read/health、CB2-002 admission、Tauri 注册与测试。
2. 最小实现本地命令、原子持久化与 admission 更新，以及无 Runtime 的 health refresh。
3. 添加任务要求的 focused backend 测试；保留 baseline。
4. 执行 Windows 原生 cargo focused tests、cargo check、cargo fmt --all -- --check、git diff --check。按项目 CI 执行 Clippy；仅记录既有 usage_tests.rs await_holding_lock，不修复。
5. Linux 如需验证仅允许项目 Docker Desktop runner；无 runner 则 NOT_RUN，禁止 WSL。
6. 完成独立完整交付审查、记录命令与数量、核验基线未被覆盖；不提交，不进入后续任务。
