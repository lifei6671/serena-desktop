# Verification

环境：Windows PowerShell；branch `feat/codebuddy`；baseline/HEAD `5dfe17be0f7b502225d0fe19f52c73b96b1a37ae`。初始工作区 clean；Git 读取全局 ignore 时出现 permission warning，不影响 status 结论。未使用 WSL，未调用真实 Provider，未 commit/push，未进入 CB9-002。

## Authority hashes

以下 SHA-256 在任务开始与最终自检时必须保持一致：

| Authority | SHA-256 |
|---|---|
| `docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md` | `7af023d5e5a84e3106b12db14ac44eb9b4ba6e003cd3fdc2027c2fe29176f334` |
| `docs/technical-design-multi-agent-provider-codebuddy-v0.1.md` | `a80a5a6fa0b3d263b194f7d4604933a805475ca15761eeae86c3c7029e0e6e15` |
| `09-28-cb5-005-codebuddy-continuation-usage-crash-contract/decision.md` | `469819941822853e2b7a0f95fea911ee490110efa2e2e393473835459470a29e` |
| `09-28-cb5-005-codebuddy-continuation-usage-crash-contract/verification.md` | `c7378b10e0640cc26657820e68a06e113040317a64c859b963096d90aa286979` |
| `09-28-cb5-005-codebuddy-continuation-usage-crash-contract/review-final.md` | `7476ed839efe8ec60e2838244822bb9b411f5d10c159df66ef8ca9943b74bd2f` |
| `09-28-cb8-004-codebuddy-crash-result-recovery/task.json` | `9d760173224fec3d7a44de1fcb724a1c142b5f45f528b9dd0c69f516ea83dc6b` |
| `09-28-cb8-004-codebuddy-crash-result-recovery/verification.md` | `adcc62a39733247dd09af2629617bed00869184c08e0fc80ee45510c72b1ba92` |
| `09-28-cb8-004-codebuddy-crash-result-recovery/review-final.md` | `15f2d23eb3650e9a72cad5b9053ba42f418a657df08aed465d439fad52c89bb7` |

## Contract evidence

- PASS — breakdown CB9-001 明确要求 contract + implementation PASS 才 `tokenUsage=true`，否则 `SKIPPED_UNSUPPORTED`；unsupported 时为 `tokenUsage=false + unknown/null`。
- PASS — technical design §20/20.1/33 明确冻结未通过 Usage Gate 时的 false capability、unknown/null 与非 Codex 禁止进入 Codex private Usage path。
- PASS — CB5-005 final decision/review 冻结 attempt 4 的 9 条真实 `usage_update`，同时冻结 `exactPromptBound=false` 及 scope/reset/terminal/late 未证明，公共 Usage 为 `EXPLICITLY_UNSUPPORTED_FOR_INITIAL_RELEASE`。
- PASS — CB8-004 `task.json.status=completed`，dependency 已满足。

## Current code and test evidence

- PASS — `src-tauri/src/agent/codebuddy/provider.rs:151-160` 的 CodeBuddy capabilities 当前明确 `token_usage: false`；`provider/tests.rs` 与 `client_tests.rs` 均断言 false。
- PASS — `src-tauri/src/agent/store/usage.rs:90-92,122-124,162-164,208-210,248-250,314-318,436-438` 对非 Codex execution 的 public writer、private state、baseline、grace、freeze、invalidation 与 projection 入口 fail closed。
- PASS — `src-tauri/src/agent/store/usage.rs:559-608` 在读取 Codex epoch/private state 前先验证 persisted provider 为 `codex`，非 Codex 返回 `None`。
- PASS — `src-tauri/src/agent/store/usage_tests.rs:909-1015` 覆盖非 Codex execution 调用全部 Codex private Usage 入口均返回 `USAGE_PROVIDER_UNSUPPORTED`，且 public/private/epoch 表均不产生行。
- PASS — `src-tauri/src/agent/product/usage_projection_tests.rs:75-130` 与 `provider_projection_tests.rs:166-204` 覆盖非 Codex Provider 不读取 Codex private state、无 public row 时 detail/observe/list 返回 unknown/null，并在 restart 后保持 Provider identity。
- PASS — 使用 `C:\Users\lifei\.cargo\bin\cargo.exe` 定向执行上述现有契约测试，4 次各 `1 passed; 0 failed`：
  - `found_cli_registers_available_platform_capabilities`
  - `non_codex_execution_rejects_every_codex_private_usage_entry_without_rows`
  - `fake_provider_public_usage_is_optional_and_never_reads_codex_private_state`
  - `fake_provider_create_and_restart_preserve_product_identity_and_unknown_usage`
- INFO — 直接调用 `cargo` 时 PATH 中不可用；改用已存在的绝对路径后测试通过。四次测试仅有既有 MSVC linker stdout warning，不影响 exit 0。

## Diff and validation

- PASS — production diff=0：相对 baseline/HEAD，`src/`、`src-tauri/` 无 tracked 或 untracked 变化。
- PASS — schema/migration diff=0：无 schema、migration 或数据库契约文件变化。
- PASS — frontend diff=0；formal authority docs diff=0；唯一任务变更位于本 task-local 目录。
- PASS — `git diff --check`。
- PASS — authority SHA-256 最终复核与任务开始值一致。
- Review not required — 本任务只有 prose/task evidence，没有 implementation、test、schema、migration、API、configuration 或其他 executable change；已完成主 Agent 自检，按要求未启动独立 code reviewer。

最终结果：`SKIPPED_UNSUPPORTED`。下一卡 CB9-002 `UNBLOCKED`，但本任务未进入。
