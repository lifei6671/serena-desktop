# Verification

统一 cwd：`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`。所有最终证据均为 native Windows；未使用 WSL，也未声称 Linux 验证。

## Final passing evidence

| Status | Command | Result |
|---|---|---|
| PASS | `$env:PATH='C:\Users\lifei\AppData\Local\codegraph\current;C:\Users\lifei\.cargo\bin;'+$env:PATH; cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check; cargo check --manifest-path src-tauri/Cargo.toml --lib; cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings; git diff --check`（各步 fail-fast） | exit 0；fmt/check/clippy production lib/diff 全通过 |
| PASS | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::provider::registry::tests -- --test-threads=1` | 9 passed，0 failed，1460 filtered |
| PASS | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::provider::port::tests -- --test-threads=1` | 7 passed，0 failed，1462 filtered |
| PASS | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::provider::control::tests -- --test-threads=1` | 1 passed，0 failed，1468 filtered |
| PASS | `cargo test --manifest-path src-tauri/Cargo.toml --lib agent::codebuddy::client::tests -- --test-threads=1` | 37 passed，0 failed，1432 filtered |
| PASS | `cargo test --manifest-path src-tauri/Cargo.toml --lib native_initialize_incompatibility_wires_registry_catalog_and_refresh -- --test-threads=1` | 1 passed，0 failed，1468 filtered |
| PASS | `cargo test --manifest-path src-tauri/Cargo.toml --lib native_initialize_transient_failures_do_not_pollute_registry -- --test-threads=1` | 1 matrix passed，0 failed，1468 filtered；实际覆盖 initialize EOF/timeout/malformed |
| PASS | `cargo test --manifest-path src-tauri/Cargo.toml --lib native_permission_terminal_and_cleanup_matrix -- --test-threads=1` | 1 matrix passed，0 failed，1468 filtered |
| PASS | `$env:PATH='C:\Users\lifei\AppData\Local\codegraph\current;C:\Users\lifei\.cargo\bin;'+$env:PATH; cargo test --manifest-path src-tauri/Cargo.toml --lib agent::product:: -- --test-threads=1` | 138 passed，0 failed，5 ignored，1326 filtered |
| PASS | `& 'C:\Users\lifei\AppData\Local\codegraph\current\node.exe' --test src/AgentPanel.test.mjs` | 86 passed，0 failed；exact ACP cases 与 7 个 near/non-exact negatives 均实际执行 |

## Diagnostic attempts not counted as passing evidence

- Host PATH 中 `cargo` 不可解析：fmt/check 未启动；随后使用现有绝对 cargo 路径修正。
- 首次 Registry focused：8 passed / 1 failed，发现新增 Registry 注释触发既有 provider-private 禁词断言；只改注释后 9/9 通过。
- 首次 permission matrix：失败并真实暴露 permission options 复用 `Failure::Incompatible` 会污染 admission；新增本地 `PermissionOptions` 分类后通过。
- 两次错误 `--exact` filter 各执行 0 个测试，不计入覆盖；随后 corrected filters 实际执行并通过。
- 首次 Product full：74 passed / 64 failed / 5 ignored，统一首根因为 child PATH 找不到 Node（`program not found`）；加入现有 Node 24 路径后 138/138 通过。
- 首次 final Clippy：`execute::run` 触发 `too_many_arguments`；收敛为 `RunControl` 后 `cargo clippy --lib -- -D warnings` 通过。

用户指定的历史 `usage_tests.rs:987 await_holding_lock` 仅属于 all-target Clippy；本卡要求的 production `--lib` 严格 Clippy 已通过，未修改该历史文件，也未把 all-target 声称为通过。
