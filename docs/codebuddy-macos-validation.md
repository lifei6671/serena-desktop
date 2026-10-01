# CodeBuddy macOS Apple Silicon 接入与验证

日期：2026-09-30。分支：`feat/codebuddy`。开始时工作区干净；未执行 Git 提交。
用户提供的 6 个版本化源码 SHA256 均与开始实施时源码一致。

## 实现

- Discovery 无进程、无 shell；检查 process PATH、`$HOME/.local/bin`、`/opt/homebrew/bin`、`/usr/local/bin`。共享 Codex regular file / execute bit / ARM64 Mach-O preflight，canonicalize executable，独立 argv=`[--acp]`，过滤空/相对 PATH。
- Registry/catalog 自动展示 CodeBuddy；安装 CLI 时 admission available；默认 enabled=false 未变；UI 无 providerId 特判。
- Fresh / Continue / Activity / Cancel / result recovery 共用原 ACP pipeline，仅平台 launcher、Runtime 和持久化证据不同。configuration catalog 使用同一受管 Runtime，不创建 Execution / Claim / runtime_instances。
- 复用 Codex setsid、Darwin start token 和 group-empty shutdown。CodeBuddy Runtime 使用 `provider=codebuddy`、`runtime_platform=macos`、`containment_type=macos_process_group`、`process_identity_scheme=darwin_proc_bsd_start_v1`；PID=PGID=SID，Windows Job 字段为空。
- sealed live evidence 必须匹配同一 Runtime ID / PID / PGID / SID / start token，Store 事务再次检查原始快照。R1、R2 各自证明，terminal / Claim release 仍走 generic authority。
- 无法证明清理完成的 Runtime / CreatedChild 保留 ownership 并隔离对应 Workspace，不无限重试或生成成功证据。跨 Host recovery 在已验证 SIGTERM 后若 leader 消失，只在有界 grace 内观察 group-empty，不再发信号。
- Windows discovery fixture 和 Job-at-creation 路径保留。Windows 专有测试仍限定 Windows；原生 Execute fixture 扩展到 macOS。

## 修改文件

| 范围 | 文件（相对 src-tauri/） |
| --- | --- |
| Discovery | `src/agent/codebuddy/discovery.rs`、`discovery/tests.rs`、新 `macos_discovery.rs` |
| 平台 Runtime | 新 `src/agent/codebuddy/macos_launcher.rs`、`macos_runtime.rs`、`macos_recovery.rs` |
| 共用接线 | `src/agent/codebuddy/mod.rs`、`provider.rs`、`fresh.rs`、`prompt.rs`、`recovery.rs`、`result_recovery.rs`、`store.rs` |
| Store | `src/agent/store/codebuddy.rs`、`codebuddy_runtime.rs` |
| 复用 Codex | `src/agent/codex/macos_launcher.rs`、`macos_runtime.rs`、`macos_recovery.rs`、`macos_runtime/tests.rs` |
| 验证 | 新 `src/agent/codebuddy/macos_runtime_tests.rs`、`provider/macos_tests.rs`；`client_tests.rs`、`provider/tests.rs`、`execute/tests.rs`、`src/agent/product/provider_catalog_tests.rs`、`src/agent/task_manager/tests.rs`、`tests/fixtures/codebuddy_execute_child.rs` |

## 验证结果

- 修改的 27 个 Rust 文件：`rustfmt --edition 2024 --config skip_children=true --check <files>` PASS。
- `cargo check --manifest-path src-tauri/Cargo.toml` PASS。
- `cargo test --manifest-path src-tauri/Cargo.toml codebuddy -- --test-threads=1`：120 PASS、0 FAIL、2 ignored。2 项 ignored 已另行显式运行，结果见下方；不计入这 120 项。
- `cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos -- --test-threads=1`：45 PASS、0 FAIL、0 ignored。
- `npm test`：176 PASS，0 skipped。
- `npm run lint`：PASS。
- 显式真实 CodeBuddy smoke：受管 initialize + shutdown PASS；受管 configuration catalog FAIL，CLI 对 session/new 返回 JSON-RPC `-32603 Internal error`，Provider 映射 `CODEBUDDY_ACP_REQUEST_FAILED` / `AgentProviderOperationFailed`。未打印完整 PATH、env、token 或 RPC payload。根因尚未确定，不归因为未证实的权限或认证问题。

复跑真实 smoke（默认测试忽略这些外部依赖测试）：

```sh
cargo test --manifest-path src-tauri/Cargo.toml real_codebuddy_ -- --ignored --nocapture --test-threads=1
```

## 验收边界

默认测试使用本机编译的原生 ACP peer、真实 process group、真实管道和 SQLite，不依赖已安装 CodeBuddy。它们验证行为与 containment，不替代真实模型服务验收。

上述首次 smoke 中真实 CLI 的 session/new 未通过，因此当时模型 Fresh / Continue / Activity / Cancel 全链路 E2E 未通过验收。后续 Host 已补充成功证据，见下节；不把首次 smoke 失败继续作为当前阻塞。未进行 Finder/DMG、真实 Host crash/relaunch 或 Windows 真机回归。没有安装/升级 CodeBuddy，没有修改认证配置。

macOS 若原 leader 已消失且 group-empty 证据未能持久化，启动恢复保持 unknown 和 Claim；不能套用 Windows 已销毁 Job 的成功推论。

## 后续 Host 证据与权限模式改造

2026-09-30 用户提供 Host verified context：CodeBuddy macOS 2.160.0 可被发现，catalog enabled=true、health=available，执行/继续/取消/恢复/activity 能力可用；显式 providerId=codebuddy、taskRole=review、hy4-preview-f 的无工具任务返回 `CODEBUDDY_MAC_OK`，Execution completed + complete。真实读取仓库任务进入 running 后遇到 `session/request_permission`，现有 RejectOnce 导致 `CODEBUDDY_PERMISSION_DENIED`，用户取消后 interrupted 安全收敛。这些是用户提供的真实 Host 证据，本轮未重新执行模型任务。

2026-09-30 的权限模式阶段曾采用 provider-internal auto 默认。2026-10-01 Permission Authority Cutover 已取消该默认：Fresh new / Continue exact source load 保留 Provider current/default mode，不因 advertise auto 而 set_mode，避免引入 CodeBuddy 自身 classifier 的重复审批 authority。显式 mode/option 仍受 typed ACP ACK/replay/confirm 约束；request_permission 继续由本地 deterministic Policy 选择 advertised AllowOnce/RejectOnce。公共 ExecutionProfile、model/reasoning、Provider health/catalog、用户配置和 CLI 参数不变。当前验证及 build/restart 后的真实 E2E 门槛见 [policy validation](codebuddy-permission-policy-validation.md#permission-authority-cutover2026-10-01)。

本轮变更与验证结果见 [CodeBuddy 权限模式验证](codebuddy-permission-mode-validation.md)。
