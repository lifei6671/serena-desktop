# CB6-001 Verification

## Environment

- cwd: `E:\wx_lifeilin\github.com\lifei6671\serena-desktop`
- host: Windows PowerShell
- Rust tool: `C:\Users\lifei\.cargo\bin\cargo.exe`
- Linux validation: `NOT_RUN`；本卡是 Windows discovery/Registry 文档与 Rust 改动，没有把 Host 结果声明为 Linux evidence。
- Real CodeBuddy / CB5 probe: `NOT_RUN`；未启动任何真实 CodeBuddy CLI、ACP、Runtime、Session、Execution 或 Claim。

## Required gates

| Check | Result | Evidence |
|---|---|---|
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | PASS | exit 0 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings` | PASS | exit 0 |
| `cargo test --manifest-path src-tauri/Cargo.toml 'codebuddy::'` | PASS | 19 passed, 0 failed；含 Windows ordinal non-ASCII case 与 drive-root 回归 |
| `cargo test --manifest-path src-tauri/Cargo.toml provider_policy_health_probe_has_no_execution_session_or_runtime` | PASS | 1 passed, 0 failed |
| `cargo test --manifest-path src-tauri/Cargo.toml 'provider_catalog_tests::'` | PASS | 4 passed, 0 failed |
| `cargo test --manifest-path src-tauri/Cargo.toml 'agent::task_manager::tests::'` | PASS | 29 passed, 0 failed |

上述 test 输出只有 MSVC linker import-library message，退出码均为 0。

## Additional affected-consumer check

`cargo test --manifest-path src-tauri/Cargo.toml http_providers_reuses_cb3_001_codex_product_fixture_without_runtime` 为 `ENVIRONMENT_FAILURE`：新增 CodeBuddy catalog 与 fixture 的 equality 已通过并打印正确结果，随后既有 `assert_query_output_contract` 因当前进程找不到 Node/npm 而 panic：`Query output contract tests require npm install and Node: program not found`。本卡不安装或下载 Node，也不把该失败伪报为 PASS。

较宽的 `cargo test --manifest-path src-tauri/Cargo.toml provider_policy` 同样在既有 `product_startup_consumes_persisted_disabled_provider_policy` 的 Node/npm contract helper 处出现相同环境失败；本卡直接受影响的 policy health test 已单独 PASS。

## Scope and static checks

- JSON / JSONL parse、Trellis context validation、文档引用路径检查：见最终收口命令结果。
- `git diff --check` 与新增文件 trailing-whitespace/conflict-marker 检查：见最终收口命令结果。
- 首轮独立 review 的 P1（懒 Registry 使 test override 提前失效）与 P2（ASCII path key/drive-root 折叠）均已修复并重跑 required gates；最终 review 以新 target identity 为准。
- 最终收口：上述 JSON/JSONL、引用路径、`git diff --check`、新增文件 whitespace/conflict-marker 与 frozen target freshness 均 PASS。
- `.gitignore` 和其他既有 untracked Trellis/task/editor 文件属于前置 dirty worktree，未修改、未清理、未纳入本任务交付。

## Independent review

- mode: independent, read-only, full target coverage
- target: `sha256:a5a952678c631d58cfcd612b59839d476b5a7e7ef543eab0f9c6022e41f6c0c5`
- freshness: 11/11 hashes matched before and after review
- coverage: `COMPLETE`
- findings: P0 none, P1 none, P2 none
- verdict: `APPROVE`
