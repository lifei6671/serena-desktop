# Phase 1 macOS Compile Baseline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Follow `.trellis/workflow.md`; keep this task active until Phase 3 validation and the single final commit are complete.

**Goal:** 让 `src-tauri` 在 Apple Silicon macOS 上通过 `cargo check --locked`、`cargo test --no-run --locked` 与 `cargo clippy --locked -- -D warnings`，同时保持 Windows Codex Runtime、任务恢复和 Store 数据契约不变。

**Architecture:** `agent::product`、`agent::work`、`agent::task_manager` 保持跨平台；`agent::codex` 在 Windows 选择现有 Runtime 实现，在非 Windows 选择显式 unavailable 的 `discovery`、`pool` 与 `provider`。macOS Phase 1 不启动 Codex Runtime、不认领遗留执行、不伪造恢复证据；Start/ResumePending、Continue、Cancel 分别保持现有 `BACKEND_UNAVAILABLE`、`AGENT_CONTINUE_NOT_ALLOWED`、`AGENT_PROVIDER_UNAVAILABLE` 契约，Store 只读能力保持可用。

**Tech Stack:** Rust 2024、Tokio、Tauri v2、SQLite/rusqlite、`tokio_util::sync::CancellationToken`、Cargo tests/clippy。

---

## 实施约束

- 只建立 macOS 编译基线和平台契约；Unix process group、真实 Codex Runtime、通知、Remote Access、DMG 属于后续子任务。
- 新增函数、结构体职责和关键分支必须写中文注释。
- Windows 模块沿用现有文件，不改写其执行、恢复、Job Object 或持久化语义。
- unavailable 后端只提供产品层当前需要的最小接口，不返回成功占位结果。
- Trellis 要求 Phase 3 统一提交一次；各 Task 末尾只做工作区验证。

## Task 1：建立平台选择边界

**Files:**

- Modify: `src-tauri/src/agent/mod.rs`
- Modify: `src-tauri/src/agent/codex/mod.rs`
- Create: `src-tauri/src/agent/codex/unavailable/discovery.rs`
- Create: `src-tauri/src/agent/codex/unavailable/pool.rs`
- Create: `src-tauri/src/agent/codex/unavailable/provider.rs`

### Step 1：先写非 Windows discovery 契约测试

新建 `unavailable/discovery.rs`，先固定失败契约：

```rust
use std::path::PathBuf;

/// Phase 1 在非 Windows 平台不启动 Codex Runtime。
pub async fn discover() -> Result<PathBuf, String> {
    Err("BACKEND_UNAVAILABLE: Codex runtime is unavailable on this platform".into())
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn non_windows_discovery_is_explicitly_unavailable() {
        let error = super::discover().await.unwrap_err();
        assert!(error.starts_with("BACKEND_UNAVAILABLE:"));
    }
}
```

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked non_windows_discovery_is_explicitly_unavailable --no-run
```

Expected: FAIL；新模块尚未接入模块树。

### Step 2：移除产品层 Windows 总开关

把 `agent/mod.rs` 中 `task_manager`、`product`、`work` 改为无条件 `pub mod`，不改其他模块可见性。

### Step 3：在 `codex/mod.rs` 建立显式平台选择

```rust
pub mod app_server;
pub mod protocol;

#[cfg(windows)]
pub(crate) mod pool;
#[cfg(not(windows))]
#[path = "unavailable/pool.rs"]
pub(crate) mod pool;

#[cfg(windows)]
pub mod provider;
#[cfg(not(windows))]
#[path = "unavailable/provider.rs"]
pub mod provider;

#[cfg(windows)]
pub mod runtime;
#[cfg(windows)]
pub mod windows_launcher;

#[cfg(windows)]
pub mod discovery;
#[cfg(not(windows))]
#[path = "unavailable/discovery.rs"]
pub mod discovery;
```

`app_server::managed` 继续保持 `#[cfg(windows)]`。

### Step 4：实现最小 unavailable pool

```rust
use tokio_util::sync::CancellationToken;

/// 非 Windows 资源池只传播停止信号，不创建 Runtime。
pub(crate) struct CodexRuntimePool {
    pub stop: CancellationToken,
}

impl Default for CodexRuntimePool {
    fn default() -> Self {
        Self { stop: CancellationToken::new() }
    }
}

impl CodexRuntimePool {
    /// Phase 1 不允许在非 Windows 平台执行 workspace Runtime 探测。
    pub(crate) fn check_workspace(&self, _workspace: &str) -> Result<(), String> {
        Err("AGENT_PROVIDER_UNAVAILABLE".into())
    }

    /// 该实现从未拥有子进程，关闭时只取消等待任务。
    pub(crate) async fn shutdown(&self) -> Result<(), String> {
        self.stop.cancel();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool { true }
}
```

这些签名与现有 Windows pool 的 `check_workspace(&str)`、`shutdown()`、`is_empty()` 对齐；不要给 unavailable pool 增加 `lease`、`enter` 等无调用方接口。

### Step 5：重跑 discovery 测试

Run: `cargo test --manifest-path src-tauri/Cargo.toml --locked non_windows_discovery_is_explicitly_unavailable`

Expected: 测试逻辑 PASS；若 provider 尚未完成导致 crate 编译失败，继续 Task 2，不用临时 cfg 隐藏产品模块。

## Task 2：实现 unavailable Provider 并接通 TaskManager

**Files:**

- Modify: `src-tauri/src/agent/codex/unavailable/provider.rs`
- Modify: `src-tauri/src/agent/task_manager.rs`

### Step 1：先写注册失败闭合测试

在 unavailable provider 模块内新增测试：创建临时 `StateStore` 与 `ProviderRegistry`，调用与 Windows 同签名的 `register_codex_provider_with_discovery`，随后断言 `registry.get(ProviderId::new("codex"))` 返回 `AgentProviderUnavailable`。

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked unavailable_provider_is_registered_but_cannot_be_dispatched --no-run
```

Expected: FAIL；注册函数尚未实现。

### Step 2：实现最小 unavailable provider

实现必须满足：

- descriptor：id=`codex`、display name=`Codex`、version=`None`。
- capabilities：execute/continue/cancel/recover/activity/token_usage 全部为 `false`。
- execute：`ProviderExecutionFailure::State("AGENT_PROVIDER_UNAVAILABLE".into())`。
- cancel/startup_reconcile：`ProviderErrorCode::AgentProviderUnavailable`。
- 注册函数签名与 Windows 版本一致，使用 `ProviderHealth::Unavailable`。
- `StateStore`、owner、pool、discovery 参数只保持调用契约，不产生运行副作用。

核心 trait 实现：

```rust
/// 非 Windows 平台保留稳定 Provider 身份，但拒绝所有运行能力。
struct UnavailableCodexProvider;

fn unavailable_error() -> ProviderError {
    ProviderError { code: ProviderErrorCode::AgentProviderUnavailable }
}

impl AgentProvider for UnavailableCodexProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: ProviderId::new("codex"),
            display_name: "Codex".into(),
            version: None,
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            can_execute: false,
            can_continue: false,
            can_cancel: false,
            can_recover: false,
            activity: false,
            token_usage: false,
        }
    }

    fn execute<'a>(
        &'a self,
        _context: ProviderExecutionContext,
        _acceptance: Arc<dyn ProviderAcceptanceSink>,
        _telemetry: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>> {
        Box::pin(async {
            Err(ProviderExecutionFailure::State("AGENT_PROVIDER_UNAVAILABLE".into()))
        })
    }

    fn cancel<'a>(&'a self, _context: ProviderCancelContext)
        -> ProviderFuture<'a, Result<(), ProviderError>> {
        Box::pin(async { Err(unavailable_error()) })
    }

    fn startup_reconcile<'a>(&'a self, _context: ProviderStartupContext)
        -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
        Box::pin(async { Err(unavailable_error()) })
    }
}
```

字段以当前 `provider/mod.rs` 定义为准，不增加兼容字段。

### Step 3：隔离 TaskManager 中仅供 Windows Runtime 使用的代码

在 `task_manager.rs`：

- `CodexProvider` import 改为 `#[cfg(all(test, windows))]`。
- `test_client` 字段保持 `#[cfg(test)]`。
- 构造 `CodexProvider` 并调用 `run_client_with_acceptance_and_telemetry` 的 Fake wire 分支改为 `#[cfg(all(test, windows))]`。
- `pub mod recovery` 改为 `#[cfg(windows)]`。
- 生产 dispatch 始终经过 registry；macOS 在创建 Runtime worker 前返回 unavailable。

### Step 4：定向验证

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked unavailable_provider_is_registered_but_cannot_be_dispatched
cargo test --manifest-path src-tauri/Cargo.toml --locked agent::task_manager::tests
```

Expected: PASS；TaskManager 的 Store/fake-provider 测试继续跨平台，Codex fake wire 仅限 Windows。

## Task 3：保留跨平台产品测试，隔离 Windows Runtime 测试

**Files:**

- Modify: `src-tauri/src/agent/product/tests.rs`

### Step 1：先编译产品测试

Run: `cargo test --manifest-path src-tauri/Cargo.toml --locked agent::product --no-run`

Expected: FAIL；剩余错误只应来自 Fake Codex Runtime、Windows recovery 或关联 helper。

### Step 2：精确隔离 Windows 测试

- `persistence_tests` 和 `restart_tests` 继续整模块 `#[cfg(windows)]`；`control_tests`、`observe_tests`、`orchestration_tests`、`work_adapter_tests` 必须跨平台编译。
- `usage_projection_tests`、`workspace_write_tests` 保持跨平台；其中若有直接启动 Codex Runtime 的单个 smoke test，只 gate 该测试。
- `Client` 与 Fake wire 的 tokio IO import、`fake_service`、`fake_service_with_turn_started` 加 `#[cfg(windows)]`。
- `product/tests.rs` 中依赖 Fake Runtime/恢复的现有测试保留单测试 `#[cfg(windows)]`，但纯源码边界测试 `continuation_core_and_product_action_filter_are_provider_opaque` 移除 gate。
- `control_tests` 仅 gate：`quarantine_pending_receipts_preserve_identity_without_replay`、`control_idempotent_running_retry_and_terminal_receipt_do_not_dispatch_twice`、`backend_failure_pending_can_resume_first_dispatch_or_cancel`、`binary_resolution_failure_pending_can_resume_first_dispatch`、`unbound_persisted_runtime_attempt_stays_fail_closed`。
- `observe_tests` 仅 gate：`dropping_observer_does_not_stop_owned_worker_or_create_another_turn`。
- `orchestration_tests` 仅 gate：`public_transport_preserves_context_idempotency_and_continuation_pipeline`、`public_finish_and_work_cancel_leave_execution_and_claim_to_agent_cancel`、`public_resume_pending_http_reuses_guard_and_returns_success_is_error_false`、`public_vertical_work_source_start_continue_acceptance_e2e`；`accepted_transport_errors_project_original_control_instead_of_rejecting` 保持跨平台。
- `work_adapter_tests` 仅 gate：`start_durable_receipt_retry_lineage_and_continuation_reuse_existing_worker`、`resume_pending_uses_existing_explicit_pipeline_and_rejects_replay`、`dropping_adapter_caller_and_observer_keeps_owned_execution_running`、`invalid_persisted_activity_rejects_cancel_without_claim_side_effect`、`cancel_transaction_failure_remains_rejected_without_mutation`、`resolver_start_and_remove_share_supervisor_operation_exclusion`。`adapter_validation_and_start_work_guards_create_nothing` 使用平台 fixture 保持跨平台拒绝覆盖：Windows 使用 fake Runtime 并断言零调用，非 Windows 使用真实 unavailable Product。
- `work_context_tests` 仅 gate：`versioned_context_is_frozen_before_dispatch_and_retries_ignore_file_drift`、`context_junction_outside_work_root_is_stale_before_creation`；`known_raw_sha_and_empty_context_are_valid_without_source_copying` 保持跨平台。`invalid_and_stale_context_have_zero_durable_runtime_or_provider_effects` 同样使用平台 fixture，保留跨平台 Context 拒绝与零持久化副作用断言。

最终审查修正：不再以整模块 gate 隐藏 Store/DTO/协议测试，也不对纯源码架构断言加 Windows gate。

以下纯业务/Store 测试必须继续在 macOS 运行：`workspace_claim_exists_forwards_the_authoritative_store_lookup`、`initialize_starts_exactly_one_auto_recovery_worker`、`thread_names_are_shared_persistent_without_control_revision_change`、两个 provider opaque 投影测试、`desktop_history_pages_are_read_only_stable_and_workspace_filtered`、`lineage_atomic_guards_and_read_projection`、`product_dto_rejects_unknown_fields_and_limits`、`execution_context_projection_is_frozen_and_read_only`。

### Step 3：加强初始化回归断言

在 `initialize_starts_exactly_one_auto_recovery_worker` 中断言 `backend_diagnostic()` 包含 `BACKEND_UNAVAILABLE:`，继续使用现有 `TEST_DISCOVERY.scope(...)`，不依赖本机 `codex`。

### Step 4：验证

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked workspace_claim_exists_forwards_the_authoritative_store_lookup
cargo test --manifest-path src-tauri/Cargo.toml --locked product_dto_rejects_unknown_fields_and_limits
cargo test --manifest-path src-tauri/Cargo.toml --locked initialize_starts_exactly_one_auto_recovery_worker
cargo test --manifest-path src-tauri/Cargo.toml --locked --no-run
```

Expected: PASS；macOS 不编译 Windows fake Runtime/recovery 测试，但仍运行 Store/DTO/只读投影测试。

## Task 4：修复 Serena 与 Unix 编译边界

**Files:**

- Modify: `src-tauri/src/serena_capability.rs`
- Modify: `src-tauri/src/mcp/source_write_atomic_replace.rs`
- Modify: `src-tauri/src/mcp/source_find.rs`
- Modify: `src-tauri/src/serena.rs`
- Modify: `src-tauri/src/agent/store.rs`

### Step 1：先写 Serena 失败闭合测试

在现有 Serena capability 测试模块增加以下非 Windows 测试。它传入不存在的 executable；正确实现必须在 spawn 前返回统一错误：

```rust
#[cfg(not(windows))]
#[tokio::test]
async fn start_runtime_is_deferred_before_spawning_on_non_windows() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let detected = installation(
        InstallationState::Installed,
        directory.path().join("must-not-spawn"),
        "fixture",
    );
    let provider = provider(detected.clone(), |_| Ok(true));
    let result = provider
        .start_runtime(
            &lease(workspace.clone()),
            detected,
            &directory.path().join("home"),
            &directory.path().join("context.yaml"),
            39_321,
        )
        .await;

    let Err(error) = result else {
        panic!("non-Windows Phase 1 must not publish a Serena Runtime");
    };
    assert_eq!(error, deferred_operation());
    assert!(provider.runtimes.lock().await.is_empty());
}
```

Run: `cargo test --manifest-path src-tauri/Cargo.toml --locked start_runtime_is_deferred_before_spawning_on_non_windows`

Expected: FAIL；当前非 Windows 路径仍尝试 spawn。

### Step 2：拆分 Serena 平台边界

- `hidden_command`、`terminate_managed_process` 保持跨平台 import。
- `contain_process`、`terminate_managed_job` 仅在 `#[cfg(windows)]` import。
- 保留 `SerenaRuntime.job` 的现有 `#[cfg(windows)]`。
- 当前 `start_runtime` 实现加 `#[cfg(windows)]`。
- 新增同签名 `#[cfg(not(windows))] start_runtime`，在任何 `Command::spawn` 前返回 `deferred_operation()`。
- 不改短生命周期 index 进程已有的 Unix `process_group(0)`。

### Step 3：修复真实 Unix `ErrorKind` 错误

`source_write_atomic_replace.rs` import 改为：

```rust
use std::io::{self, ErrorKind, Write};
#[cfg(windows)]
use std::io::Read;
```

不要改变 `AlreadyExists` 映射。

### Step 4：消除平台相关 warning

- `source_find.rs`：Windows 使用 `let mut builder` 并调用 `case_insensitive(true)`；非 Windows 使用不可变 `builder`。
- `serena.rs::hidden_command`：Windows 使用 `mut Command` 并设置 `CREATE_NO_WINDOW`；非 Windows 使用不可变绑定。
- `agent/store.rs`：只对确实仅被 Windows provider 消费的 `CodexUsageBaselineIntent`、`USAGE_TERMINAL_GRACE_MS` re-export 加 `#[cfg(windows)]`；若 Store 测试使用某项，则该项保持无条件。
- 不使用 `#[allow(unused_*)]` 掩盖问题。

### Step 5：定向验证

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked start_runtime_is_deferred_before_spawning_on_non_windows
cargo test --manifest-path src-tauri/Cargo.toml --locked mcp::source_write
cargo test --manifest-path src-tauri/Cargo.toml --locked agent::store
```

Expected: PASS；macOS Serena 长驻 Runtime 在 spawn 前失败，原子写入与 Store 行为不变。

## Task 4b：收口测试平台边界与 Rust 1.96 lint

**Files:**

- Modify: `src-tauri/src/agent/store/usage_tests.rs`
- Modify: `src-tauri/src/agent/codex/app_server/tests.rs`
- Modify: `src-tauri/src/mcp/process.rs`
- Modify: `src-tauri/src/agent/codex/protocol.rs`
- Modify: `src-tauri/src/remote/manager.rs`

### Step 1：保留失败门槛作为 RED 证据

`cargo test --manifest-path src-tauri/Cargo.toml --locked --no-run` 当前稳定暴露四个 Windows-only 测试引用；`cargo clippy --manifest-path src-tauri/Cargo.toml --locked -- -D warnings` 当前稳定暴露三个 Rust 1.96 `nonminimal_bool`。不得扩大 cfg 或添加 allow。

### Step 2：精确隔离 Runtime 测试引用

- `store/usage_tests.rs` 只给 `freeze_and_runtime_termination_preserve_public_usage_and_isolate_runtimes` 加 `#[cfg(windows)]`。
- `codex/app_server/tests.rs` 给 `InjectedRuntimeMismatch`、`TestAcceptanceSink`、`turn_notification`、`run_provider_with_injected_runtime_mismatch` 及其两个调用测试加 `#[cfg(windows)]`；其他协议测试保持跨平台。
- `total_deadline_and_metadata_contract_are_explicit` 中仅把创建 Store、`prepare_runtime` 与 `after_termination` 的尾部断言块放入 `#[cfg(windows)]`，前面的 deadline/metadata 协议断言继续跨平台。
- `mcp/process.rs` 的测试全部依赖 Windows，因此将整个 tests 模块改为 `#[cfg(all(test, windows))]`，删除内部重复 cfg 不作额外重构。

### Step 3：按 clippy 建议做等价表达式替换

- `protocol.rs`：`!value.as_str().is_some_and(|s| !s.is_empty())` 改为 `value.as_str().is_none_or(str::is_empty)`。
- `remote/manager.rs`：`!context.is_some()` 改为 `context.is_none()`。
- `remote/manager.rs`：OAuth instance 判断改为 `.as_ref().is_none_or(|o| o.context.instance_id != context.instance_id)`。

### Step 4：验证

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked --no-run
cargo clippy --manifest-path src-tauri/Cargo.toml --locked -- -D warnings
```

Expected: 两条均 PASS；不减少纯协议、Store usage 或跨平台进程测试覆盖。

## Task 5：Phase 1 完整验证与 Trellis 收尾

**Files:**

- Modify: `.trellis/tasks/09-21-macos-p1-compile-baseline/prd.md`
- Modify: `.trellis/tasks/09-21-macos-p1-compile-baseline/task.json`
- Modify: `.trellis/workspace/codex/journal-*.md`

### Step 1：格式与三道硬门槛

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml --locked
cargo test --manifest-path src-tauri/Cargo.toml --locked --no-run
cargo clippy --manifest-path src-tauri/Cargo.toml --locked -- -D warnings
```

Expected: 全部 exit 0。若 fmt 失败，只运行项目 formatter 后重试；不得删除有效测试或弱化 `-D warnings`。

### Step 2：运行回归测试

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked unavailable_provider_is_registered_but_cannot_be_dispatched
cargo test --manifest-path src-tauri/Cargo.toml --locked initialize_starts_exactly_one_auto_recovery_worker
cargo test --manifest-path src-tauri/Cargo.toml --locked start_runtime_is_deferred_before_spawning_on_non_windows
cargo test --manifest-path src-tauri/Cargo.toml --locked agent::store
```

Expected: 全部 PASS。

### Step 3：检查修改范围

```bash
git status --short
git diff --stat
git diff -- src-tauri/src/agent src-tauri/src/serena.rs src-tauri/src/serena_capability.rs src-tauri/src/mcp/source_find.rs src-tauri/src/mcp/source_write_atomic_replace.rs
```

Expected: 只有 Phase 1 源码、测试和当前 Trellis 任务产物；不改 `docs/macos-porting-checklist.md` 用户内容或后续阶段文件。

### Step 4：更新 Trellis 证据

- 在 `prd.md` 逐项勾选有命令证据支持的 acceptance criteria。
- 将硬门槛和定向测试实际结果写入当前 journal。
- 运行 `python3 .trellis/scripts/task.py update-phase 09-21-macos-p1-compile-baseline 3`。
- 按 `.trellis/workflow.md` 完成检查阶段，全部通过后再 finish。

### Step 5：Trellis 单次提交

```bash
git add src-tauri/src/agent src-tauri/src/serena.rs src-tauri/src/serena_capability.rs src-tauri/src/mcp/source_find.rs src-tauri/src/mcp/source_write_atomic_replace.rs .trellis/tasks/09-21-macos-p1-compile-baseline .trellis/tasks/09-21-macos-platform-porting .trellis/workspace/codex
git diff --cached --check
git commit -m "fix(macos): establish rust compile baseline"
```

若 workspace 或 task metadata 被项目 ignore，遵守现有 ignore，不用 `-f` 强行加入。提交前确认 staged diff 不含无关用户修改。

### Step 6：完成子任务并同步父任务

Run: `python3 .trellis/scripts/task.py finish 09-21-macos-p1-compile-baseline`

随后仅勾选父任务的 Phase 1 项，不提前勾选 Phase 2–6。

## 完成判据

- macOS 上 check、test no-run、clippy `-D warnings` 全部通过。
- Windows Codex provider/runtime/recovery 没有行为改写。
- macOS Codex Runtime 操作按产品契约返回：Start/ResumePending 为 `BACKEND_UNAVAILABLE`、Continue 为 `AGENT_CONTINUE_NOT_ALLOWED`、Cancel 为 `AGENT_PROVIDER_UNAVAILABLE`；Store 读取与产品 DTO 测试仍运行。
- macOS 不执行恢复、不认领遗留进程、不伪造 Runtime 成功证据。
- Serena 长驻 Runtime 在 spawn 前明确失败。
- 修改范围仅限 Phase 1 和 Trellis 任务记录。
