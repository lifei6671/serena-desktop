# Host orchestration timeout repair verification

cwd=`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`；TASK=本任务路径，精确Cargo配置和完整命令见usage-README.md的验证段。

- PASS exit0：`cargo test --offline --locked --manifest-path TASK/harness/Cargo.toml usage_`，18 passed、0 failed、16 Stage A tests filtered，1.41s。
- PASS exit0：`cargo build --offline --locked --manifest-path TASK/harness/Cargo.toml`，Windows交付binary已构建。
- PASS exit0：`cargo fmt --manifest-path TASK/harness/Cargo.toml -- --check`。
- PASS exit0：`git diff --check`。
- NOT_RUN：真实CodeBuddy/usage-repair；真实repair sentinel尚不存在。

新增4个tests覆盖缺sentinel/timeout、分类错误、old result、repair sentinel或其他repair输出存在时拒绝，合法gate只新增durable sentinel，原sentinel/timeout bytes保留，无第三mode/force/output。

usage-repair-scope.json以本轮基线核验原usage_runtime.rs完整字节前缀不变（仅追加repair函数）、usage.rs不变、所有旧evidence不变。没有Provider Usage语义修复、无Crash/生产改动。
