# Phase 3A：macOS ARM64 Codex 发现与 Provider 激活

## Goal

在不改变既有 Windows Runtime / Job Object 安全契约的前提下，将 Phase 2A/2B 已完成的 `MacosRuntime` 与 `macos_recovery` 接入现有 `CodexProvider`，使 SerenaDesktop 在 Apple Silicon macOS 上能够发现经过精确兼容性验证的 Codex ARM64 Mach-O，并复用现有 execute、continue、cancel、finalization 与 startup recovery orchestration。

## Supported Platform

首版正式支持范围固定为：

```text
OS   = macOS
Arch = arm64 / aarch64
```

明确不支持：

- Intel x86_64 Mac；
- 在 Apple Silicon 上通过 Rosetta 运行的 x86_64 SerenaDesktop 或 Codex；
- 纯 x86_64 Codex Mach-O；
- Linux 或其他 Unix 平台。

当前进程不是 macOS ARM64，或者候选 Codex 不包含 ARM64 slice 时，必须返回稳定的不兼容错误；不得寻找或回退到 x86_64 binary。

## Requirements

### Provider 与平台边界

- 继续使用唯一的 `CodexProvider`，不得新增 `MacosCodexProvider`。
- Windows 与 macOS 继续共用 `provider.rs` 中的 execute、continue、cancel、protocol、finalization 和 startup recovery orchestration。
- 不复制一套 macOS Provider 或 Runtime Pool。
- 不新增公共 `Runtime trait`，平台差异必须集中在 crate-private adapter/module boundary。
- 不在共享 `provider.rs` 中散布平台条件编译。
- Windows `runtime.rs`、`windows_launcher.rs` 以及 Job Object 安全契约保持不变。
- Linux 等未支持平台继续使用 unavailable provider。

### Candidate Discovery 与 Compatibility Selection

- 候选来源按以下固定顺序枚举：
  1. 当前 `PATH` 中的原生 Codex；
  2. `~/.local/bin`；
  3. 已冻结的 Homebrew / npm 确定位置；
  4. npm package 内 `aarch64-apple-darwin` vendor binary；
  5. `/Applications/ChatGPT.app/Contents/Resources/codex`，仅作为最后的 best-effort candidate。
- Finder 或登录项的精简 `PATH` 只能提供候选，不能成为 discovery Authority。
- 候选必须先 canonicalize，再按 canonical path 去重，然后依序验证。
- 较早候选不兼容时必须继续验证后续候选；只有第一个完整兼容的候选可以被选择。
- ChatGPT.app 内部路径不是公共契约或强依赖；不存在、布局改变或验证失败都只淘汰该候选。
- 诊断必须稳定、可区分且不泄露完整 `PATH` 或其他无关环境内容。
- 存在可运行 ARM64 Mach-O、但所有候选均未通过精确 compatibility allowlist 时，最终诊断必须明确标记 `COMPATIBILITY BLOCKED`；完全没有可运行候选时才使用一般 backend unavailable 诊断。

### Executable 与 ARM64 Mach-O Contract

最终 executable 必须是通过 absolute path + `argv[]` 直接启动的真实 Mach-O，禁止经过 shell。

以下内容不得成为最终 executable：

- npm JavaScript shim；
- shell script；
- shell wrapper；
- 依赖 `/bin/sh`、`zsh` 或 `bash` 间接启动 Codex 的包装器。

每个候选至少依次验证：

1. path 存在；
2. regular file；
3. Unix execute bit；
4. canonical path 成功；
5. Mach-O 格式；
6. 包含 ARM64 slice；
7. 拒绝纯 x86_64 Mach-O；
8. 固定 Codex version；
9. binary SHA-256；
10. App Server protocol schema SHA-256；
11. 真实 App Server Compatibility Contract。

Universal Mach-O 只有在包含 ARM64 slice 并通过全部后续验证时才可接受；同时包含 x86_64 不产生 Intel 支持语义。

### Compatibility Allowlist

- compatibility entry 至少包含 `codexVersion`、`os`、`arch`、`binarySha256` 与 `protocolSchemaSha256`。
- Windows 既有 x86_64 entry 的 version/hash/schema 行为保持原样。
- macOS 首版只允许 `macos / arm64` entry，不新增 `macos / x86_64`。
- 相同 version 的不同 binary hash 不自动兼容；每个允许的 binary hash 都必须独立通过完整 Contract Test。
- npm vendor binary 与 ChatGPT.app bundled binary 如 hash 不同，只有分别通过完整 Contract Test 后才能分别加入精确 allowlist。

### Runtime 与取消语义

- 新增 macOS 私有 managed adapter，并继续满足现有 `ManagedClient` 内部接口。
- Compatibility Probe 创建的任何真实 `MacosRuntime` 必须使用正式 `StateStore`、existing host owner 和 existing `CodexRuntimePool`，进入正式 Runtime ownership domain。
- Probe 可以使用 crate-private `ProbeContext` 传递正式 ownership authority；termination failure 必须进入 existing global quarantine，并由 existing `macos_recovery` 在重启后继续收口。
- Probe 禁止使用 temporary StateStore、detached runtime、fake Execution、fake Workspace Claim、新 Pool 或新 recovery state machine。
- 真正阻塞的 process create、setsid/identity、signal、bounded wait、termination confirmation 与同步 cleanup 可以进入 blocking worker。
- stdin writer、stdout JSONL reader、stderr reader、App Server RPC、notification dispatch 以及 Thread/Turn lifecycle 必须继续运行在 Tokio async 路径。
- 不得让 Tokio async mutex guard 跨越 blocking worker 的长时间等待。
- 用户取消 Execution 必须继续使用 Codex `turn/interrupt`。
- Runtime teardown/failure 才能使用 `SIGTERM process group -> bounded grace -> SIGKILL`。
- `turn/interrupt` 成功不能作为 Runtime process group 已安全终止的证据。

### Ownership、Evidence 与 Recovery

- 继续复用 Phase 2A/2B 已冻结的 macOS Runtime ownership、process identity、termination evidence 和 fail-closed 契约。
- evidence 不完整时必须保留 ownership，并将 Execution/Runtime 维持在 quarantine/unknown，Workspace Claim 保留。
- 主 PID 消失、stdio EOF、App Server 断开、等待超时或新 Runtime 启动成功均不得替代完整 termination evidence。
- Startup Recovery 直接复用现有 `macos_recovery`。
- 本阶段不重新设计 StateStore、Execution 11-state graph、Dispatch State、Workspace Claim、Atomic Claim Release、Cross-Runtime Recovery 或 unknown 语义。
- Compatibility mismatch 只有在对应 Probe Runtime 已取得完整 termination evidence 后才能继续下一个 candidate；cleanup/evidence failure 必须停止 selection、保留 ownership 并进入 quarantine。

### Windows Regression Boundary

- 本阶段不要求重新执行完整 Windows 实机 Job Object / Host Crash Gate。
- 必须执行共享 Provider Contract Regression，覆盖 fixture managed runtime 下的 execute、continue、cancel、provider terminal、finalization、cleanup orchestration 与 startup recovery dispatch。
- 如果实现需要改变 Windows Runtime 行为或公共 Runtime Safety Contract，必须停止实施并提交新的 Design Change。

## Acceptance Criteria

### Discovery / Reject

- [x] Finder 风格精简 `PATH` 下仍能从固定位置选择兼容 ARM64 Codex。
- [x] PATH 中兼容候选可被选择；PATH 中错误版本不会阻止后续兼容候选。
- [x] `~/.local/bin`、Homebrew、npm ARM64 vendor 和 ChatGPT.app 最后候选均有确定顺序测试。
- [x] symlink、空格、中文和外置卷形式路径均按 canonical path 正确处理。
- [x] 无 execute bit、shell wrapper、JavaScript shim、纯 x86_64 Mach-O、错误 version、binary hash、schema hash 和 Contract 均被拒绝。
- [x] Universal Mach-O 仅在包含 ARM64 slice 且完整兼容时被接受。

### Runtime / Provider

- [x] macOS 使用现有 `CodexProvider` 对外报告 execute、continue、cancel 与 recover 能力。
- [x] execute、continue、cancel 和 stdio JSONL fixture 路径通过。
- [x] 业务取消只调用 `turn/interrupt`，Runtime shutdown 独立完成 process-group 收口。
- [x] graceful shutdown 与 SIGTERM grace timeout 后 SIGKILL 均形成正确 evidence。
- [x] evidence 不完整、Runtime crash 与 reconcile 保持 unknown/quarantine/Claim retained。
- [x] startup recovery orchestration 委托既有 `macos_recovery`。
- [x] 所有真实 Probe Runtime 均写入正式 Store；cleanup failure 进入 existing Pool quarantine，不存在 temporary Store 或 detached owner。
- [x] 共享 Provider Contract Regression 全部通过，Windows 共用业务语义不变。

### Real Smoke

- [x] 在 Apple Silicon Mac 上对真实安装的 Codex 执行 architecture/version/hash/schema/Contract smoke。
- [x] 不在 allowlist 时明确报告 `COMPATIBILITY BLOCKED`，不临时放宽 allowlist。
- [x] 只有 allowlisted macOS ARM64 Codex 才能进入真实 App Server smoke。

## Explicit Non-Goals

- Intel macOS；
- Rosetta compatibility；
- Linux；
- Git、uv、Serena、cloudflared；
- macOS signing 或 notarization；
- 通用 Runtime trait；
- 通用 Runtime Evidence Schema；
- 第二个 Agent Provider；
- Provider Runtime framework 重构。
