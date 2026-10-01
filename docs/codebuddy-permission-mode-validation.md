# CodeBuddy 会话权限模式改造与验证

> 当前合同（2026-10-01）：auto 不再是 provider-internal default；Fresh/Continue 默认保留 Provider current/default mode，不因 advertise auto 而发送 set_mode。显式 mode 仍走 typed ACP ACK/replay/confirm。以下 2026-09-30 内容为历史阶段记录，当前 cutover 与验证见 [policy validation](codebuddy-permission-policy-validation.md#permission-authority-cutover2026-10-01)。

日期：2026-09-30。分支：`feat/codebuddy`。未提交 Git，保留开始时 29 项未提交 macOS 适配；没有修改用户配置或安装/升级 CodeBuddy。

## 行为

- auto 是受 advertise 约束的 provider-internal default：Fresh minimal `session/new`，Continue exact source `session/load` 返回后，先 validate/replay 当前 typed catalog，再仅在 `modes` 或 `configOptions category=mode` 实际 advertise auto 时填充本次 DesiredConfiguration.mode。
- 复用 `SessionCatalog::configure` 与 typed `SetSessionModeRequest`；不猜 config option id、不新增 raw RPC、不修改 CLI argv/env。没有 auto 时维持原模式，不因此改变 Provider admission。
- 等待 auto ACK，再 replay/confirm；在 acceptance-ready 边界重验。error/timeout/malformed、current_mode_update 回退、config option 回退/撤销/移除均阻止 acceptance 和 prompt。
- SDK 空响应结构可能宽松接受 `[]`/null；在现有 exact pending ID/method 输入 guard 中要求 set_mode 成功 result 为对象。合法 `{}` ACK 与 JSON-RPC error 路径保留。
- Continue 仍使用 source Session，child Runtime 独立；source provider/session/conversation/request identity 不被改写。无额外 session/new/resume。
- auto 后 unresolved permission 仍走 exact identity + RejectOnce，只选择请求 advertise 的拒绝 ID，不自动 allow_once/allow_always。
- 公共 ExecutionProfile、model/reasoning、用户 role defaults 和 configuration catalog 查询语义不变；默认 auto 不是全局权限绕过。

## 本轮修改文件

| 范围 | 文件 |
| --- | --- |
| Session 默认与确认 | `src-tauri/src/agent/codebuddy/fresh.rs`、`continued.rs` |
| ACK 输入边界 | `src-tauri/src/agent/codebuddy/protocol.rs` |
| typed ACK 回归 | `src-tauri/src/agent/codebuddy/client_tests.rs` |
| 原生 Provider 测试 | `src-tauri/src/agent/codebuddy/execute/tests.rs`（仅模块声明）、新增 `execute/permission_mode_tests.rs` |
| 原生 ACP peer | `src-tauri/tests/fixtures/codebuddy_execute_child.rs` |
| Windows 默认预期 | `src-tauri/src/agent/codebuddy/fresh/tests.rs` |
| catalog 查询语义 | `src-tauri/src/agent/codebuddy/provider/macos_tests.rs`（查询 peer advertise auto，仍禁止 set_mode/prompt） |
| 文档 | `docs/technical-design-multi-agent-provider-codebuddy-v0.1.md` §18、`docs/codebuddy-macos-validation.md`、本文 |
| 本地任务记录 | `.trellis/tasks/09-30-codebuddy-macos/permission-mode.md` |

开始时用户提供的技术设计、fresh.rs、permission.rs、provider.rs 四个 SHA256 全部匹配。结束时 permission.rs 与 provider.rs 的 SHA256 仍与用户提供值一致；本轮未修改这两份文件。

## 验证

| 命令/检查 | 结果 |
| --- | --- |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | PASS |
| `cargo check --manifest-path src-tauri/Cargo.toml` | PASS |
| `cargo test --manifest-path src-tauri/Cargo.toml codebuddy -- --test-threads=1` | 125 PASS、0 FAIL、2 ignored；47.82s（不含编译） |
| `git diff --check` | PASS |
| 独立只读评审 | 无需修复的问题；未修改文件或重复运行 Cargo |

初次新增 permission_mode_tests focused 运行为 3 PASS / 1 FAIL：`result=[]` 被 SDK 当作空响应接受并到达 acceptance。补入上述对象 ACK guard 后，在最终 CodeBuddy 集中该失败用例及所有新增测试均 PASS，未弱化失败断言。

新增 5 个测试：4 个 public Provider 原生 matrix 测试共 28 个 Fresh/Continue 场景，加 1 个内存 typed ACK 测试。覆盖 modes/config-only 任意 option id、ACK gate（ACK 前零 acceptance/零 prompt）、无 auto、10 类故障（error、timeout、array/scalar/null malformed、config revoke/rollback/remove、typed current-mode rollback/malformed）、Continue source identity、auto 后 RejectOnce 和 profile 不污染。内存 ACK 测试进一步覆盖字符串 malformed 与合法 `{}`。

既有 `codebuddy_macos_` Runtime/Recovery 7 项在最终测试集中全部 PASS：

- `codebuddy_macos_managed_handshake_and_cleanup`
- `codebuddy_macos_failed_cleanup_retains_owner`
- `codebuddy_macos_unverified_created_child_is_retained`
- `codebuddy_macos_durable_identity_and_evidence_are_platform_specific`
- `codebuddy_macos_evidence_rejects_changed_original_identity`
- `codebuddy_macos_recovery_preserves_unknown_without_identity`
- `codebuddy_macos_startup_recovers_original_native_process_group`

另有 3 项 macOS discovery、managed configuration catalog 和产品 `codebuddy_catalog_and_refresh_preserve_capability_truth` 均 PASS。

两个默认 ignored 测试本轮明确为 **NOT_RUN**，不计为 PASS：

- `real_codebuddy_managed_configuration_catalog_smoke`
- `real_codebuddy_managed_initialize_smoke`

测试链接出现非阻塞 `__eh_frame section too large` warning；最终构建/测试退出码为 0。

## 真实验收与限制

用户提供的 Host context 已证明 macOS CodeBuddy 2.160.0 无工具模型任务可完成，真实仓库读取任务被固定 RejectOnce 阻塞。该 Host 事实与旧首次 smoke 失败已分时记录在 macOS 验证文档；本轮没有重跑真实模型任务。

本轮测试使用受管原生 ACP peer、真实管道、SQLite 和平台 containment。Windows/macOS 共用实现；Windows 原生测试代码已同步默认预期，但本机没有执行 Windows 真机测试（NOT_RUN）。auto 改造后的真实 CodeBuddy 仓库编码、Continue 模型任务和 Finder/DMG 验收均 NOT_RUN；不把原生 fixture PASS 写成真实模型验收。

若实际 Session 未 advertise auto，旧模式仍可能请求权限并被拒绝；若 auto ACK 后继续请求权限，RejectOnce 仍会阻止该工具。这是冻结合同内的 fail-closed 行为。


## 后续本地 Policy 实现

固定 RejectOnce 已升级为冻结 execution workspace/mode + exact typed tool snapshot 的确定性 AllowOnce/RejectOnce。当时 auto mode 行为保持；2026-10-01 authority cutover 已取消该默认，当前保留 Provider current/default mode。允许决策不生成 CODEBUDDY_PERMISSION_DENIED。原阶段固定拒绝的证据为历史记录，不代表当前实现。该策略不是 OS sandbox，也不保证被允许构建命令不能访问 workspace 外。完整矩阵、验证与重启后 E2E 门槛见 [CodeBuddy Permission Policy validation](codebuddy-permission-policy-validation.md)。
