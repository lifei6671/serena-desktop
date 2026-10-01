# Implementation
1. 阅读指定设计、当前 admission/local mutation、Claim/PendingExplicitResume/cancel/startup reconcile 代码与测试。
2. 复用现有 focused tests；新增最小 backend 合同断言，中文注释。记录状态矩阵。
3. Windows 原生执行 focused Rust tests、cargo check --locked、cargo fmt --all -- --check、git diff --check；按现有 CI 命令运行 Clippy，仅记录已知 usage_tests.rs await_holding_lock。
4. 所有命令记录 cwd、退出码、计数和日志。Linux 不需要；若需要只能项目 Docker runner，禁止 WSL。
5. 独立 read-only 子 Agent 审查本任务增量及证据；Host Gate 另行审查。保留任务供 Host，不归档、不提交、不进入 CB3-001。
