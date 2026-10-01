# Verification

环境：Windows PowerShell；branch `feat/codebuddy`；baseline/HEAD `c631a6707d2da0d3e0c8dbf440701e30e5739461`。起始工作区 clean。未使用 WSL，未调用真实 Provider，未 commit/push，未进入 Phase10。

## Authority hashes

| Authority | SHA-256 |
|---|---|
| `docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md` | `7af023d5e5a84e3106b12db14ac44eb9b4ba6e003cd3fdc2027c2fe29176f334` |
| `docs/technical-design-multi-agent-provider-codebuddy-v0.1.md` | `a80a5a6fa0b3d263b194f7d4604933a805475ca15761eeae86c3c7029e0e6e15` |
| `.trellis/tasks/09-28-cb9-001-codebuddy-usage-skip/decision.md` | `5357a726eea8b5bdfb0fc9fc622e51da4c624f16cded464f98d221020e83038e` |

## PASS

- Rust Usage Product targeted：`cargo test --manifest-path Cargo.toml --lib 'agent::product::tests::usage_projection_tests::' -- --nocapture` — 7 passed；实际执行非零。
- Rust Store Usage targeted：`cargo test --manifest-path Cargo.toml --lib 'agent::store::usage_tests::' -- --nocapture` — 13 passed。
- Rust non-Codex private rejection：精确 filter — 1 passed。
- Rust historical Provider restart：精确 filter — 1 passed。
- Rust CodeBuddy provider catalog：精确 filter — 1 passed；Windows `canExecute/canContinue/canCancel/canRecover/activity=true`，`tokenUsage=false`，非 Windows 由 `cfg!(windows)` 收敛。
- Product full module：`cargo test --manifest-path Cargo.toml --lib 'agent::product::' -- --nocapture` — 136 passed，5 个明确 isolated real-provider smoke ignored，0 failed。
- Frontend CB9-002 targeted：`node.exe --test --test-name-pattern 'CB9-002' src/AgentPanel.test.mjs` — 1 passed。
- Frontend AgentPanel full file：`node.exe --test src/AgentPanel.test.mjs` — 86 passed，0 failed/ignored。
- `cargo check --manifest-path Cargo.toml` — PASS。
- `cargo fmt --manifest-path Cargo.toml -- --check` — PASS。
- `cargo clippy --manifest-path Cargo.toml --lib -- -D warnings` — PASS。
- `git diff --check` — PASS。
- authority SHA-256 复核 — PASS。
- production/Runtime/protocol/recovery/writer/schema/migration diff=0 — PASS。

Cargo 需要启动 Node 的 Product contract test 时，仅为该子进程把已存在的 `C:\nvm4w\nodejs` 前置到 PATH；没有安装依赖或修改系统环境。

## Non-passing attempts that do not count as evidence

- 两次 Rust filter 因缺少完整模块路径各执行 0 tests；均明确不计 PASS，随后用正确 filter 得到 1/1 与 7/7。
- Usage Product 全组首次运行 6/7，唯一失败是 Cargo 子进程 PATH 找不到 `node`；注入已存在 Node 目录后同组 7/7。
- frontend 新测试最初两次失败均为测试选择器问题：跨字段正则匹配到 1970 时间、随后选错 grid；改成直接定位 Token `<code>` 后 targeted 1/1、整文件 86/86。production 未修改。
- 独立 review round 1 的 P3 指出 running Cancel 只存在于 fixture、未被本专项测试观察；测试随后真实打开 running detail 并断言“取消任务”，删除仅回读本地 capability fixture 的无价值断言，targeted 1/1、整文件 86/86 再次通过。

## Known existing/environment notes

- Git 读取 `C:\Users\lifei\.config\git\ignore` 出现 permission warning；status/diff 命令仍 exit 0 且结果完整。
- Trellis Python runtime 不可用：`python` 未找到，`py -3` 返回 `No installed Python found`；因此按既有格式用 `apply_patch` 建立任务，未运行 `task.py create/start`。
- 历史上 `cargo clippy --all-targets -- -D warnings` 会命中未改动的 `src-tauri/src/agent/store/usage_tests.rs:987` `await_holding_lock`；本卡要求并执行的 production `--lib` 严格 Clippy 已通过，未越界修改该历史测试。
