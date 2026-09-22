# Phase 3A Design：macOS ARM64 Codex Provider Integration

## Status

本设计落实已确认的方案 A：**macOS 私有适配层 + 复用现有 Codex Provider 业务逻辑**。本文是实施前的设计权威；任务在用户审核 `implement.md` 并批准前保持 `planning`。

## Current State

- `codex/mod.rs` 当前仅在 Windows 编译真实 `provider.rs`、`pool.rs`、`runtime.rs` 与 `discovery.rs`；非 Windows 使用 unavailable provider/pool/discovery。
- Windows `app_server/managed.rs` 同时承担 compatibility probe、Windows Runtime 创建、stdio 接入与 reconciliation ownership。
- Phase 2A/2B 已提供独立的 `macos_launcher.rs`、`macos_runtime.rs`、`macos_runtime_store.rs` 与 `macos_recovery.rs`，但尚未接入真实 Provider 产品路径。
- 共享 `provider.rs`、`pool.rs`、`task_manager/recovery.rs` 与 `app_server/recovery.rs` 目前通过 Windows `runtime` 类型和 Windows evidence 名称连接。
- 当前协议只用单组全局 `VERSION/BINARY_SHA256/SCHEMA_SHA256`，无法表达相同 Codex version 的平台/架构不同 binary hash。

## Architecture Summary

```text
ProviderRegistry
    ↓
CodexProvider
    ↓
共享 Provider 业务逻辑
    ├── execute
    ├── continue
    ├── cancel
    ├── protocol / App Server
    ├── finalization
    └── startup recovery orchestration
            ↓
      crate-private platform boundary
            ├── Windows adapter
            │     ├── existing app_server/managed.rs
            │     ├── existing runtime.rs
            │     └── existing Job Object evidence
            └── macOS ARM64 adapter
                  ├── app_server/macos_managed.rs
                  ├── macos_runtime_adapter.rs
                  ├── existing MacosRuntime
                  └── existing macos_recovery
```

`CodexProvider` 与 `CodexRuntimePool` 保持唯一实现。平台选择只发生在 `codex/mod.rs` 声明的私有 module alias 和私有 adapter 内；共享业务文件不自行决定平台。

## Platform Boundary

### Module selection

`codex/mod.rs` 负责集中选择三个私有边界：

```text
discovery
managed_adapter
runtime_adapter
```

- Windows：继续指向现有 `discovery.rs`、`app_server/managed.rs` 和由新私有 Windows adapter 转发的现有 `runtime.rs`。
- macOS：指向 `macos_discovery.rs`、`app_server/macos_managed.rs` 和 `macos_runtime_adapter.rs`。
- 其他平台：继续指向 unavailable provider/pool/discovery，不参与真实 Codex 产品路径。

`provider.rs`、`pool.rs`、`task_manager/recovery.rs` 和 `app_server/recovery.rs` 只依赖上述私有边界提供的具体类型与函数，不出现平台分支。

### Runtime adapter surface

不定义 trait。Windows 与 macOS adapter 分别提供同名的 crate-private concrete surface：

```rust
Runtime
RuntimeError
RuntimeFailure
is_complete_termination(&RuntimeRecord) -> bool
recover(store, owner, runtime_id, timeout) -> Future<Result<(), RuntimeFailure>>
```

Windows adapter 只转发现有 Windows 类型、`runtime::recover` 与既有 evidence predicate；不修改 `runtime.rs` 或 `windows_launcher.rs`。

macOS adapter：

- 以 concrete wrapper 持有 `MacosRuntime`，把 create/stdio/initialized/shutdown 映射为共享 managed/pool 需要的形状；
- 使用 `spawn_blocking` 包裹同步 `MacosRuntime::create` 与 `MacosRuntime::shutdown`；
- 失败时把仍存在的 `MacosRuntime` ownership 保存在 `RuntimeFailure.runtime`；
- 无 live owner 的恢复调用委托 `macos_recovery` 的单 Runtime 入口，继续使用既有身份与 group-empty evidence 规则；
- `is_complete_termination` 只接受 `macos_live_process_group_empty` 或 `macos_recovered_process_group_empty` 且 evidence state/time 完整。

为让 pool 的无-owner 重试在两个平台都使用正确 host identity，shared pool 的 retained failure 记录显式保存 `owner` 并把它传给 adapter；Windows adapter 忽略新增参数，Windows 行为不变。

## ARM64 Discovery / Compatibility Contract

### Candidate discovery

`macos_discovery.rs` 将发现拆成两个阶段：

```text
enumerate_candidates(context)
    ↓
canonicalize + deduplicate
    ↓
select_compatible(candidates, verifier)
```

固定来源顺序：

1. 按 `PATH` 条目顺序查找 `codex`；
2. `$HOME/.local/bin/codex`；
3. `/opt/homebrew/bin/codex`、`/usr/local/bin/codex`，以及 `/opt/homebrew/lib/node_modules`、`/usr/local/lib/node_modules`、`$HOME/.local/lib/node_modules`、`$HOME/.npm-global/lib/node_modules` 和 PATH 中 `<prefix>/bin` 对应的 `<prefix>/lib/node_modules`；
4. 上述 npm root 内 `@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex` 与 `@openai/codex/vendor/aarch64-apple-darwin/bin/codex`；
5. `/Applications/ChatGPT.app/Contents/Resources/codex`。

候选枚举只收集路径，不做“找到即成功”的决定。canonical path 使用 insertion-order 去重；较早候选失败后将稳定失败类别记录到 bounded diagnostic summary，并继续后续候选。

Finder 精简 PATH 测试不读取 shell dotfiles、不执行 login shell，也不调用 `which`、`npm`、`node` 或 shell shim。

### File and Mach-O validation

在任何 version/schema probe 前执行本地结构验证：

1. `symlink_metadata`/`metadata` 确认目标存在且是 regular file；
2. `PermissionsExt::mode() & 0o111 != 0`；
3. canonical path 必须是 absolute path；
4. 读取 bounded Mach-O header；
5. thin Mach-O 的 CPU type 必须是 `CPU_TYPE_ARM64`；
6. fat/universal Mach-O 至少一个 slice 必须是 `CPU_TYPE_ARM64`；
7. 纯 `CPU_TYPE_X86_64` 返回稳定 `CODEX_ARCH_UNSUPPORTED`；
8. script/shebang/JavaScript 文本因不是 Mach-O 返回稳定 `CODEX_EXECUTABLE_FORMAT_UNSUPPORTED`。

当前 SerenaDesktop 进程编译目标不是 `target_os=macos,target_arch=aarch64` 时，discovery 在枚举前返回 `CODEX_HOST_ARCH_UNSUPPORTED`。没有 Rosetta fallback。

### Compatibility allowlist

新增 crate-private `compatibility.rs`，把 allowlist 表达为：

```rust
CompatibilityEntry {
    codex_version,
    os,
    arch,
    binary_sha256,
    protocol_schema_sha256,
}
```

首版 entries：

| Codex version | OS | Arch | Binary SHA-256 | Schema SHA-256 |
|---|---|---|---|---|
| `codex-cli 0.153.4` | Windows | x86_64 | `444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B` | `B06F77062369D481A59CC70720C12B89CB9DD49C385863923262102D3AD6C978` |
| `codex-cli 0.153.4` | macOS | arm64 | `B973D440ACAC501FD2594A43E7CA9CE41E0A65B9DFB28D0D7A7837C99E1261E3` | `B06F77062369D481A59CC70720C12B89CB9DD49C385863923262102D3AD6C978` |

macOS hash 来源是官方 npm artifact `@openai/codex@0.153.4-darwin-arm64`，registry integrity 为 `sha512-B1qhN3fa1ay0R0wGziXqgwSkB5icpYChNKHhtBHff/0UtSTC7z+l8aTtvMlGjH3E8HEvY3+njIJelM9CAAoVWg==`。该 artifact 在本机只读研究中确认：

- Mach-O 64-bit arm64；
- `codex-cli 0.153.4`；
- binary SHA-256 与上表一致；
- fresh exported App Server schema SHA-256 与冻结 schema 一致。

最终 Contract Test 仍在实施阶段执行，只有 version、target、binary hash、schema hash 和真实 App Server Contract 全部匹配时才返回 compatible。

allowlist 不包含 macOS x86_64。Universal Mach-O 根据 ARM64 slice 进入后续验证，但其完整文件 hash 必须有独立精确 entry；不能借用 thin ARM64 hash。

### ChatGPT.app boundary

ChatGPT.app 路径只在候选列表最后出现，不存在或不兼容均不影响其他候选。本机当前 bundled Codex 观测为：

```text
Mach-O 64-bit executable arm64
codex-cli 0.155.0-alpha.9.2
SHA-256 9280C0754E8F1F6B72F495D30C8C82A006DBC4995BF0492916FA0901F6BFD1F9
```

它不匹配首版 allowlist，因此当前 real smoke 的预期结果是 `COMPATIBILITY BLOCKED`。本设计不为其增加临时 entry，也不降低 version/hash/schema/Contract 要求。

## Managed Runtime Data Flow

```text
AgentProductService bootstrap
    ├── create AgentTaskManager shell
    ├── obtain formal StateStore + host owner + CodexRuntimePool
    └── ProbeContext
            ↓
macos discovery compatibility selection
            ↓
macos managed compatibility probe
    ├── every real MacosRuntime uses formal ownership domain
    ├── compatibility mismatch + complete cleanup → continue candidate
    └── cleanup/evidence failure → retain in global quarantine and stop selection

selected executable
    ↓
CodexProvider::connect
    ↓
macos managed_adapter::connect
    ├── verify target + Mach-O + allowlist + schema + Contract
    ├── reserve Runtime attempt
    ├── spawn_blocking(MacosRuntime::create)
    ├── take stdio ownership
    ├── Client::transport (Tokio async)
    ├── client.initialize / notifications / JSONL (Tokio async)
    ├── persist initialized compatibility identity
    └── reconciliation monitor
            ↓
        spawn_blocking(MacosRuntime::shutdown)
```

blocking worker 只拥有同步 OS lifecycle 工作。stdio 和 RPC 不进入 blocking worker；调用 blocking worker 前先释放 pool/client 的 async mutex guard，并通过 ownership move 传递 Runtime，避免 guard 跨越 bounded wait。

macOS managed adapter 保持与 Windows `ManagedClient` 相同的内部语义：Client drop 触发独立 reconciliation；初始化失败、stdio 接管失败或 monitor cancel 都不能丢失 Runtime ownership。

## Probe Ownership Amendment

Compatibility Probe 不是 detached diagnostic process。任何由 version、schema 或真实 App Server Contract probe 创建的 `MacosRuntime` 都进入与业务 Runtime 相同的正式 ownership domain。

crate-private `ProbeContext` 只封装已有 authority：

```rust
ProbeContext {
    store: StateStore,
    owner: String,
    runtime_pool: Arc<CodexRuntimePool>,
}
```

它不成为公开 Runtime contract，也不创建新的 Pool 或 recovery 状态机。

启动顺序调整为：

1. `AgentProductService` 先用正式 Store 创建 `AgentTaskManager` shell；
2. Manager 暴露 crate-private probe context，不暴露进程细节；
3. macOS discovery 使用该 context 做完整 compatibility selection；
4. 成功后把 executable/backend diagnostic 写回同一个 Manager；
5. startup recovery 与 Provider publication 继续使用同一个 owner、Pool 和 Store。

Probe failure 分为两类：

```text
Compatibility failure
    + complete Runtime termination evidence
    → candidate rejected; continue searching

Runtime cleanup/evidence failure
    → RuntimeFailure ownership retained by existing Pool under global quarantine key
    → stop candidate selection
    → Provider remains unavailable/quarantined
    → restart uses existing macos_recovery
```

Probe 不创建 Execution 或 Workspace Claim；因此不会伪造业务状态。Runtime Store 行是正式 orphan Runtime 记录，完整终止后可保留为既有历史证据；不完整时由原有 orphan recovery 解释。

明确禁止 temporary StateStore、detached probe Runtime、static/global owner leak、fake Execution、fake Claim、新 Runtime trait、新 Pool、新 recovery state machine 和新 StateStore schema。

## Cancellation and Termination

```text
Execution cancel
    → existing Codex turn/interrupt
    → provider terminal/finalization

Runtime teardown or failure
    → MacosRuntime shutdown
    → SIGTERM process group
    → bounded grace
    → SIGKILL when required
    → direct child reaped + group empty evidence
```

两条路径不互相替代。`turn/interrupt` 成功后 Runtime 仍由 reconciliation monitor 完成 shutdown；stdio EOF 或协议断开只触发 reconcile，不生成 termination evidence。

## Startup Recovery and Evidence

- Provider startup orchestration 在 macOS adapter 中调用既有 `macos_recovery`，并将既有 `ProviderReconcileSummary` 投影到共享 Provider 结果。
- live owner retry 与跨 Host recovery 只通过 adapter 调用既有 macOS evidence contract。
- `app_server/recovery.rs` 的跨 Runtime 安全判断改为调用 `runtime_adapter::is_complete_termination`；Windows adapter 返回原有 Job evidence 判断，macOS adapter 返回既有两种 group-empty evidence 判断。
- Phase 3A 不改变 StateStore schema、Claim transaction、Execution transition 或 macOS recovery 判定规则。

## Error Contract

新增或复用以下稳定类别：

- `CODEX_HOST_ARCH_UNSUPPORTED`：当前 SerenaDesktop 不是 macOS ARM64；
- `CODEX_ARCH_UNSUPPORTED`：候选没有 ARM64 slice；
- `CODEX_EXECUTABLE_FORMAT_UNSUPPORTED`：候选不是 Mach-O，覆盖 script/shim；
- `CODEX_EXECUTABLE_NOT_RUNNABLE`：不是 regular file、无 execute bit或 canonicalize 失败；
- `CODEX_APP_SERVER_INCOMPATIBLE`：单个候选的 version/hash/schema/Contract 不在精确 allowlist；
- `CODEX_COMPATIBILITY_BLOCKED`：至少存在可运行 ARM64 Mach-O，但全部候选均未通过完整 compatibility；面向用户明确显示 `COMPATIBILITY BLOCKED`；
- `BACKEND_UNAVAILABLE`：没有可运行候选，消息只包含 bounded 类别摘要；
- 既有 macOS Runtime create/store/signal/evidence 错误码保持不变。

## Planned File Map

### Create

- `src-tauri/src/agent/codex/compatibility.rs`：平台/架构精确 allowlist 与匹配。
- `src-tauri/src/agent/codex/macos_discovery.rs`：候选枚举、canonicalize/deduplicate、ARM64 Mach-O preflight 与 compatibility selection。
- `src-tauri/src/agent/codex/platform_windows.rs`：私有 Windows adapter，纯转发现有 Runtime/evidence。
- `src-tauri/src/agent/codex/macos_runtime_adapter.rs`：私有 macOS product adapter，连接 `MacosRuntime` 与 shared pool/managed surface。
- `src-tauri/src/agent/codex/app_server/macos_managed.rs`：macOS compatibility probe、stdio transport 与 reconciliation ownership。
- `src-tauri/src/agent/codex/macos_discovery/tests.rs`：discovery/reject 矩阵。
- `src-tauri/src/agent/codex/app_server/macos_managed/tests.rs`：managed adapter 生命周期与真实 Contract fixture。
- `src-tauri/src/agent/codex/macos_runtime_adapter/tests.rs`：blocking ownership、evidence 与 recovery adapter 测试。

### Modify

- `src-tauri/src/agent/codex/mod.rs`：集中平台 module alias；macOS 选择真实 provider/pool/discovery。
- `src-tauri/src/agent/codex/protocol.rs`：CompatibilityIdentity 改为按 target 检查结构化 allowlist。
- `src-tauri/src/agent/codex/app_server.rs`：暴露私有 managed adapter alias 所需模块，不改变协议 Client。
- `src-tauri/src/agent/codex/app_server/managed.rs`：仅把 Windows compatibility 检查切到明确 Windows/x86_64 entry；不改变 Job/stdio/runtime 行为。
- `src-tauri/src/agent/codex/pool.rs`：依赖 runtime adapter，并把 owner 传给平台恢复；不复制 pool。
- `src-tauri/src/agent/codex/provider.rs`：只把 imports 指向私有 adapter，业务方法不增加平台分支。
- `src-tauri/src/agent/product.rs`：先创建正式 Manager shell，再通过其 crate-private ProbeContext 完成 discovery，最后进入既有 recovery/publication。
- `src-tauri/src/agent/task_manager.rs`：提供 crate-private ProbeContext 与 resolution 安装方法，不改变 dispatch/state graph。
- `src-tauri/src/agent/task_manager/recovery.rs`：只把 runtime/managed imports 指向私有 adapter。
- `src-tauri/src/agent/codex/app_server/recovery.rs`：termination predicate 委托 runtime adapter。
- `src-tauri/src/agent/codex/macos_runtime.rs`：增加 product 接入所需的 stdio handoff 与 initialized adapter method，不改变 create/shutdown/evidence 状态机。
- `src-tauri/src/agent/codex/macos_recovery.rs`：暴露复用现有状态机的单 Runtime adapter 入口，不改变身份或 Claim release 规则。
- 现有 Provider、Pool、App Server 与 recovery 测试：解除仅 Windows 的共享 Contract 测试门禁，并使用 fixture adapter 覆盖 macOS 接入。
- `docs/macos-porting-checklist.md`：只在全部 Gate 后记录 Phase 3A 实际结果和 real smoke 状态。

## Files and Contracts Explicitly Not Modified

- `src-tauri/src/agent/codex/runtime.rs`；
- `src-tauri/src/agent/codex/windows_launcher.rs`；
- Windows Job Object create/assign/terminate/evidence contract；
- StateStore schema 与 migration；
- Execution 11-state graph；
- Dispatch State；
- Workspace Claim 与 Atomic Claim Release；
- `MacosRuntime` 已冻结的 setsid、PID=PGID=SID、identity、SIGTERM/SIGKILL 和 group-empty evidence 语义；
- Serena、Git、uv、cloudflared、signing/notarization；
- 前端/UI 与 Tauri command wire。

## Test Matrix

| Area | Required coverage |
|---|---|
| Host gate | macOS ARM64 accepted；macOS x86_64/Rosetta target rejected |
| Candidate order | PATH、`~/.local/bin`、Homebrew、npm ARM64 vendor、ChatGPT.app final |
| Selection | earlier incompatible candidate does not stop later compatible candidate |
| Paths | symlink、空格、中文、外置卷形式 absolute path、canonical dedupe |
| Reject | no execute bit、shell、JS shim、x86_64-only、wrong version/hash/schema/Contract |
| Mach-O | thin ARM64、thin x86_64、fat with ARM64、fat without ARM64、truncated header |
| Provider | execute、continue、cancel、terminal、finalization、cleanup orchestration |
| Runtime | stdio JSONL、SID/PGID ownership、graceful、SIGKILL escalation、incomplete evidence |
| Recovery | startup dispatch、orphan、unknown、Claim retained、crash/reconcile |
| Real smoke | official npm ARM64 0.153.4 Contract；current installed Codex blocked when not allowlisted |
| Shared regression | fixture Managed Runtime proves Windows/macOS shared Provider semantics unchanged |

## Material Contract Difference / Design Blocker

### Material contract difference

存在一项已由本次需求明确批准的 Material Contract Difference：Compatibility allowlist 从单一全局 binary hash 升级为 `version + os + arch + binary hash + schema hash` 精确 entry。Windows entry 的值与行为保持不变；新增且仅新增 macOS ARM64 entry。

### Design blocker

Probe Ownership Amendment 已由用户批准；按本节把 probe 纳入正式 ownership domain 后不存在新的设计阻断。

当前本机 ChatGPT.app bundled Codex 不在 allowlist，因此本机 installed-binary real smoke 当前预期为 `COMPATIBILITY BLOCKED`；这不是设计阻断，也不能成为放宽规则的理由。官方 npm macOS ARM64 0.153.4 artifact 已提供可冻结的 binary/schema evidence，实施阶段仍需执行真实 App Server Contract Gate。

## Rollback Shape

如果 macOS product integration 在任一 Gate 失败：

- 保留已验证的 compatibility/discovery 单元代码可以单独回滚或继续处于未接入状态；
- `codex/mod.rs` 恢复 macOS 指向 unavailable provider/pool/discovery 即可关闭产品路径；
- 不回滚或修改 Phase 2A/2B 的 StateStore、MacosRuntime 或 macos_recovery 数据契约；
- 不使用临时 x86_64/Rosetta fallback 作为回滚方案。
