# Phase 3A macOS ARM64 Codex Provider Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Apple Silicon macOS 上发现并精确验证 ARM64 Codex Mach-O，把既有 `MacosRuntime`/`macos_recovery` 接入唯一的 `CodexProvider`，同时保持 Windows Runtime/Job Object 契约和共享 Provider 业务语义不变。

**Architecture:** 由 `codex/mod.rs` 集中选择 crate-private discovery、managed 和 runtime adapter。Windows adapter 只转发现有实现；macOS adapter 把同步进程生命周期放入 blocking worker，stdio/RPC 继续使用共享 Tokio async App Server Client。Provider、Pool 和 recovery orchestration 保持单一共享实现，不新增 Runtime trait。

**Tech Stack:** Rust 2024、Tauri 2、Tokio、libc/libproc、Mach-O header parsing、SHA-256、SQLite StateStore、Node test runner。

---

## Preconditions and Stop Rules

- 任务保持 `planning`，直到用户审核本计划并批准 `task.py start`。
- 实施前读取 `prd.md`、`design.md`、相关 Trellis spec/context。
- 所有新增函数和核心逻辑添加中文注释。
- 如果必须改变 `runtime.rs`、`windows_launcher.rs`、Windows Job Object 行为、StateStore schema、Claim release 或 MacosRuntime evidence 状态机，立即停止并提交 Design Change。
- Compatibility Probe 创建的任何真实 `MacosRuntime` 必须使用正式 Store/host owner/existing Pool；禁止 temporary Store、detached runtime、fake Execution/Claim、新 Pool 或新 recovery state machine。
- 当前 `/Applications/ChatGPT.app/Contents/Resources/codex` 是 ARM64 `0.155.0-alpha.9.2`，预期 real smoke 为 `COMPATIBILITY BLOCKED`；不得将其临时加入 allowlist。

### Task 1: Freeze platform-aware compatibility entries

**Files:**
- Create: `src-tauri/src/agent/codex/compatibility.rs`
- Modify: `src-tauri/src/agent/codex/protocol.rs`
- Modify: `src-tauri/src/agent/codex/app_server/managed.rs`
- Modify: `src-tauri/src/agent/codex/app_server/tests.rs`

- [ ] **Step 1: Write failing allowlist tests**

Add tests that construct observed identities for:

```rust
// Windows 原 entry 必须保持可接受。
assert!(check_entry(Target::WindowsX86_64, windows_identity()).is_ok());
// macOS ARM64 使用独立 binary hash。
assert!(check_entry(Target::MacosArm64, macos_arm64_identity()).is_ok());
// 相同 version/schema 但复用 Windows binary hash 必须失败。
assert_eq!(
    check_entry(Target::MacosArm64, windows_identity())
        .unwrap_err()
        .code,
    "CODEX_APP_SERVER_INCOMPATIBLE"
);
// macOS x86_64 不存在 entry。
assert!(entry_for(Target::MacosX86_64).is_none());
```

- [ ] **Step 2: Run focused tests and confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::compatibility -- --nocapture
```

Expected: FAIL because platform-aware compatibility module and target types do not exist.

- [ ] **Step 3: Implement the exact compatibility table**

Create crate-private `CompatibilityEntry` and `Target` values with exactly these entries:

```text
Windows/x86_64 0.153.4 binary 444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B
macOS/arm64   0.153.4 binary B973D440ACAC501FD2594A43E7CA9CE41E0A65B9DFB28D0D7A7837C99E1261E3
schema for both          B06F77062369D481A59CC70720C12B89CB9DD49C385863923262102D3AD6C978
```

Update `CompatibilityIdentity` to check an explicit target. Change Windows managed verification only enough to pass `WindowsX86_64`; do not alter its launcher, Job Object, stdio or reconciliation path.

- [ ] **Step 4: Run compatibility and App Server tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::compatibility -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::app_server -- --nocapture
```

Expected: PASS; existing Windows entry tests retain the same version/hash/schema values.

- [ ] **Step 5: Commit the isolated contract change**

```bash
git add src-tauri/src/agent/codex/compatibility.rs src-tauri/src/agent/codex/protocol.rs src-tauri/src/agent/codex/app_server/managed.rs src-tauri/src/agent/codex/app_server/tests.rs
git commit -m "feat(macos): add arm64 codex compatibility entry"
```

### Task 2: Implement ARM64 Mach-O candidate discovery and selection

**Files:**
- Create: `src-tauri/src/agent/codex/macos_discovery.rs`
- Create: `src-tauri/src/agent/codex/macos_discovery/tests.rs`
- Modify: `src-tauri/src/agent/codex/mod.rs`

- [ ] **Step 1: Write failing source-order and selection tests**

Use isolated temp directories and an injected verifier. Cover:

```rust
// PATH 中错误版本不能阻止后续 npm ARM64 vendor candidate。
let selected = select_compatible(
    enumerate_candidates(&context),
    |path| if path == compatible_vendor { Ok(evidence()) } else { Err(incompatible()) },
).await.unwrap();
assert_eq!(selected, compatible_vendor.canonicalize().unwrap());

// canonical symlink 与真实路径只验证一次。
assert_eq!(verification_count_for(&canonical_target), 1);

// ChatGPT.app 始终位于最后，缺失时不构成全局异常。
assert_eq!(candidates.last(), Some(&chatgpt_candidate));
```

Fixtures 必须包含空格、中文和模拟 `/Volumes/External Drive/...` 的 absolute path suffix。

- [ ] **Step 2: Write failing Mach-O rejection tests**

Create bounded binary headers for thin ARM64, thin x86_64, fat with ARM64, fat without ARM64, shebang script and truncated input. Assert stable codes:

```text
thin ARM64                    accepted for compatibility probe
fat containing ARM64         accepted for compatibility probe
thin/fat x86_64 only         CODEX_ARCH_UNSUPPORTED
script/JavaScript/truncated  CODEX_EXECUTABLE_FORMAT_UNSUPPORTED
no execute bit               CODEX_EXECUTABLE_NOT_RUNNABLE
```

- [ ] **Step 3: Run discovery tests and confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_discovery -- --nocapture
```

Expected: FAIL because macOS discovery functions are not implemented.

- [ ] **Step 4: Implement enumeration, canonical dedupe and ARM64 preflight**

Implement fixed ordered sources without executing shell/npm/node. Parse only the bounded Mach-O/fat headers needed to detect `CPU_TYPE_ARM64`; do not add an object-file dependency. Reject unsupported host architecture before enumeration. Keep the verifier injectable under tests；Task 5 在 managed adapter 存在后再连接 production `discover()`，本任务不提前激活产品路径。

- [ ] **Step 5: Implement continue-on-incompatible selection**

```text
preflight succeeds + compatibility succeeds              → select and stop
runnable ARM64 candidate fails compatibility              → record category and continue
all runnable ARM64 candidates fail compatibility          → CODEX_COMPATIBILITY_BLOCKED
no runnable ARM64 candidate exists                        → BACKEND_UNAVAILABLE
```

Do not include the full process PATH in the error.

- [ ] **Step 6: Run focused discovery tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_discovery -- --nocapture
```

Expected: PASS for the complete Discovery/Reject matrix.

- [ ] **Step 7: Commit discovery**

```bash
git add src-tauri/src/agent/codex/macos_discovery.rs src-tauri/src/agent/codex/macos_discovery/tests.rs src-tauri/src/agent/codex/mod.rs
git commit -m "feat(macos): discover compatible arm64 codex"
```

### Task 3: Add private concrete runtime adapters

**Files:**
- Create: `src-tauri/src/agent/codex/platform_windows.rs`
- Create: `src-tauri/src/agent/codex/macos_runtime_adapter.rs`
- Create: `src-tauri/src/agent/codex/macos_runtime_adapter/tests.rs`
- Modify: `src-tauri/src/agent/codex/mod.rs`
- Modify: `src-tauri/src/agent/codex/macos_runtime.rs`
- Modify: `src-tauri/src/agent/codex/macos_recovery.rs`

- [ ] **Step 1: Write failing adapter shape and ownership tests**

Tests must prove:

```text
create executes on a blocking worker and returns owned stdio
initialized persists version/schema through existing macOS store path
shutdown returns success only with direct-child-reaped + group-empty evidence
shutdown failure returns RuntimeFailure with retained live owner
single-runtime recovery delegates existing macos_recovery rules
Windows adapter evidence predicate accepts exactly the existing two Windows evidence kinds
macOS adapter evidence predicate accepts exactly the existing two macOS evidence kinds
```

- [ ] **Step 2: Run focused adapter tests and confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_runtime_adapter -- --nocapture
```

Expected: FAIL because adapters do not exist.

- [ ] **Step 3: Add Windows forwarding adapter**

Re-export/forward existing `runtime::RuntimeError`, `runtime::RuntimeFailure`, `runtime::recover` and the exact current evidence predicate. Do not edit `runtime.rs` or `windows_launcher.rs`.

- [ ] **Step 4: Add minimal MacosRuntime product hooks**

Add only:

```text
owned stdio handoff
initialized(version, schema)
adapter-safe conversion of create/shutdown failure ownership
single-runtime recovery entry that reuses macos_recovery internals
```

Do not change setsid validation, process identity, signal order, timeout bounds or evidence completion conditions.

- [ ] **Step 5: Implement macOS concrete adapter**

Move `MacosRuntime` into `spawn_blocking` for create/shutdown. Return stdio to Tokio before protocol work. Never hold a Tokio mutex guard across the blocking await. Convert every failure without dropping live ownership.

- [ ] **Step 6: Run adapter and frozen Phase 2 tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_runtime -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_recovery -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_runtime_adapter -- --nocapture
```

Expected: PASS; Phase 2A/2B behavior tests remain unchanged.

- [ ] **Step 7: Commit runtime adapters**

```bash
git add src-tauri/src/agent/codex/platform_windows.rs src-tauri/src/agent/codex/macos_runtime_adapter.rs src-tauri/src/agent/codex/macos_runtime_adapter/tests.rs src-tauri/src/agent/codex/mod.rs src-tauri/src/agent/codex/macos_runtime.rs src-tauri/src/agent/codex/macos_recovery.rs
git commit -m "feat(macos): adapt codex runtime for provider use"
```

### Task 4: Add macOS managed App Server adapter

**Files:**
- Create: `src-tauri/src/agent/codex/app_server/macos_managed.rs`
- Create: `src-tauri/src/agent/codex/app_server/macos_managed/tests.rs`
- Modify: `src-tauri/src/agent/codex/app_server.rs`
- Modify: `src-tauri/src/agent/codex/mod.rs`

- [ ] **Step 1: Write failing compatibility-probe and lifecycle tests**

Cover:

```text
absolute ARM64 executable required
version/hash/schema checked against MacosArm64 entry
wrong hash and wrong schema are rejected independently
stdio remains async after blocking create
Client initialization persists compatibility identity
Client drop wakes reconciliation
initialization/stdio failure retains and terminates Runtime ownership
```

- [ ] **Step 2: Run focused tests and confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::app_server::macos_managed -- --nocapture
```

Expected: FAIL because macOS managed adapter is absent.

- [ ] **Step 3: Implement macOS compatibility probe**

Open and hash the canonical executable, run `--version` and fresh schema export through direct absolute executable + argv, and validate `Target::MacosArm64`. Probe subprocesses must use the same owned process-group cleanup contract; no shell command strings.

- [ ] **Step 4: Implement ManagedClient connection**

Mirror the established internal `ManagedClient` surface while using `macos_runtime_adapter`. Keep `Client::transport`, initialize, JSONL readers/writer, notification dispatch and reconciliation monitor on Tokio async tasks. Move only synchronous OS lifecycle calls to blocking workers.

- [ ] **Step 5: Run managed and protocol tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::app_server::macos_managed -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::app_server -- --nocapture
```

Expected: PASS; protocol framing and existing fixture behavior remain unchanged.

- [ ] **Step 6: Commit managed adapter**

```bash
git add src-tauri/src/agent/codex/app_server/macos_managed.rs src-tauri/src/agent/codex/app_server/macos_managed/tests.rs src-tauri/src/agent/codex/app_server.rs src-tauri/src/agent/codex/mod.rs
git commit -m "feat(macos): connect codex app server runtime"
```

### Task 5: Activate the shared Provider and Pool on macOS

**Files:**
- Modify: `src-tauri/src/agent/codex/mod.rs`
- Modify: `src-tauri/src/agent/codex/provider.rs`
- Modify: `src-tauri/src/agent/codex/pool.rs`
- Modify: `src-tauri/src/agent/task_manager/recovery.rs`
- Modify: `src-tauri/src/agent/codex/app_server/recovery.rs`
- Modify: `src-tauri/src/agent/codex/macos_discovery.rs`
- Modify: existing provider/pool/recovery test modules selected by the affected files

- [ ] **Step 1: Write failing shared Provider Contract tests for macOS adapter selection**

Using Fake/Fixture Managed Runtime, assert:

```text
register -> available only after compatible discovery
execute -> accepted turn -> provider terminal -> finalization
continue -> existing thread/turn binding preserved
cancel -> turn/interrupt invoked, no process-group kill as business cancel
cleanup orchestration -> independent runtime shutdown evidence required
startup recovery dispatch -> macos_recovery summary reaches Provider orchestration
incomplete evidence -> unknown/quarantine and Claim retained
```

- [ ] **Step 2: Run shared tests and confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::provider -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml agent::task_manager::recovery -- --nocapture
```

Expected: at least the macOS real-provider selection assertions FAIL while non-Windows still resolves unavailable modules.

- [ ] **Step 3: Switch macOS to the shared provider/pool**

In `codex/mod.rs`, compile the existing `provider.rs` and `pool.rs` for Windows or macOS, while other platforms keep unavailable modules. Connect production `macos_discovery::discover()` to the real macOS managed verifier. Change shared imports to `managed_adapter`/`runtime_adapter`; do not add operational platform branches inside execute/continue/cancel/finalization methods.

- [ ] **Step 4: Delegate termination and recovery predicates**

Replace hard-coded Windows evidence checks in shared pool/recovery orchestration with adapter calls. Pass owner identity through retained failure/recovery calls. Verify Windows adapter evaluates the exact prior conditions.

- [ ] **Step 5: Run the shared Provider Contract Regression**

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::provider -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::pool -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml agent::task_manager::recovery -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml agent::product -- --nocapture
```

Expected: PASS for execute、continue、cancel、terminal、finalization、cleanup 和 startup recovery fixture contracts.

- [ ] **Step 6: Commit shared integration**

```bash
git add src-tauri/src/agent/codex/mod.rs src-tauri/src/agent/codex/provider.rs src-tauri/src/agent/codex/pool.rs src-tauri/src/agent/codex/macos_discovery.rs src-tauri/src/agent/task_manager/recovery.rs src-tauri/src/agent/codex/app_server/recovery.rs src-tauri/src/agent/codex/provider src-tauri/src/agent/task_manager/recovery
git commit -m "feat(macos): activate shared codex provider"
```

### Task 6: Run macOS ARM64 integration and real smoke gates

**Files:**
- Modify tests only if a failing Gate exposes a missing requirement-backed fixture.
- Modify: `docs/macos-porting-checklist.md` after all code/tests stabilize.

- [ ] **Step 1: Verify the official npm ARM64 artifact evidence**

Use an isolated temporary directory to obtain `@openai/codex@0.153.4-darwin-arm64`, then verify:

```text
file: Mach-O 64-bit executable arm64
version: codex-cli 0.153.4
binary SHA-256: B973D440ACAC501FD2594A43E7CA9CE41E0A65B9DFB28D0D7A7837C99E1261E3
schema SHA-256: B06F77062369D481A59CC70720C12B89CB9DD49C385863923262102D3AD6C978
```

Run the real App Server initialize/Contract fixture against that exact binary. Expected: compatible and direct Mach-O launch, without shell.

- [ ] **Step 2: Run real App Server lifecycle smoke with the allowlisted binary**

Use an isolated read-only fixture Workspace. Directly launch the allowlisted Mach-O, complete App Server initialize/compatibility exchange, verify JSONL stdio, then request orderly Client shutdown and confirm group-empty evidence. This smoke must not start a model turn or consume account usage；execute/continue/cancel remain covered by shared Fake/Fixture Managed Runtime Contract tests.

- [ ] **Step 3: Verify external-volume discovery semantics**

Create and attach a temporary APFS disk image with a volume name containing spaces and Chinese characters, place the allowlisted ARM64 binary under that mounted `/Volumes/...` path, run canonical discovery/compatibility, then detach and remove the image. Expected: direct Mach-O selection succeeds and cleanup leaves no mounted test volume or temporary image.

- [ ] **Step 4: Verify current ChatGPT.app candidate is blocked**

Run discovery with Finder-style minimal PATH and the current installed bundled binary. Expected diagnostic:

```text
COMPATIBILITY BLOCKED
observed version = codex-cli 0.155.0-alpha.9.2
observed hash = 9280C0754E8F1F6B72F495D30C8C82A006DBC4995BF0492916FA0901F6BFD1F9
```

The diagnostic may expose the selected candidate path and stable mismatch category, but not the full PATH environment.

- [ ] **Step 5: Run the macOS Runtime escalation matrix**

Run fixtures for graceful SIGTERM, ignored SIGTERM followed by SIGKILL, stdio EOF, Runtime crash, leader disappearance, incomplete group observation and startup recovery. Expected: only direct-child-reaped + group-empty or recovered group-empty evidence releases ownership/Claim.

- [ ] **Step 6: Run final focused and repository checks**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npm test
npm run lint
npm run build
python3 ./.trellis/scripts/task.py validate 09-22-macos-p3a-codex-provider
git diff --check
```

Expected: all commands PASS. Windows native Host Crash/Job Object Gate remains explicitly pending for a Windows host; shared fixture contract regression must already be green.

- [ ] **Step 7: Update the macOS checklist with observed truth**

Record separately:

```text
Phase 3A implementation status
official npm ARM64 Contract result
current installed ChatGPT.app candidate = COMPATIBILITY BLOCKED or compatible
Windows native validation = pending user Windows test
```

Do not mark Finder real-app flow passed unless it was exercised from the built `.app`.

- [ ] **Step 8: Commit verification documentation**

```bash
git add docs/macos-porting-checklist.md
git commit -m "docs(macos): record phase 3a verification"
```

### Task 7: Apply the approved Probe Ownership Amendment

**Files:**
- Modify: `src-tauri/src/agent/codex/app_server/macos_managed.rs`
- Modify: `src-tauri/src/agent/codex/macos_discovery.rs`
- Modify: `src-tauri/src/agent/codex/mod.rs`
- Modify: `src-tauri/src/agent/product.rs`
- Modify: `src-tauri/src/agent/task_manager.rs`
- Modify: `src-tauri/src/agent/codex/pool.rs` only if a crate-private probe retention entry point is required
- Test: existing macOS managed/discovery/product/pool test modules

- [ ] **Step 1: Write failing formal-domain probe tests**

Add tests proving:

```text
version/schema/App Server probes persist Runtime rows in the supplied formal StateStore
probe rows use the supplied existing host owner
compatibility mismatch continues only after complete termination evidence
termination failure returns retained Runtime ownership and stops selection
retained probe failure is stored in the existing Pool global quarantine key
global quarantine blocks subsequent Workspace execution
restart recovery delegates the durable orphan row to existing macos_recovery
no Execution or Workspace Claim is created for a probe
probe future cancellation at create/read/initialize retains ownership and routes terminate failure into the existing global quarantine
business Runtime creation handoff cancellation also retains any terminate failure instead of dropping `RuntimeFailure.runtime`
```

- [ ] **Step 2: Run focused tests and confirm RED**

```bash
cargo test --manifest-path src-tauri/Cargo.toml probe_ownership -- --nocapture
```

Expected: FAIL because managed probes still create a temporary StateStore and compress `RuntimeFailure` into `ProtocolError`.

- [ ] **Step 3: Introduce crate-private ProbeContext**

Construct `ProbeContext` only from the existing Manager's formal `StateStore`、host owner and `Arc<CodexRuntimePool>`. Do not expose process identity or add a trait. The Windows path continues to call existing discovery/managed code without behavioral changes.

- [ ] **Step 4: Preserve typed probe failure ownership**

Make macOS managed verification return an internal result that distinguishes compatibility failure from `RuntimeFailure`. On termination failure, keep `runtime_id` and `RuntimeFailure.runtime`; do not convert it to a string or `ProtocolError` before the Pool adopts it.

- [ ] **Step 5: Route discovery selection through the formal domain**

Create the Manager shell before discovery. Pass its ProbeContext into macOS discovery. Continue after compatibility rejection only when cleanup completed; on Runtime cleanup failure call the existing Pool retention path with the global quarantine key and stop selection.

- [ ] **Step 6: Remove every temporary probe Store**

Delete temporary `StateStore::open` calls from version/schema/App Server Contract probes. Temporary directories remain permitted only for generated schema output and isolated current working directories; they must not hold Runtime ownership state.

- [ ] **Step 7: Run amendment regressions**

```bash
cargo test --manifest-path src-tauri/Cargo.toml probe_ownership -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_discovery -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::app_server::macos_managed -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml agent::product -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml agent::task_manager::recovery -- --nocapture
```

Expected: PASS；Runtime cleanup failure retains ownership/quarantine and no fake business state exists.

- [ ] **Step 8: Re-run all final checks and real lifecycle smoke**

Repeat Task 6 final commands and the official npm ARM64 lifecycle probe. Expected: all automated checks pass；real probe rows use the formal Store, terminate with complete group-empty evidence, and current ChatGPT.app remains `COMPATIBILITY BLOCKED`.

- [ ] **Step 9: Commit the amendment implementation**

```bash
git add src-tauri/src/agent/codex src-tauri/src/agent/product.rs src-tauri/src/agent/task_manager.rs
git commit -m "fix(macos): retain codex probe runtime ownership"
```

## Final Review Gate

- [x] Diff contains no `MacosCodexProvider`, copied pool, public Runtime trait, macOS x86_64 entry or Rosetta fallback.
- [x] `provider.rs` has no platform-specific business branches; only private adapter imports changed.
- [x] `runtime.rs` and `windows_launcher.rs` are unchanged.
- [x] Windows compatibility values and Job evidence predicate are behaviorally identical.
- [x] macOS startup recovery delegates existing `macos_recovery`; no schema/migration/Claim changes exist.
- [x] Every failed termination retains ownership or leaves durable unknown/quarantine evidence.
- [x] No compatibility probe opens a temporary StateStore or drops `RuntimeFailure.runtime`；all real probe Runtime rows belong to the formal Store/owner/Pool domain.
- [x] Real smoke results distinguish implementation pass from local installed-binary compatibility block.
