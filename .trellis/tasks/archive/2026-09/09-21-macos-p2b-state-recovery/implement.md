# Phase 2B：State Store 与恢复集成实施计划

> 采用测试先行。每个任务先运行指定测试确认 RED，再完成最小实现至 GREEN；不得通过弱化既有测试或改变 Windows 行为过关。

## 实施原则

- 所有新增函数和核心分支使用中文注释。
- macOS 代码仅在 `cfg(target_os = "macos")` 下编译；Linux unavailable 行为保持原样。
- 不引入跨平台 Runtime trait、通用 containment abstraction 或新依赖。
- `unknown` 与 Claim 保留是所有证据不足路径的默认结果。
- 每个阶段结束运行 `git diff --check`；发现设计前提不成立时回到 planning，不扩大实现范围。

## Task 1：冻结 v9 fixture 并增加 schema v10 migration

**Files**

- Create: `src-tauri/src/agent/schema_v10.sql`
- Create: `src-tauri/tests/fixtures/agent_state_v9.sql`
- Modify: `src-tauri/src/agent/store.rs`
- Modify: `src-tauri/src/agent/store/tests.rs`

### 1.1 RED：增加真实 v9 升级测试

新增 `migrates_frozen_v9_fixture_to_v10`：从 fixture 创建数据库，确认 fixture 的 Windows Runtime、Execution、Claim 与 `user_version == 9`，调用 `migrate` 后断言：

- `user_version == 10`；
- 历史 Runtime 为 `windows/windows_job/windows_filetime_v1`；
- macOS PGID/SID/verified-at 为 `NULL`；
- 原数据、外键关系和终止证据不变。

运行并确认因缺少 v10 而失败：

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::store::tests::migrates_frozen_v9_fixture_to_v10 -- --exact
```

### 1.2 GREEN：实现最小 v10 schema

- 在 `store.rs` 注册 `SCHEMA_V10`，支持 `1..=10`，在 `version < 10` 时应用 migration。
- `schema_v10.sql` 添加设计文档中的六个字段及 insert/update validation triggers。
- migration 末尾执行验证性 no-op `UPDATE`，让所有历史行经过新触发器。
- fixture 必须是冻结 SQL，不得调用当前 `SCHEMA_V*` 拼装旧库。

### 1.3 RED/GREEN：验证 migration 原子回滚

新增 `v10_migration_failure_preserves_v9_database`：在 v9 fixture 中安装只拒绝验证性 `UPDATE` 的 trigger，调用 migration 后断言失败，并确认：

- `user_version == 9`；
- 新增列和 v10 triggers 不存在；
- Runtime、Execution、Claim 数据完全保留。

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::store::tests::v10_migration_failure_preserves_v9_database -- --exact
```

### 1.4 RED/GREEN：覆盖约束矩阵

增加 focused tests，分别验证合法 Windows/macOS 行通过，以及以下非法组合被 SQLite 拒绝：

- 平台与 containment/identity scheme 不匹配；
- Windows 行携带 macOS PGID/SID；
- macOS 行携带 Windows Job 字段；
- macOS 身份组部分缺失或 PID/PGID/SID 不相等；
- active macOS state 缺少完整身份；
- complete evidence kind 与平台不匹配。

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::store::tests::v10_
```

**Gate**：v9 成功迁移、失败完整回滚、约束矩阵通过；现有 Store tests 仍通过。

## Task 2：扩展 RuntimeRecord 与平台条件 release gate

**Files**

- Modify: `src-tauri/src/agent/store.rs`
- Modify: `src-tauri/src/agent/store/transactions.rs`
- Modify: `src-tauri/src/agent/store/transactions/tests.rs`

### 2.1 RED：增加 RuntimeRecord 投影测试

新增测试读取 Windows 和 macOS Runtime，断言六个 v10 字段按数据库原值返回，不从 Job 字段推断平台。

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::store::tests::runtime_record_projects_v10_platform_evidence -- --exact
```

### 2.2 GREEN：扩展只读投影

- 给 `RuntimeRecord` 增加平台、containment、identity scheme、PGID、SID、verified-at 字段。
- 扩展 `StateStore::runtime` SELECT 和列映射。
- 不改变现有 Windows 字段类型和调用方语义。

### 2.3 RED：增加 Claim release evidence 矩阵

在 transaction tests 增加：

- 合法 Windows evidence 继续允许 `RuntimeTerminated`；
- 合法 macOS live/recovered group-empty evidence 允许释放；
- 平台、containment、scheme 或 evidence type 任一不匹配时返回 `RUNTIME_TERMINATION_EVIDENCE_REQUIRED`，Claim 保留；
- 仅有 `state='terminated'` 不允许释放。

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::store::transactions::tests::runtime_termination_
```

### 2.4 GREEN：收紧 `terminated_runtime`

把 release gate 改为平台条件 SQL：Windows 分支保持既有 Job evidence 判断；macOS 分支要求完整 macOS 字段组及两个允许的 group-empty evidence type。不得修改 Local Human Authority 逻辑。

**Gate**：transaction focused tests 通过，既有 Windows evidence tests 不变。

## Task 3：版本化 macOS start token

**Files**

- Modify: `src-tauri/src/agent/codex/macos_launcher.rs`
- Modify: `src-tauri/src/agent/codex/macos_launcher/tests.rs`

### 3.1 RED：增加 token codec 测试

覆盖 round-trip、错误前缀、字段缺失/多余、非数字、负数、微秒越界。测试只使用私有 `ProcessStartToken`，不得暴露 libproc 类型。

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_launcher::tests::process_start_token_
```

### 3.2 GREEN：实现私有 codec

- 为 `ProcessStartToken` 增加 `encode`/`decode`，固定格式 `darwin_proc_bsd_start_v1:<seconds>:<microseconds>`。
- 仅把 Rust 标量/字符串开放给 sibling macOS 模块；`proc_bsdinfo` 仍留在 identity adapter 内。

**Gate**：launcher focused tests 与 Phase 2A 真实 identity tests 通过。

## Task 4：新增 macOS Runtime Store adapter

**Files**

- Create: `src-tauri/src/agent/codex/macos_runtime_store.rs`
- Modify: `src-tauri/src/agent/codex/mod.rs`
- Modify: `src-tauri/src/agent/store.rs`
- Modify: `src-tauri/src/agent/store/tests.rs`

### 4.1 RED：定义 Store 状态迁移测试

覆盖：

- `prepare` 创建无身份的 macOS `preparing` 行；
- `start` 原子写入 PID/token/PGID/SID/verified-at 并进入 `starting`；
- initialized、terminating、unknown 的合法状态转换；
- live/recovered complete evidence 分别写入允许的 type；
- 0 行或多次冲突更新返回稳定的 Store error；
- complete evidence commit 同步冻结当前 Runtime usage，与 Windows 语义一致。

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::store::tests::macos_runtime_store_
```

### 4.2 GREEN：实现最小 adapter

- 新模块只编译于 macOS，复用 `StateStore` 的 connection/transaction 边界。
- 使用 macOS 私有 error/evidence 输入，不改变 Windows `runtime_store.rs`。
- Store 方法只接受调用者已观察到的事实，不自行查询 OS 或推断证据。

**Gate**：macOS Store tests 通过，Windows runtime_store 文件无 diff。

## Task 5：把 live MacosRuntime 接入 StateStore

**Files**

- Modify: `src-tauri/src/agent/codex/macos_runtime.rs`
- Modify: `src-tauri/src/agent/codex/macos_runtime/tests.rs`
- Modify: `src-tauri/tests/fixtures/macos_runtime_child.rs`（仅当现有模式不足以制造确定性退出场景）

### 5.1 RED：增加 live persistence 与 ownership tests

覆盖：

- create 前 Store prepare 失败时不 spawn；
- spawn 后 start persistence 失败时，failure 仍携带可 shutdown 的 Runtime；
- 创建成功后 RuntimeRecord 精确保存 `PID == PGID == SID` 与 token；
- 正常退出、SIGTERM、SIGKILL 均只在 child reaped + group empty 后提交 `macos_live_process_group_empty`；
- identity mismatch、group observation failure 或 evidence commit failure 时写/保持 unknown，不形成 complete evidence。

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_runtime::tests::store_
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_runtime::tests::shutdown_
```

### 5.2 GREEN：最小接线

- `MacosRuntime` 持有 `StateStore`，create 参数增加 owner/store。
- launch 前写 preparing；launch 后立即持久化已验证身份。
- shutdown 开始写 terminating，成功时提交 sealed live evidence，失败时尽力写 unknown。
- Store 写失败必须保留 `Box<MacosRuntime>` ownership；不得在 `Drop` 中伪造终止证据。

**Gate**：Phase 2A 全部 runtime tests 加新增 Store tests 通过。

## Task 6：实现跨 Host macOS Runtime recovery

**Files**

- Create: `src-tauri/src/agent/codex/macos_recovery.rs`
- Create: `src-tauri/src/agent/codex/macos_recovery/tests.rs`
- Modify: `src-tauri/src/agent/codex/mod.rs`
- Modify: `src-tauri/src/agent/codex/macos_launcher.rs`

### 6.1 RED：身份验证和 no-signal tests

使用真实 macOS child fixture 与最小 test seam，覆盖：

- PID/token/PGID/SID 完全匹配且 leader 在 group 中才进入 signal 阶段；
- leader 已不存在，即使 group empty，也返回 unknown；
- token、PGID、SID 任一不匹配不发信号，无关进程继续存活；
- group member 查询失败不发信号。

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_recovery::tests::identity_
```

### 6.2 RED：有界终止 tests

覆盖：

- SIGTERM 后 group empty，提交 `macos_recovered_process_group_empty`；
- grace 到期且 leader 身份仍匹配，才发送 SIGKILL；
- TERM 后 leader 消失但 group 非空，停止并 unknown，不发送 KILL；
- KILL 后 group 未空、查询失败或 evidence commit 失败均 unknown。

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_recovery::tests::termination_
```

### 6.3 GREEN：实现恢复状态机

- recovery 读取 `RuntimeRecord` 并验证平台/containment/scheme/完整身份。
- 复用 macOS identity adapter 的标量身份观察和 group 查询；不复用 live direct-child evidence。
- 严格按设计的 TERM/grace/revalidate/KILL/bounded-wait 顺序执行。
- 只在 group-empty 被成功观察并提交后返回 recovered；其他路径返回 stable unknown outcome。

**Gate**：所有 recovery tests 通过，no-signal tests 明确验证无关进程未被终止。

## Task 7：接入 startup Claim recovery

**Files**

- Modify: `src-tauri/src/agent/codex/macos_recovery.rs`
- Modify: `src-tauri/src/agent/codex/macos_recovery/tests.rs`
- Modify: `src-tauri/src/agent/codex/unavailable/provider.rs`

### 7.1 RED：增加 startup reconciliation matrix

构造 Runtime/Execution/Claim 持久化场景，覆盖：

- orphan macOS Runtime 被独立恢复，不改变无关联 Execution；
- `PendingExplicitResume` 保留；
- 已有合法 complete release evidence 幂等清理；
- Pending/Unknown + 可验证 macOS Runtime 恢复为 interrupted 并原子释放 Claim；
- 缺失 Runtime、Windows Runtime、identity mismatch 或 containment evidence 不足时 Execution 为 unknown、Claim 保留；
- evidence 已提交但 final release 首次失败时 Claim 保留，再次启动可幂等完成。

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_recovery::tests::startup_
```

### 7.2 GREEN：复用既有 Claim authority

- 调用 `recover_claims` 分类，不复制 Claim 状态机。
- complete recovered evidence 通过既有 `ResumeRecovery(RuntimeTermination)` 与 `finish_runtime_terminated` 完成 interrupted finalization。
- 任何 provider/Store/release 失败映射为现有 `ProviderReconcileKind`，不删除 Claim。

### 7.3 RED/GREEN：macOS provider capability 接线

- macOS `UnavailableCodexProvider` 保存 Store/owner，`can_recover=true`，`startup_reconcile` 调用 macOS recovery。
- execute/continue/cancel 仍 false/Unavailable，Registry health 仍 Unavailable。
- 非 macOS `cfg(not(any(windows, target_os = "macos")))` 保持 `can_recover=false` 和当前错误。

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::provider::tests::macos_provider_only_enables_recovery -- --exact
```

**Gate**：startup matrix 和 provider capability tests 通过；未修改 task-manager 调度协议。

## Task 8：回归、真实 macOS 验证与收尾

### 8.1 Focused suite

```bash
cargo test --manifest-path src-tauri/Cargo.toml agent::store
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_launcher
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_runtime
cargo test --manifest-path src-tauri/Cargo.toml agent::codex::macos_recovery
```

### 8.2 全量验证

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
npm test
git diff --check
```

### 8.3 最终审查

- 确认 `src-tauri/src/agent/codex/runtime.rs` 与 `windows_launcher.rs` 无 diff。
- 检查所有新增函数与核心逻辑具有中文注释。
- 检查没有启用 macOS execute/continue/cancel 或 discovery。
- 检查 Process Group 文档和错误文本没有宣称 Job Object 等价或约束主动脱离的后代。
- 对照 PRD 逐项勾选 acceptance criteria；未满足项不得归档任务。

## 回滚点

- Task 1–2 失败：停止在 schema/release gate，不进入进程操作代码。
- Task 3–5 失败：保留 Phase 2A 文件的原行为，移除未完成的 Store 接线。
- Task 6–7 发现身份无法可靠重验：维持 macOS `can_recover=false`，回到 design 重新评审，不降级为猜测或宽松杀进程。
