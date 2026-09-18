# SerenaDesktop V0.2 revision003 Implementation Task Breakdown

依据冻结方案 [`technical-design-agent-platform-v0.2.md`](./technical-design-agent-platform-v0.2.md) revision003，以及当前仓库模块边界完成实施任务拆分。

设计状态：

```text
APPROVED
DESIGN FROZEN
BINDING
revision003
```

标记说明：

- `H`：hard dependency
- `S`：soft dependency
- `E`：external evidence gate
- `A`：human acceptance gate

所有 Implementation Task 均控制在 `small` 或 `medium` blast radius；没有不可拆分的 `large` Task。

---

---

# Phase 0 — Contract Freeze / Baseline / Evidence

## P0-001 — 当前质量与测试基线

Phase: Phase 0  
Type: contract-test  
Goal: 固定当前 Rust、前端、Git whitespace 与构建结果。  
Why now: 后续回归必须与已知基线比较。  
Dependencies: None  
Blocked by: None  
Allowed scope: 只读执行 README/CI 已声明命令。  
Forbidden scope: 修复失败、格式化文件、修改依赖。  
Contract references: §4；§48；§52 Phase 0  
Implementation requirements: 记录命令、exit code、测试数、首个错误及 Git 状态。  
Non-goals: 清理历史 warning/failure。  
Tests required: `npm run lint/build`、现有前端测试、`cargo fmt/check/clippy/test`、`git diff --check`。  
Evidence required: 完整命令矩阵和基线分类。  
Acceptance criteria: 每个 Gate 都有 `PASS/FAIL/NOT_RUN` 证据。  
Rollback / failure behavior: 失败只登记 baseline，不改代码。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: P0-002～P0-009

## P0-002 — MCP Tool 与 Schema 基线

Phase: Phase 0  
Type: contract-test  
Goal: 固定当前 Tool 名称、input/output schema、description 和 contract hash。  
Why now: 后续 `workspaceId` 迁移需要可审查差异。  
Dependencies: None  
Blocked by: None  
Allowed scope: `src-tauri/src/mcp/{registry,orchestration,server}.rs` 只读；现有 contract tests。  
Forbidden scope: 修改 Tool Schema 或 deprecated surface。  
Contract references: §4.3～§4.4；§8；§10.4；§50；§52 Phase 0  
Implementation requirements: 特别记录 Source 的 `relative_path`、Git 的 `path` 及现有 Activate 契约。  
Non-goals: 提前实施 2A.2。  
Tests required: registry/schema/hash tests。  
Evidence required: Tool catalog 与 hash snapshot。  
Acceptance criteria: 所有公开 Tool 的基线字段均可追溯。  
Rollback / failure behavior: 缺少测试时登记 gap。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: P0-001、P0-003～P0-009

## P0-003 — Workspace、Config 与 State Migration 基线

Phase: Phase 0  
Type: contract-test  
Goal: 固定 `ManagerConfig.workspaces`、AppPaths、StateStore schema v1～v5 和旧 Execution 数据形态。  
Why now: 2A.1/2A.2/3/4 都会新增迁移。  
Dependencies: None  
Blocked by: None  
Allowed scope: `config.rs`、`agent/store.rs`、现有 schema/tests 只读。  
Forbidden scope: 生成 migration、重写历史数据。  
Contract references: §4.1～§4.2；§7；§10.6；§37～§38；§54  
Implementation requirements: 记录旧配置缺字段、旧项目顺序、旧 Execution workspace 字段。  
Non-goals: 实施 generation/revision。  
Tests required: config round-trip、DB open/migrate/restart 基线。  
Evidence required: fixture 和版本矩阵。  
Acceptance criteria: 升级输入与预期保留字段明确。  
Rollback / failure behavior: 无法证明的数据标 Unknown。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: 其他 P0 Evidence

## P0-004 — Serena CLI 与 Project Workflow Evidence

Phase: Phase 0  
Type: contract-test  
Goal: 固定 Serena 最低版本、`--project`、默认 project creation、Root postcondition、index/onboarding 行为。  
Why now: Serena Adapter 不得基于猜测实现。  
Dependencies: None  
Blocked by: 可用 Serena binary  
Allowed scope: Serena CLI、临时测试目录；不修改仓库。  
Forbidden scope: 写入用户真实项目、实现 Adapter。  
Contract references: §8.2；§11；§41～§42；§52 Phase 0  
Implementation requirements: 验证非交互创建、canonical Root、index/onboarding 是否独立。  
Non-goals: 多进程验证。  
Tests required: 空目录、已准备目录、路径大小写与错误版本。  
Evidence required: binary version、命令、输出和目录变化。  
Acceptance criteria: 2A.3 所需 Serena workflow 均有一手证据。  
Rollback / failure behavior: 证据不符则记录 `DESIGN_BLOCKER` 并进入 DCR。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P0-006、P0-008、P0-009

## P0-005 — Serena 多进程与 Shared SERENA_HOME Evidence

Phase: Phase 0  
Type: contract-test  
Goal: 验证不同 Workspace 独立 endpoint/process 与共享配置安全性。  
Why now: 决定 2A.3 的可用容量策略。  
DCR resolution: contract-test 已完成。Serena CLI `1.7.0` 的 A/B 独立 process/endpoint 可并发 live、各自保持 Root/marker，停止 A 不影响 B；共享 `SERENA_HOME/serena_config.yml` 时 `projects` registry 出现确定性 lost update，最终仅保留 Workspace B，Workspace A 注册丢失。Host 已批准冻结 per-slot `SERENA_HOME`；P2A3-008 按此实现，并以 `maxInstances > 1` 的容量契约运行。

Dependencies: P0-004 (H)  
Blocked by: None；contract-test evidence 已获 Host 接受，DCR 已 resolved

Allowed scope: 临时 Workspace、临时端口、临时 Serena Home。  
Forbidden scope: 在代码中实现多个 fallback。  
Contract references: §10.9；§11.3；§52 Phase 0/2A.3  
Implementation requirements: 覆盖 A/B 并发、配置互不覆盖、working set/start/stop latency。  
Non-goals: 实现 per-slot Home。  
Tests required: A/B 同时运行及同时调用。  
Evidence required: PID、endpoint、Root identity、配置 diff、资源数据。  
Acceptance criteria: contract-test evidence 已记录，DCR 已冻结 per-slot Home；P2A3-008 可据此实施。

Rollback / failure behavior: 若 per-slot Home 与冻结契约出现新的实质冲突，记录 `DESIGN_BLOCKER` 并仅阻塞 Phase 2A.3。

Risk: high  
Estimated blast radius: small  
Can run in parallel with: P0-007～P0-009

## P0-006 — CodeGraph CLI Contract Evidence

Phase: Phase 0  
Type: contract-test  
Goal: 固定 `status --json`、`init --yes`、`sync`、rebuild 和 Root identity 契约。  
Why now: 2D readiness/action 必须使用真实 CLI 证据。  
Dependencies: None  
Blocked by: 可用 CodeGraph binary  
Allowed scope: 临时目录和现有 index 的只读探测。  
Forbidden scope: 修改业务项目 index。  
Contract references: §8.2；§41～§42；§52 Phase 0/2D  
Implementation requirements: 记录 version、JSON schema、initialized/projectPath/index.state。  
Non-goals: RuntimeSlot 实现。  
Tests required: 未初始化、ready、stale/error index。  
Evidence required: 原始 JSON 与命令版本。  
Acceptance criteria: 2D 能从稳定字段判定 readiness。  
Rollback / failure behavior: 不稳定契约进入 DCR，仅阻塞 2D。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P0-004、P0-008、P0-009

## P0-007 — CodeGraph 多 Workspace/Process Evidence

Phase: Phase 0  
Type: contract-test  
Goal: 验证 A/B index/runtime 可隔离并发及容量基础数据。  
Why now: 防止将旧 global Binding 搬入新 Manager。  
Dependencies: P0-006 (H)  
Blocked by: CodeGraph environment  
Allowed scope: 两个临时 Workspace/index。  
Forbidden scope: 隐式 init、修改真实 index。  
Contract references: §10.7～§10.9；§42；§52 Phase 0/2D  
Implementation requirements: 验证不同 Root、不共享/retarget process、资源和 stop latency。  
Non-goals: 预先实现 Manager。  
Tests required: A/B 查询、错误隔离、process crash。  
Evidence required: Root/process identity 与资源记录。  
Acceptance criteria: 并发策略和首版容量常量有证据。  
Rollback / failure behavior: 未完成只阻塞 Phase 2D。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: P0-005、P0-008、P0-009

## P0-008 — Codex Usage Wire Contract Evidence

Phase: Phase 0  
Type: contract-test  
Goal: 固定 Codex binary/version/hash、token schema、ordering、terminal coverage 和 checkpoint 能力。  
Why now: Phase 4 不得猜测累计值和 complete 边界。  
Dependencies: None  
Blocked by: pinned Codex binary  
Allowed scope: Codex app-server 探针、临时执行。  
Forbidden scope: 修改 runtime、推导 `total_tokens`。  
Contract references: §31～§35；§52 Phase 0/4  
Implementation requirements: 覆盖 fresh、continue、terminal、late notification。  
Non-goals: Usage DB/Product/UI。  
Tests required: integer/null/ordering/coverage probes。  
Evidence required: binary hash、wire samples、事件顺序。  
Acceptance criteria: participating fields 与 complete 条件明确。  
Rollback / failure behavior: 不满足时进入 DCR，仅阻塞 Phase 4。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: 其他 P0 Evidence

## P0-009 — Installer、Data Path 与 Single-instance 基线

Phase: Phase 0  
Type: contract-test  
Goal: 固定 portable 当前行为、Tauri identifier、config/data path、WebView2 和 single-instance 基线。  
Why now: Phase 6 必须基于真实 Windows 行为。  
Dependencies: None  
Blocked by: Windows test host  
Allowed scope: 当前 binary/config、临时用户数据。  
Forbidden scope: 修改 Tauri bundle/release。  
Contract references: §4.11；§43～§47；§52 Phase 0/6  
Implementation requirements: 记录 installed/portable 目标路径和 autostart 当前指向。  
Non-goals: 构建 installer。  
Tests required: 同用户 path resolution、双启动、WebView2 presence。  
Evidence required: 路径、进程与注册项截图/日志。  
Acceptance criteria: Phase 6 的环境前置条件明确。  
Rollback / failure behavior: 未完成只阻塞 Phase 6。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: 其他 P0 Evidence

## P0-010 — 正式化前端测试入口

Phase: Phase 0  
Type: implementation  
Goal: 增加冻结设计规定的 `npm test` 入口。  
Why now: 后续各 Phase 需要统一前端 Gate。  
Dependencies: P0-001 (H)  
Blocked by: 历史测试必须先被识别为 baseline  
Allowed scope: `package.json`；现有 `src/*.test.mjs`。  
Forbidden scope: 修改业务代码、顺手修复无关测试。  
Contract references: §48；§52 Phase 0；§56.46  
Implementation requirements: script 等价于 `node --test src/*.test.mjs`。  
Non-goals: 修改 GitHub release workflow。  
Tests required: `npm test`。  
Evidence required: exit code、test totals。  
Acceptance criteria: 命令可重复运行并覆盖现有前端测试。  
Rollback / failure behavior: 历史失败单列 cleanup task，不混修。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: P1-001

---

---

# Phase 1 — Provider-Agnostic Agent Control Plane

## P1-001 — Provider Domain Types

Phase: Phase 1  
Type: implementation  
Goal: 增加 `ProviderId`、Descriptor、Capabilities、Context、Error、RunResult domain types。  
Why now: Registry 和 Port 的最小编译基础。  
Dependencies: P0-001 (H)  
Blocked by: None  
Allowed scope: 新 `agent/provider` domain module、`agent/mod.rs`。  
Forbidden scope: 改 Codex Runtime、Store、Execution 状态机。  
Contract references: §15～§18；§50 Provider  
Implementation requirements: Human-approved binding clarification：

- `ProviderId` 是 `String` newtype，`serde(transparent)`，JSON string；首版值为 `codex`，Core 不解析 Provider-specific 前缀/格式；validation 要求非空并拒绝任何 whitespace/control character，除此之外不增加正则、长度或字符集约束。
- `ProviderExecutionContext` 与 `ProviderCancelContext` 均为 `#[serde(rename_all = "camelCase", deny_unknown_fields)]`，且仅含 `execution_id: String`；不携带 `workspaceId`、`canonicalRoot`、thread、turn、job、`historyMode` 或 Runtime identity，Provider 通过权威 StateStore / Execution identity 读取冻结执行上下文。
- `ProviderStartupContext` 为 `deny_unknown_fields`、序列化为 `{}` 的空 struct；Provider 实例持有自己的 Store / Runtime 依赖。
- `ProviderErrorCode` 的 serde string 精确为 `AGENT_PROVIDER_NOT_FOUND`、`AGENT_PROVIDER_UNAVAILABLE`、`AGENT_PROVIDER_CAPABILITY_UNSUPPORTED`、`AGENT_PROVIDER_CONTRACT_ERROR`、`AGENT_PROVIDER_OPERATION_FAILED`；`ProviderError` 为 `#[serde(rename_all = "camelCase", deny_unknown_fields)]` 且仅含 `code: ProviderErrorCode`。
- `ProviderOutcome` 是 snake_case enum：`completed` / `failed` / `cancelled` / `interrupted`；它不授权 Workspace Claim release。
- `ProviderResultCompleteness` 是 snake_case enum：`unknown` / `partial` / `complete`，语义与现有 Execution result completeness 一致，但本 Task 不修改现有 Execution 类型。
- `ProviderRunResult` 为 `#[serde(rename_all = "camelCase", deny_unknown_fields)]`，精确字段为 `execution_id: String`、`outcome: ProviderOutcome`、`result: Option<serde_json::Value>`、`result_completeness: ProviderResultCompleteness`、`diagnostic_code: Option<String>`。
- `ProviderError` 不含 raw provider message、command/stdout/stderr/runtime/thread/turn/job；`ProviderRunResult` 不含 `safe`、`safeToReleaseWorkspace`、`releaseEvidence`、`jobEmpty`、`runtimeTerminated`、`cleanupComplete` 或 thread/turn/job/`historyMode`/Runtime identity。finalization 必须重新读取权威 StateStore / Runtime evidence。

Non-goals: Registry 或 routing。  
Tests required: `ProviderId` validation/JSON string；全部 Context/Error/Outcome/Completeness/RunResult 精确 serialization；unknown-field 与 forged safety/identity field rejection。
Evidence required: focused Rust tests。  
Acceptance criteria: 上述字段类型、枚举 wire value、camelCase/snake_case/SCREAMING_SNAKE_CASE 与 `deny_unknown_fields` 全部由 focused tests 固定；domain 不暴露第二套 Workspace/Execution Authority，`ProviderRunResult` 不能授权 Claim Release；P1-001 不再存在字段或 serialization DESIGN_BLOCKER。
Rollback / failure behavior: 删除新增纯 domain module 即可。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: None

---

## P1-002 — Object-safe AgentProvider Port

Phase: Phase 1  
Type: implementation  
Goal: 定义 `Arc<dyn AgentProvider>` 可用的 object-safe Port。  
Why now: ProviderRegistry 的唯一执行边界。  
Dependencies: P1-001 (H)  
Blocked by: None  
Allowed scope: `agent/provider` trait 模块。  
Forbidden scope: 引入 `async-trait`、改变 Runtime ownership。  
Contract references: §17；§19  
Implementation requirements: boxed future；execute/cancel/startup_reconcile 契约完整。  
Non-goals: Codex adapter。  
Tests required: object-safety compile test、fake provider contract test；`ProviderStartupContext` 继续固定为空 `{}` 且无 result sink / callback；fake `startup_reconcile` 返回包含 items 的 typed `ProviderReconcileSummary`，不得再用零尺寸 / ZST 断言机械锁死 summary。
Evidence required: compile/test result。  
Acceptance criteria: fake Provider 可作为 `Arc<dyn AgentProvider>` 调用。  
Rollback / failure behavior: 无注册调用前可独立回滚。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P1-006 的事件 domain 草案

## P1-003 — ProviderRegistry

Phase: Phase 1  
Type: implementation  
Goal: 实现唯一 ID 注册、查询、descriptor/capability/health 列表。  
Why now: AgentTaskManager 不应直接构造 Codex。  
Dependencies: P1-002 (H)  
Blocked by: None  
Allowed scope: `agent/provider/registry.rs` 及单元测试。  
Forbidden scope: WorkspaceCapabilityRegistry、动态 Plugin ABI。  
Contract references: §22；§50 Provider  
Implementation requirements: duplicate ID fail startup；unknown 返回稳定错误。  
Non-goals: Provider runtime 生命周期。  
Tests required: register/get/list/duplicate/unknown/unavailable。  
Evidence required: registry tests。  
Acceptance criteria: 首版仅注册 `codex`，无字符串分支散落。  
Rollback / failure behavior: Registry 创建失败时不启动 Agent Control Plane。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P1-006

## P1-004 — Codex Registration Adapter

Phase: Phase 1  
Type: implementation  
Goal: 用薄 Adapter 将现有 `CodexProvider` 注册到通用 Port。  
Why now: 验证 Port 能包裹现有 Runtime 而不重写它。  
Dependencies: P1-003 (H)  
Blocked by: None  
Allowed scope: `agent/codex/provider.rs`、provider registration bootstrap。  
Forbidden scope: 修改 `CodexRuntimePool`、Job、terminal evidence。  
Contract references: §16～§19；§22  
Implementation requirements: descriptor/capabilities/health 从 Codex adapter 提供。  
Non-goals: Usage/Activity 新行为。  
Tests required: Codex registration、CLI missing/unavailable。  
Evidence required: focused provider tests。  
Acceptance criteria: 现有 Codex execute/cancel 行为可经 trait 调用。  
Rollback / failure behavior: 恢复直接构造路径但不改 Runtime 数据。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: P1-006

## P1-005 — AgentTaskManager 经 Registry 路由

> Human-approved binding clarification / DCR：这是对 revision003 Design Freeze 的最小补充，仅冻结 P1-005 的 Provider pre-turn acceptance 握手与机械回归，不进入 P1-006 / P1-007，也不修改 P1-001 已冻结的 `ProviderExecutionContext` 字段。

> Human-approved resolver clarification / DCR（第二个最小 DCR）：仅补充 `get_registered()` 与 Provider unavailable 时的 cancel 路由回归；原 ProviderAcceptanceSink DCR 及其余 P1-005 冻结语义不变。

> Human-approved execution-failure clarification / DCR-3（第三个最小 DCR）：`AgentProvider::execute` 改用 Provider 层内部 `ProviderExecutionFailure`，解除 AgentTaskManager 对 Codex 私有 `ExecutionFailure` 的依赖。只解决前一轮 DESIGN_BLOCKER；不扩展 `ProviderError` wire contract，不建立通用错误框架，也不进入 P1-006 / P1-007 / P1-008。

Phase: Phase 1  
Type: implementation  
Goal: execute/cancel 按 ProviderId 经 Registry 调用。  
Why now: 完成 Control Plane 的执行解耦。  
Dependencies: P1-004 (H)  
Blocked by: None  
Allowed scope: `agent/provider/port.rs`、`agent/provider/registry.rs`、`agent/provider/registry/tests.rs`、`agent/codex/provider.rs`、`agent/task_manager.rs` 与相关 provider / product adapter tests。
Forbidden scope: Claim、requestKey、cancel/finalize/recovery 语义变化；Store schema / lifecycle、Runtime、Job、telemetry 修改。
Contract references: §15；§17～§20；§22；§52 Phase 1  
Implementation requirements: 当前默认仍是 Codex；`execute(context, acceptance, telemetry)` 保持 object-safe / boxed future / no async-trait，并返回 `Result<ProviderRunResult, ProviderExecutionFailure>`。`ProviderExecutionFailure` 仅为 Provider 层内部类型，不实现 serde，不是 wire / Product DTO，也不进入 MCP schema；最小 variant 为 `State(String)` 与 `Runtime { code, message }`。`State(String)` 保留现有执行状态/安全诊断字符串语义；`Runtime` 只携带安全诊断 code + message，禁止 Runtime handle/id、thread、turn、job、`historyMode`、Claim/evidence。既有 `ProviderError` 继续仅含 `{ code }`，五个稳定码不变且不扩 payload；cancel、startup_reconcile 与 Registry 仍使用 `ProviderError`。Codex Adapter 将 `codex::provider::ExecutionFailure::State(value)` 映射为 `ProviderExecutionFailure::State(value)`，将 `ExecutionFailure::Runtime(failure)` 映射为 `ProviderExecutionFailure::Runtime { code: failure.code, message: failure.message }`；`failure` 中的真实 Runtime owner/identity 与 quarantine ownership 留在 `CodexRuntimePool` / Adapter 内，跨 Port 只传 code/message。AgentTaskManager 只消费 `ProviderExecutionFailure`，不依赖 Codex 私有 `ExecutionFailure`，不解释 Runtime/thread/turn/`historyMode`。Host 提供 one-shot / at-most-once `ProviderAcceptanceSink`；它无 payload、无 rejected/error 方法，drop 不改变 lifecycle。Start 在 availability/quarantine/pre-dispatch checks 后、`turn/start` 前 acceptance；Continue 仅在 managed Thread resume、exact identity、`historyMode=Paginated` 与 Execution bind 成功后、`turn/start` 前 acceptance。acceptance 前的 `Err` 投影为 rejection；无 acceptance 的异常结束或 terminal success 不得虚构 acceptance，走现有稳定错误 / contract error 路径。`accepted()` 不写 DB，不改变 dispatch/status；Store `dispatch_state` 不能等价替代 acceptance。它不授权 Claim release，不代表 providerInvoked/dispatched/terminal，不进入 Activity Revision；公共层不解释 Codex 私有 Thread / Turn / `historyMode` / Runtime。Provider completed 不能直接 release Claim，finalize 仍重读权威 evidence。Registry 保留 `get(&ProviderId)` 的 health-gated 语义供 execute / continue 使用，unknown 返回 `AGENT_PROVIDER_NOT_FOUND`，`Unavailable` 返回 `AGENT_PROVIDER_UNAVAILABLE`；新增 `get_registered(&ProviderId)`，只检查注册存在性，unknown 返回 `AGENT_PROVIDER_NOT_FOUND`，已注册时即使 `Unavailable` 也返回 Provider handle 且不改变或伪造 health。cancel 从 persisted `Execution.provider` 构造 `ProviderId`，经 `get_registered()` 后检查 `capabilities().can_cancel`；`false` 返回 `AGENT_PROVIDER_CAPABILITY_UNSUPPORTED`，`true` 调用 `provider.cancel()`，Provider / CLI health 不作前置门禁。该区分不是通用 bypass，不新增状态、错误码、Store 字段、fallback、Codex 特判或 Plugin ABI。上述失败边界调整必须保持 `AGENT_RUNTIME_QUARANTINED` Product 投影、Recovery Runtime/State failure 分类，以及 Execution/Claim pending/Unknown/finalize authority 全部不变。
Non-goals: 多 Provider UI。  
Tests required: provider / control tests 继续验证 `ProviderError` `{ code }` wire 不变，并验证 `ProviderExecutionFailure` 不可 serde / wire；Recovery `explicit_resume_binary_failure_keeps_pending_and_claim` 恢复通过并保持 Runtime failure 分类；Runtime quarantine Product regression 恢复 `AGENT_RUNTIME_QUARANTINED`；Start acceptance 时点；Continue acceptance 在 bind 后、`turn/start` 前；wrong identity / legacy / missing history 不 acceptance 且不 `turn/start`；Provider terminal 不可替代 acceptance；`get_registered()` unknown 返回 not found、registered unavailable 返回原 Provider handle 且 health 保持 unavailable；persisted Provider unavailable 时 cancel 仍经 capability gate 调用 `provider.cancel()`，同时 execute / continue 仍由 `get()` 拒绝；现有 P1-005 routing / acceptance / cancel tests 全部继续通过；Runtime/Recovery/Cancel/requestKey/Claim/atomic release 全套回归。
Evidence required: focused + existing safety test results。  
Acceptance criteria: `AgentProvider::execute` 只经 Provider 层内部 `ProviderExecutionFailure` 暴露 State/Runtime 安全诊断，AgentTaskManager 不再依赖 Codex 私有失败类型；`ProviderError` `{ code }` wire、`AGENT_RUNTIME_QUARANTINED` Product 投影、Recovery Runtime/State 分类与 Execution/Claim pending/Unknown/finalize authority 均无变化，前一轮 DESIGN_BLOCKER 关闭。routing 变化后所有 Runtime Safety tests 无回归；Provider 已注册但 unavailable 时，cancel 仍按 persisted ProviderId 路由并保留 capability / manual-resolution safety，execute / continue 不可启动或继续 Provider work。
Rollback / failure behavior: 数据格式不变，可恢复旧 route。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P1-006、P1-007

## P1-006 — Provider Startup Reconcile Port

> Human-approved startup-reconcile clarification / DCR-4：`ProviderStartupContext` 保持空 `{}`；`ProviderReconcileSummary` 改为 Provider 层内部 typed result，并冻结 registration-only startup routing、Codex mapping、失败隔离与最小 health update。该 DCR 不改变 P1-005，也不进入 P1-007 / P1-008。

Phase: Phase 1  
Type: implementation  
Goal: Desktop startup 经 Registry 调用各 Provider 的 reconcile。  
Why now: Recovery ownership 不能留在通用层的 Codex 特判。  
Dependencies: P1-004 (H)  
Blocked by: None  
Allowed scope: `agent/provider/port.rs` + port tests（仅 typed summary contract 与移除旧 ZST assertion）、`agent/provider/registry.rs` + tests（Registry 仅允许增加一个内部 health update API，如 `set_health` 或等价命名，且必须校验 Provider 已注册）、`agent/product.rs` + directly related startup / recovery tests（仅 `initialize` / `recover_before_publish` 所需的最小 startup initialization / report plumbing 与类型投影）、现有 `lib.rs` Agent startup、`task_manager.rs`、Codex recovery adapter。
Forbidden scope: 修改 Recovery identity、Unknown fail-closed、Runtime ownership / quarantine、Claim release / finalize、startup-before-publication 顺序；修改 `ProviderError`、`ProviderExecutionFailure`、P1-005 acceptance / `get()` / `get_registered()` / cancel 语义；修改 Store / Runtime / Job；改变 Product DTO / MCP schema 或重设计 public Product behavior，`agent/product.rs` 权限不超出 startup initialization / report plumbing；新增错误码、状态机、插件框架、自动重试、health reason / time metadata；进入 P1-007 / P1-008。
Contract references: §5.5～§5.8；§17；§19～§20；§22
Implementation requirements: `ProviderStartupContext` 继续保持序列化为 `{}` 的空 struct，不引入 result sink / callback。`ProviderReconcileSummary { items: Vec<ProviderReconcileItem> }`、`ProviderReconcileItem { subject_id: String, kind: ProviderReconcileKind }` 与八个冻结 kind 是 Provider 层内部、provider-agnostic typed startup report；不做 serde，不是 Product / MCP / wire DTO。`subject_id` 是只供 Host startup report / logging 使用的 opaque subject identity，Core 不解析格式 / 前缀；summary 不得携带 Runtime handle / id、thread、turn、job、`historyMode`、Claim、release / termination evidence、raw provider error、`ExecutionRecord` 或 Provider private object。Codex 可继续内部产生现有 `Vec<RecoveryOutcome>`，在 Adapter 边界按原 report 顺序逐项映射为 `OrphanResourceRecovered`、`OrphanResourceUnknown`、`ExecutionReleased`、`ExecutionInconsistent`、`ExecutionPendingExplicitResume`、`ExecutionUnknown`、`ExecutionProviderFailure`、`ExecutionInterrupted`；私有 evidence / Runtime owner 留在 Codex recovery / `CodexRuntimePool`，不跨 Port。startup routing 必须枚举已注册 Provider，经 registration-only `get_registered()` 取得 Provider，不得用 health-gated `get()` 跳过历史安全恢复；只有 `capabilities().can_recover == true` 才执行 `startup_reconcile`，Codex 完成 P1-006 后设为 `true`。Host 复用现有 startup logging / reporting，仅匹配 kind + `subject_id`，不解释 Codex 私有 identity / evidence。单个 Provider 返回 `ProviderError` 时，其 durable Claim / Unknown 保持 fail-closed，不释放未证明安全的资源；Registry 将该已注册 Provider health 更新为 `Unavailable`，后续 execute / continue 仍被 `get()` 拒绝；其他已注册且可恢复 Provider 继续 reconcile，不回滚。unknown health update 继续返回 `AGENT_PROVIDER_NOT_FOUND`。
Non-goals: 并行重写 recovery；通用插件 / 重试 / health metadata 框架。
Tests required: typed summary 全八类 mapping；与原 `RecoveryOutcome` report 逐项顺序等价；summary 类型无 private identity / evidence fields 且不可 serde / wire；Provider 已注册但 unavailable 时仍执行 recovery；单 Provider reconcile failure 会 mark unavailable、保持 Claim / Unknown fail-closed、拒绝其后续 execute / continue，并允许其他 `can_recover` Provider 继续；unknown health update 返回 `AGENT_PROVIDER_NOT_FOUND`；既有 restart、unknown issuer、orphan claim、termination evidence、P1-005 routing / acceptance / cancel 回归不变。
Evidence required: recovery regression matrix。  
Acceptance criteria: Registry reconcile 与旧恢复结果 kind / subject / 顺序逐项一致；公共层只消费 `ProviderReconcileKind` + opaque `subject_id`，不获得 Codex private identity / evidence；unavailable Provider 不会被 recovery 跳过；单 Provider failure fail-closed、标记 unavailable 且不阻止其他可恢复 Provider；P1-005 与既有 Recovery / Runtime / Claim / startup 顺序语义无变化。
Rollback / failure behavior: reconcile error 隔离为对应 Provider unavailable，未证明安全的 durable Claim / Unknown 保持不释放，其他可恢复 Provider 继续且不回滚。
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P1-005、P1-007

## P1-007 — AgentEventSink 与 TelemetryProjector Boundary

Phase: Phase 1  
Type: implementation  
Goal: 建立闭集 Activity/Usage telemetry boundary。  
Why now: 后续 Phase 3/4 必须共用 Provider-agnostic Sink。  
Dependencies: P1-001、P1-004 (H)  
Blocked by: None  
Allowed scope: `agent/provider` telemetry、Codex notification adapter、store projection interface。  
Forbidden scope: 通过 telemetry 传 terminal/runtime/claim evidence。  
Contract references: §23～§23.1；§29；§30  
Implementation requirements: Codex Adapter publish 前验证 runtime/thread/turn/execution identity。  
Non-goals: Phase 3/4 persistence semantics。  
Tests required: identity mismatch drop、forged terminal event reject、Activity/Usage allowlist。  
Evidence required: focused telemetry tests。  
Acceptance criteria: Projector 无 thread/turn 类型依赖。  
Rollback / failure behavior: telemetry 可禁用但不得影响 lifecycle。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P1-005、P1-006

## P1-008 — Provider Product Projection 与 Architecture Gates

> Bounded implementation clarification（P1-008B）：本单元先建立 Provider-owned、非 serde/wire 的 `validate_continuation(ProviderContinuationContext { source_execution_id }) -> ProviderContinuationDecision`。Codex Adapter 仅在自身 StateStore 内解释 managed provenance；Port 不携带或返回 thread/turn/runtime/job/historyMode/Claim/evidence/finalResult。它不接入 Product、Store 或 TaskManager Continue routing，不改变 `ProviderRunResult`、Claim authority 或状态机。当前 `continuation_eligible()` 保持原样；P1-008C 才执行 route cutover 并删除重复的 Codex 私有判断。

> Bounded implementation clarification（P1-008C1）：Continue 先在 Store 按既有 canonical request/request hash 与 Work retry context 解析 exact retry；仅无 prior 时才由 TaskManager 对 Provider-agnostic lifecycle candidate 使用 registration-only Provider lookup、capability 与 `validate_continuation`。创建事务复核 retry、Work、core lifecycle、Claim 与 source revision。Product `canContinue` 先作 core/claim/agent cheap filter，再安全投影只读 Provider validation。`CreateExecutionInput.thread_id`、`execution-request-v1` request hash 与 child thread copy 本单元保持不变；其私有 identity/request-hash 依赖仍由 P1-008C2 处理，P1-008 Gate 尚未闭合。

> Bounded implementation clarification（P1-008C2A）：`execution-request-v1` 的 continuation request identity 改为 provider-agnostic `parent_execution_id`（source Execution ID）；fresh 仍为 null，固定 bytes/hash 不变。`thread_id` 继续作为临时 persistence/runtime compatibility input，但不参与当前 canonical hash；新 child 写入 generic parent，同时保留 child thread copy。仅 `parent_execution_id IS NULL` 的 pre-C2 persisted row 可由 Store 内封闭的 exact legacy hash 比对重试；不 backfill 或推测 parent。Codex runtime child thread copy 的变更仍是 C2B blocker，本说明不宣告 P1-008 Gate PASS。

> Bounded implementation clarification（P1-008C2B）：current continuation child 只持久化 generic `parent_execution_id`，不再复制 Provider thread。parent/source 是 runtime continuation authority；Codex Adapter 在私有边界按受管 provenance 解析 source thread，并在 bind 后才开始 turn。`parent_execution_id IS NULL` 且 child 已有 thread 的 fallback 只保留给旧持久化行；不 backfill、不扩散 Provider-private identity，也不宣告 P1-008 Gate PASS。

Phase: Phase 1  
Type: contract-test  
Goal: 增加 Provider DTO/error projector及机械依赖 Gate。  
Why now: 封闭兼容字段出口并完成 Phase 1 Gate。  
Dependencies: P1-005～P1-007、P0-010 (H)  
Blocked by: None  
Allowed scope: `agent/product.rs`、product tests、CI/test scripts。  
Forbidden scope: UI redesign、删除 threadId/threadName/turnId。  
Contract references: §21～§23；§39；§50 Provider；§52 Phase 1  
Implementation requirements: compatibility projector 是唯一 opaque allowlist；revision hash 排除 provider-private identity。  
Non-goals: Usage/Activity UI。  
Tests required: AST/rg forbidden identifier、error mapping、forged safe/evidence。  
Evidence required: Phase 1 complete test matrix。  
Acceptance criteria: Control/Product 不读取 thread/turn/job/historyMode 做控制决策。  
Rollback / failure behavior: Gate 失败阻止 Phase 1 完成。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

---

# Phase 2A.1 — Workspace Registry Authority / Project CRUD

## P2A1-001 — Registry Revision 与 Workspace Generation Migration

Phase: Phase 2A.1  
Type: migration  
Goal: 为配置增加 `workspace_registry_revision` 和 per-Workspace `generation`。  
Why now: Resolver 与 Remove/Root authority 需要版本身份。  
Dependencies: P1-008 (H)  
Blocked by: P0-003 baseline  
Allowed scope: `config.rs`、`types.ts`、config fixtures/tests。  
Forbidden scope: 重生成 ID、重新 canonicalize 旧 Root。  
Contract references: §7.3；§10.2；§54.1  
Implementation requirements: 缺字段默认 1；旧顺序/ID/name/root 原样保留。  
Non-goals: CRUD。  
Tests required: old config migrate、round-trip、restart。  
Evidence required: before/after fixture。  
Acceptance criteria: migration 不改变现有 Workspace identity。  
Rollback / failure behavior: 新字段可忽略，旧 Registry 仍可读取。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P2A1-002

## P2A1-002 — Canonical Root Identity Helper

Phase: Phase 2A.1  
Type: implementation  
Goal: 实现一致的 Windows canonical Root identity 比较。  
Why now: Register 去重与后续 Lease 安全依赖它。  
Dependencies: P1-008 (H)  
Blocked by: None  
Allowed scope: workspace/config domain 新 helper 与 tests。  
Forbidden scope: 路径 target 解析、Source IO。  
Contract references: §7.4～§7.6；§51.1  
Implementation requirements: Windows case-insensitive identity；拒绝非法/不存在 Root。  
Non-goals: `WorkspacePathResolver`。  
Tests required: casing、separator、same-root aliases、non-Git directory。  
Evidence required: helper unit tests。  
Acceptance criteria: 重复 canonical Root 被稳定识别。  
Rollback / failure behavior: canonicalization failure 返回稳定注册错误。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P2A1-001

## P2A1-003 — Workspace Registry Service 与 Read API

Phase: Phase 2A.1  
Type: implementation  
Goal: 建立由 `ManagerConfig.workspaces` 支撑的串行 Registry service。  
Why now: CRUD 不能继续散落在 Serena sync 路径。  
Dependencies: P2A1-001、P2A1-002 (H)  
Blocked by: None  
Allowed scope: `config.rs`、`commands.rs`、workspace service module。  
Forbidden scope: MCP request authority、Provider readiness。  
Contract references: §7.1～§7.3；§8；§52 Phase 2A.1  
Implementation requirements: 复用 `SupervisorState.operation` 和 atomic persist。  
Non-goals: Remote discovery schema。  
Tests required: list/get、revision、atomic persist、concurrent readers。  
Evidence required: service tests。  
Acceptance criteria: Registry read 不访问 Serena registry。  
Rollback / failure behavior: 写失败保留旧 config。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: None

## P2A1-004 — workspace_inspect_directory

Phase: Phase 2A.1  
Type: implementation  
Goal: 实现只读目录检查和基础 metadata 返回。  
Why now: Register 前需要无副作用验证。  
Dependencies: P2A1-002 (H)  
Blocked by: None  
Allowed scope: workspace inspection module、Tauri command、tests。  
Forbidden scope: 注册、Serena prepare、Git/CodeGraph init。  
Contract references: §8.1～§8.2；§51.1  
Implementation requirements: 只校验 Root/basic metadata；非 Git 合法。  
Non-goals: capability readiness 作为注册前置。  
Tests required: directory/file/missing/access denied/non-Git。  
Evidence required: command contract tests。  
Acceptance criteria: inspect 不修改目录或配置。  
Rollback / failure behavior: 检查失败仅返回错误。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: P2A1-003、P2A1-005

## P2A1-005 — Native Directory Picker

Phase: Phase 2A.1  
Type: UI  
Goal: 提供 Local Human 的原生目录选择入口。  
Why now: Project Register 的用户授权来源。  
Dependencies: P1-008 (H)  
Blocked by: None  
Allowed scope: Tauri dialog command、`api.ts`、Project UI 的 picker trigger。  
Forbidden scope: 自动 Register、远程路径输入、Provider prepare。  
Contract references: §8.1；§51.1  
Implementation requirements: cancel 为正常结果；只返回本地选择结果。  
Non-goals: 项目管理完整 UI。  
Tests required: IPC contract、cancel、single directory。  
Evidence required: frontend/command tests。  
Acceptance criteria: 用户显式选择后才产生候选 Root。  
Rollback / failure behavior: picker 不可用时不改变 Registry。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: P2A1-004

## P2A1-006 — workspace_register

Phase: Phase 2A.1  
Type: implementation  
Goal: 原子登记新 Workspace。  
Why now: Registry Authority 的首个写入口。  
Dependencies: P2A1-003、P2A1-004 (H)  
Blocked by: None  
Allowed scope: Registry service、Tauri API、focused tests。  
Forbidden scope: 自动启动 Serena/CodeGraph、要求 Git。  
Contract references: §8.2～§8.3；§50 Workspace  
Implementation requirements: 分配稳定 ID/generation；立即可见；异步 readiness 不阻塞。  
Non-goals: Capability observation 实现。  
Tests required: success、duplicate canonical root、invalid name、non-Git。  
Evidence required: persisted config diff 与 tests。  
Acceptance criteria: Register 成功不产生 Provider process。  
Rollback / failure behavior: persist 失败不发布新 Registry。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: P2A1-007

## P2A1-007 — workspace_rename 与 workspace_reorder

Phase: Phase 2A.1  
Type: implementation  
Goal: 实现纯 Registry metadata/order mutation。  
Why now: 两者不改变 Root authority，可共用同一串行配置写边界。  
Dependencies: P2A1-003 (H)  
Blocked by: None  
Allowed scope: Registry service、Tauri API、tests。  
Forbidden scope: 改 ID/root/generation、启动 Provider。  
Contract references: §8.4；§8.7；§10.2  
Implementation requirements: rename 保持 ID/root/generation；reorder 保持条目集合。  
Non-goals: UI drag/drop。  
Tests required: duplicate name、unknown ID、reorder validation、restart。  
Evidence required: config before/after。  
Acceptance criteria: 仅对应 metadata/revision 变化。  
Rollback / failure behavior: 原子写失败保留原顺序和名称。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: P2A1-006、P2A1-008

## P2A1-008 — workspace_remove 基础事务

Phase: Phase 2A.1  
Type: implementation  
Goal: 删除 Registry entry，绝不删除磁盘，并检查现有 Agent Claim。  
Why now: Project CRUD Gate 必需；后续 Phase 再挂接 Write/Capability guard。  
Dependencies: P2A1-003 (H)  
Blocked by: None  
Allowed scope: Registry remove、现有 workspace claim query、tests。  
Forbidden scope: 删除目录/index/config、设计新 lock manager。  
Contract references: §8.5～§8.6；§50 Workspace；§55 Workspace  
Implementation requirements: running execution 返回 `WORKSPACE_IN_USE`。  
Non-goals: 尚未存在的 WriteGuard/RuntimeSlot 协调。  
Tests required: idle remove、claimed remove、missing ID、disk preservation。  
Evidence required: filesystem/config assertions。  
Acceptance criteria: Registry 删除与磁盘内容完全分离。  
Rollback / failure behavior: 任何 guard/stop/persist 失败都保留 entry。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A1-006、P2A1-007

## P2A1-009 — DesktopSelectedWorkspace

Phase: Phase 2A.1  
Type: implementation  
Goal: 建立仅供 UI 和新任务默认值的选择状态。  
Why now: 替代旧 ActiveWorkspace 的 UI 含义。  
Dependencies: P2A1-003 (H)  
Blocked by: None  
Allowed scope: App state、Tauri command、frontend controller/types。  
Forbidden scope: Source/Git/MCP/Capability routing fallback。  
Contract references: §8.9；§10.1；§54.1  
Implementation requirements: remove selected entry 时清空；restore 不启动 Provider。  
Non-goals: Workspace execution authority。  
Tests required: select/restore/remove clear/no runtime side effect。  
Evidence required: state and frontend tests。  
Acceptance criteria: selection 改变不改变任何运行请求 Workspace。  
Rollback / failure behavior: 无效保存值清空 UI selection。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: P2A1-006～P2A1-008

## P2A1-010 — 停止 Startup Serena Registry Sync

Phase: Phase 2A.1  
Type: implementation  
Goal: 从 startup 移除 Serena Registry 覆盖 `ManagerConfig.workspaces`。  
Why now: 2A.1 起 ManagerConfig 已是唯一 Authority。  
Dependencies: P2A1-003 (H)  
Blocked by: None  
Allowed scope: `lib.rs` startup、`commands.rs::sync_workspaces` 调用点、tests。  
Forbidden scope: 删除显式 Import、删除 Serena compatibility fields。  
Contract references: §9.1；§52 Phase 2A.1；§54.1；§55 Workspace  
Implementation requirements: 升级后的旧条目原样保留。  
Non-goals: 删除所有旧 sync 代码。  
Tests required: startup/restart 不覆盖 config。  
Evidence required: startup integration test。  
Acceptance criteria: Phase 2A.1 完成后无 startup Serena Sync。  
Rollback / failure behavior: 后续 Phase 回滚也不得恢复 startup sync。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A1-011

## P2A1-011 — Serena Additive Import

Phase: Phase 2A.1  
Type: implementation  
Goal: 将旧 Serena 项目作为显式、additive、idempotent Import。  
Why now: 保留迁移便利但不恢复 Authority。  
Dependencies: P2A1-002、P2A1-003 (H)  
Blocked by: P0-004 evidence (S)  
Allowed scope: `mcp/projects.rs` 的读取逻辑、Local command、tests。  
Forbidden scope: startup 自动调用、覆盖/删除/重排现有 Registry。  
Contract references: §9.2；§54.1～§54.2  
Implementation requirements: canonical root 去重；已有条目保持 ID/name/order。  
Non-goals: Provider prepare。  
Tests required: additive、repeat import、conflict、missing Serena registry。  
Evidence required: import diff/tests。  
Acceptance criteria: 重复 Import 为 no-op。  
Rollback / failure behavior: Import 失败不改变 Registry。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: P2A1-010

## P2A1-012 — Project Management UI

Phase: Phase 2A.1  
Type: UI  
Goal: 接入 picker、inspect、register、rename、remove、reorder、selection。  
Why now: 后端 CRUD 稳定后提供 Local Human Authority UI。  
Dependencies: P2A1-005～P2A1-009 (H)；P2A1-011 (S，仅在显示“从 Serena 导入”入口时)  
Blocked by: None  
Allowed scope: `ProjectPanel.tsx`、`api.ts`、`types.ts`、page-scoped styles/tests。  
Forbidden scope: Capability Health UI、Agent UI、Runtime contract。  
Contract references: §8；§10.1；§52 Phase 2A.1  
Implementation requirements: 操作反馈独立；非 Git 可登记；Remove 文案明确不删磁盘；Import 未交付时基本 Project Management 仍完整可用。  
Non-goals: Provider readiness/action 展示。  
Tests required: CRUD flows、cancel picker、selected removal、drag reorder。  
Evidence required: frontend tests/screenshots。  
Acceptance criteria: 所有 Project CRUD 可从 UI 完成并即时反映。  
Rollback / failure behavior: 单次失败保留服务器权威快照。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: None

## P2A1-013 — Registry Core Migration/Restart/Concurrency Gate

Phase: Phase 2A.1  
Type: integration-test  
Goal: 关闭 Phase 2A.1 Registry 主路径 Gate；显式 Import 若提供则另验冻结的 additive/idempotent 条目。  
Why now: Authority 切换必须有升级与并发证据。  
Dependencies: P2A1-001～P2A1-010、P2A1-012 (H)；P2A1-011 (S，仅对 Import 专项验收)  
Blocked by: None  
Allowed scope: config/command/project integration tests。  
Forbidden scope: 修复 2A.2 routing。  
Contract references: §52 Phase 2A.1；§53 Workspace；§54.1；§56.3～§9  
Implementation requirements: 覆盖旧项目、startup、并发 CRUD、restart；显式 Import 若提供，须另验 additive/idempotent，未提供时不得阻塞基本 Registry Gate。  
Non-goals: WorkspaceLease。  
Tests required: Phase 2A.1 Registry Gate 全矩阵；若 Import 已提供，追加重复 Import/additive 测试。  
Evidence required: test totals 与 config fixtures。  
Acceptance criteria: Gate 条目全部 PASS。  
Rollback / failure behavior: 任一失败阻止 2A.2。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

---

---

# Phase 2A.2 — Workspace Resolver / Request Authority / Execution Freeze

## P2A2-001 — WorkspaceLease 与 WorkspaceResolver

Phase: Phase 2A.2  
Type: implementation  
Goal: 实现 `workspaceId → WorkspaceLease`。  
Why now: 所有 request authority 的可信入口。  
Dependencies: P2A1-013 (H)  
Blocked by: None  
Allowed scope: workspace domain/resolver、Registry read interface、tests。  
Forbidden scope: caller absolute root、Desktop/session fallback。  
Contract references: §10.2～§10.3；§50 Workspace  
Implementation requirements: Lease 含 id/canonicalRoot/generation；稳定错误映射。  
Non-goals: path target resolution。  
Tests required: found/missing/blank/wrong type/missing root。  
Evidence required: resolver unit tests。  
Acceptance criteria: Resolver 不读取 ActiveWorkspace 或 selection。  
Rollback / failure behavior: resolve 失败不调用下游。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A2-006

## P2A2-002 — WorkspacePathResolver

Phase: Phase 2A.2  
Type: implementation  
Goal: 将 tool-specific relative path 安全映射为 canonical target。  
Why now: Source/Git/未来 Tool 的共同路径边界。  
Dependencies: P2A2-001 (H)  
Blocked by: None  
Allowed scope: workspace path module、Windows path tests。  
Forbidden scope: 公共字段 rename、文件业务操作。  
Contract references: §10.4；§12.2；§51.2～§51.3  
Implementation requirements: 拒绝 absolute/UNC/root/escape；验证 junction/reparse boundary。  
Non-goals: 统一 `relative_path` 与 `path` 名称。  
Tests required: Windows/Unix absolute、UNC、`..`、junction、valid relative。  
Evidence required: focused path tests。  
Acceptance criteria: absolute target 只能从 Lease 派生。  
Rollback / failure behavior: 验证不确定时 fail closed。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A2-003、P2A2-006

## P2A2-003 — Workspace Discovery API

Phase: Phase 2A.2  
Type: implementation  
Goal: 使 `workspace_list`/`workspace_get` 成为纯只读 Discovery。  
Why now: ChatGPT 必须能发现 workspaceId。  
Dependencies: P2A2-001 (H)  
Blocked by: None  
Allowed scope: MCP workspace query handlers/schema/tests。  
Forbidden scope: Activate、Binding、Runtime warm。  
Contract references: §8 Workspace Discovery；§10.4～§10.5  
Implementation requirements: 先返回 Registry catalog（`workspaceId`、`name`、`generation`、Root 展示状态）；Capability Health/Readiness/Stage/Action 在 P2A3-013 后作为 enrichment，不成为 Discovery 前置依赖；不产生上下文。  
Non-goals: 每次 Tool 前自动 query；在 2A.2 构建 Capability Health。  
Tests required: list/get/no mutation/no process/no binding。  
Evidence required: MCP contract tests。  
Acceptance criteria: 已知 ID 后后续请求无需重复 list。  
Rollback / failure behavior: Query 失败不改变 execution authority。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P2A2-002、P2A2-006

## P2A2-004 — Workspace-scoped MCP Schema Foundation

Phase: Phase 2A.2
Type: implementation
Goal: 建立可复用的 `workspaceId` 参数/DTO/Schema、Resolver 接线与 Workspace provenance Schema 基础；不向仍依赖 Global ActiveWorkspace 的 Tool 发布新的 `workspaceId` Schema。
Why now: 固定公共请求级 Authority 的共同边界，供后续每个 Tool family 在安全路由同一原子变更中采用。
Dependencies: P2A2-001、P0-002 (H)  
Blocked by: None  
Allowed scope: 共享 MCP 参数/DTO/Schema helper、`WorkspaceResolver` 接线、Workspace provenance Schema、公共 contract helper/tests。
Forbidden scope: Source/Git/CodeGraph/Serena semantic/Work 或 agent start 的公开 Schema 或 handler 路由迁移；Execution Workspace snapshot；path 字段 rename；query/cancel/continue 新增 `workspaceId`。
Contract references: §10.4；§10.6；§50；§56.58～§67  
Implementation requirements: missing→`WORKSPACE_CONTEXT_REQUIRED`；blank/type→`INVALID_PARAMS`；unknown→`WORKSPACE_NOT_FOUND`。  
Non-goals: 任何 handler backend cutover；P2A2-007 Work / agent start Schema+Resolver+Execution Workspace snapshot 原子迁移；P2A2-009 Git Schema+Lease-rooted route；P2A2-010 CodeGraph unavailable/停止 advertise；P2A3-010 Serena Semantic route；P2A3-011 Source compatibility facade。
Tests required: 共享 missing/malformed/unknown 边界、Resolver 接线、provenance Schema 与未准备 Tool 不新增公开 Schema 的 contract tests。
Evidence required: schema diff/hash。  
Acceptance criteria: 共享 Foundation 可复用且已验证；本任务不让任何仍缺少同变更 Lease route 的 Tool family 对外生效 `workspaceId` Schema；不存在 Global ActiveWorkspace、DesktopSelectedWorkspace、session/last-request 或“仅校验 ID、不由 ID 决定 Root”的中间回退。
Rollback / failure behavior: 后续 Tool family 无法在同一变更中建立安全 Lease route 时不得采用 Foundation 发布新 Schema；不得 fallback。
Risk: medium
Estimated blast radius: small
Can run in parallel with: P2A2-005～P2A2-006

## P2A2-005 — Local Tauri IPC Workspace Authority Foundation / Audit

Phase: Phase 2A.2
Type: contract-test
Goal: 盘点 Local Tauri Workspace-scoped IPC surface，固定显式 `workspaceId` DTO/serialization contract，并建立每个 surface 到其原子 Authority cutover owner 的映射。
Why now: 本地调用不能绕过 Remote 契约，但不应发布“仅校验 ID、实际 Root 仍由全局状态决定”的中间行为。
Dependencies: P2A2-001、P2A2-004 (H)
Blocked by: None
Allowed scope: `commands.rs`/`api.ts`/调用方的 IPC surface audit、DTO/serialization contract tests、任务映射与证据。
Forbidden scope: Agent Start/Git/CodeGraph/Serena Semantic/Source 的公开行为或 handler route；Execution Workspace snapshot；AgentTaskManager、StateStore、Claim、Provider Dispatch；从 DesktopSelectedWorkspace 自动补值。
Contract references: §10.1；§10.4；§10.6；§52 Phase 2A.2
Implementation requirements: UI 可把 selection 作为新任务默认输入，但请求 payload 必须显式；每个尚未安全 cutover 的 surface 仅记录 owner，不新增 validation-only 行为。
Non-goals: 修改 selection 模型；执行 Agent Start 的 Resolver/Lease/snapshot 迁移（P2A2-007）。
Tests required: DTO serialization、selection 仅作默认值、IPC surface→owner mapping；不得新增全局 fallback。
Evidence required: IPC contract/audit tests。
Acceptance criteria: Local IPC Foundation 已验证且不改变未完成 backend Authority cutover 的公开行为；映射明确为 Agent Start→P2A2-007、Git→P2A2-009、CodeGraph→P2A2-010/后续 Workspace Capability Adapter、Serena Semantic→P2A3-010、Source→P2A3-011。
Rollback / failure behavior: 无安全 backend route 的 surface 只保留 audit/mapping，不得新增 Schema、validation-only 或 fallback。
Risk: medium
Estimated blast radius: small
Can run in parallel with: P2A2-006

## P2A2-006 — Execution Workspace Generation Persistence / Migration Compatibility

Phase: Phase 2A.2  
Type: migration  
Goal: 验证已实现的 Execution `workspace_generation` 持久化、schema migration 与兼容性。  
Why now: 持久化 generation 是 P2A2-007 原子 Start Authority cutover 的既有数据基础。  
Dependencies: P2A2-001、P0-003 (H)  
Blocked by: None  
Allowed scope: StateStore/schema migration、Execution/WorkRun record 持久化读取、旧库/restart/fixture 与 hash compatibility tests。  
Forbidden scope: Begin/Start/Continue Workspace snapshot 构造；Global ActiveWorkspace、DesktopSelectedWorkspace、session 或 last-request Authority；状态机、Claim、terminal/recovery 语义。  
Contract references: §10.6；§54.1  
Implementation requirements: 旧 nonterminal 只能从已有权威持久化事实回填；无法证明 generation 时 fail-closed。generation 参与冻结的持久化/hash 契约时，仅验证版本与兼容性 guard。  
Non-goals: Start Authority route（P2A2-007）；Continue Workspace inheritance（P2A2-008）。  
Tests required: old DB migration、restart、fixture、generation persistence/readback、hash compatibility、ambiguous fail-closed。  
Evidence required: schema and migration tests。  
Acceptance criteria: 每个新 Execution 可读取冻结 generation；历史升级与兼容性不猜测环境 Workspace；本任务不新增或保留任何 ambient Workspace→Execution snapshot 路径。  
Rollback / failure behavior: 无法证明的旧执行不得猜测 selection。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A2-002～P2A2-005

## P2A2-007 — agent_execute start 原子冻结 Workspace

Phase: Phase 2A.2  
Type: implementation  
Goal: 使 Local/Remote Agent Start 统一经 `request.workspaceId → WorkspaceResolver → WorkspaceLease`，并在 Execution/Claim 创建事务中原子冻结 id/root/generation。
Why now: 新 Execution 的唯一 Workspace Authority；P2A2-005 仅审计 IPC 边界，不得单独完成或模拟该 cutover。
Dependencies: P2A2-004、P2A2-005、P2A2-006 (H)
Blocked by: None  
Allowed scope: Local Tauri Agent Start command/API adapter、MCP orchestration DTO/route、product/work/task manager/store transaction、Claim 与 focused tests。
Forbidden scope: 修改 requestKey 语义、Continue Workspace inheritance、global ActiveWorkspace/DesktopSelectedWorkspace/session/last-request fallback。
Contract references: §5.8～§5.10；§10.6  
Implementation requirements: caller 不提供 root；Local/Remote request 使用同一 Resolver/Lease 路径；snapshot 与 Execution/Claim create 同一原子事务，成功后才允许 Provider Dispatch。
Non-goals: Continue。  
Tests required: A/B start、root/generation change、requestKey replay、Claim regressions。  
Evidence required: Work/Execution persistence tests。  
Acceptance criteria: Local/Remote Start 的 request ID 是唯一 Root authority；start 后 UI/Registry 修改不能改变 Execution Root。
Rollback / failure behavior: resolve/create/claim 任一步失败均不派发，且不得回退全局、Desktop、session 或 last-request 状态。
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A2-010

## P2A2-008 — Continue Workspace Inheritance

Phase: Phase 2A.2  
Type: implementation  
Goal: Continue 创建新 Execution 并继承父 Execution Workspace snapshot。  
Why now: 防止出现 executionId 与 caller workspaceId 双 Authority。  
Dependencies: P2A2-007 (H)  
Blocked by: None  
Allowed scope: agent execute DTO/product/work tests。  
Forbidden scope: Continue 接收 workspaceId、修改 thread/historyMode 语义。  
Contract references: §5.10；§10.6；§56.67  
Implementation requirements: query/cancel/continue 仅以 executionId/parentExecutionId 定位。  
Non-goals: Usage continue baseline。  
Tests required: parent A + selection B、malicious workspace field、restart Continue。  
Evidence required: continuation tests。  
Acceptance criteria: child snapshot 与 parent 完全一致。  
Rollback / failure behavior: 父身份不完整则拒绝 Continue。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A2-010

## P2A2-009 — Git Lease-rooted Execution 与 Relative Path

Phase: Phase 2A.2  
Type: implementation  
Goal: 在 Authority cutover 前使现有 Git backend 从请求 `workspaceId` 解析 Lease，并以 `git -C lease.canonicalRoot` 执行。  
Why now: 当前 Git handler 读取 `active.workspace.root`；若等到 2D 才迁移，会产生公开 Git Tool 断档。  
Dependencies: P2A2-001、P2A2-002、P2A2-004 (H)  
Blocked by: None  
Allowed scope: `src-tauri/src/mcp/{mod,git,registry}.rs` 的 Git 基础路由及 focused tests。  
Forbidden scope: 新 Git compatibility facade、Provider Port 收敛、caller root、Desktop/session/global active fallback、公共 `path` 字段 rename。  
Contract references: §10.4；§10.7 Git；§52 Phase 2A.2 Backend Continuity；§53 Capability Runtime/Git  
Implementation requirements: `git_status/git_diff/git_log` 均先 resolve Lease；可选 `path` 走 WorkspacePathResolver，拒绝 absolute/UNC/escape；Git 保持 stateless command，并脱离当前 Broker 的 Serena server/client readiness 前置检查。  
Non-goals: 在 2A.2 将 Git 包装为 WorkspaceCapabilityProvider。  
Tests required: A/B 并发 `git -C` Root、Serena unavailable 时 Git 正常、missing/unknown ID、非 Git、cancel、合法/非法 `path`。  
Evidence required: command invocation 与 request/Lease/Root trace。  
Acceptance criteria: Global ActiveWorkspace 移除后，Git 仍可用且只访问请求 Lease Root。  
Rollback / failure behavior: 无法安全 resolve/validate 时拒绝调用，不恢复 ActiveWorkspace route。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A2-007～P2A2-008

## P2A2-010 — CodeGraph Authority Transition Policy

Phase: Phase 2A.2  
Type: migration  
Goal: 在旧 Global Active CodeGraph 路由撤除到 2D Adapter 可用之间，显式标记 CodeGraph unavailable/不 advertise。  
Why now: 当前 `codegraph_explore` 读取 `active.graph`，而新 Workspace Slot 到 2D 才落地。  
Dependencies: P2A2-001、P2A2-004 (H)  
Blocked by: None  
Allowed scope: `src-tauri/src/mcp/{mod,registry,codegraph}.rs` 的可用性/发布边界及 tests。  
Forbidden scope: 临时 CodeGraph Runtime、global binding fallback、retarget、隐式 init/index/sync。  
Contract references: §10.4～§10.7；§42；§52 Backend Continuity；§55 Workspace/Capability Runtime  
Implementation requirements: 2D 新 Adapter 就绪前不 advertise 旧 CodeGraph Tool；即使直接调用也不得执行旧 global-active handler；P2A3-013 Health DTO 到位后投影为 unavailable，不让 2A.2 依赖该 DTO。  
Non-goals: 在 2A.2 实现 CodeGraph Adapter 或 status --json。  
Tests required: tool list 无旧 CodeGraph advertise、直接调用不读 Global Active、Desktop selection/activate 不能恢复、其他 Source/Git Tool 不受影响；2A.3 后 Health unavailable。  
Evidence required: Tool catalog diff、CodeGraph unavailable 与无副作用测试。  
Acceptance criteria: Authority cutover 后没有可执行的 global-active CodeGraph 正常路径。  
Rollback / failure behavior: 未有 Lease-scoped Adapter 前维持 unavailable，不回退旧 Binding。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: P2A2-009

## P2A2-011 — 移除 Global ActiveWorkspace 与 Session Routing Authority

Phase: Phase 2A.2  
Type: migration  
Goal: 从正常 execution path 移除 Broker ActiveWorkspace/Activate/session/last-request fallback。  
Why now: 完成请求级 Authority 切换。  
Dependencies: P2A2-004、P2A2-005、P2A2-009、P2A2-010、P2A3-011 (H)  
Blocked by: Lease-routed Serena Source compatibility route；Git Lease route；CodeGraph unavailable policy  
Allowed scope: `mcp/mod.rs`、registry descriptions、commands compatibility handlers/tests。  
Forbidden scope: 删除必要 deprecated surface、建立 Session Map。  
Contract references: §10.4～§10.5；§52 Backend Continuity；§55 Workspace  
Implementation requirements: compatibility handlers 不建立后续请求 binding。  
Non-goals: 删除 UI selection。  
Tests required: query/activate 后 Source/Git missing workspaceId 仍失败；reconnect 无 rebind；旧 CodeGraph handler 不可执行。  
Evidence required: routing/forbidden-state tests。  
Acceptance criteria: 新 Tool path 不读取 global active/session。  
Rollback / failure behavior: 无显式 Lease 的 Tool disable，不恢复 fallback。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## P2A2-012 — Workspace Remove Coordination Foundation

Phase: Phase 2A.2  
Type: implementation  
Goal: 以显式 typed coordination 检查 Agent Claim 与后续 WriteGuard/Capability Operation 的 Remove exclusion。  
Why now: 后续组件应进入同一删除检查点，但不需要动态 participant/hook registry。  
Dependencies: P2A1-008、P2A2-001 (H)  
Blocked by: None  
Allowed scope: workspace remove typed coordination、现有 claim adapter、tests。  
Forbidden scope: 动态 participant/hook/guard registry、新全局锁系统、提前实现 WriteGuard/RuntimeSlot。  
Contract references: §8.6；§10.8～§10.9  
Implementation requirements: 使用现有 operation mutex 和显式 typed 检查；后续具体 Guard/Slot 到位后直接接线，失败保留 Registry entry。  
Non-goals: 删除磁盘。  
Tests required: current claim busy、idle success、typed 检查失败保留 Registry entry。  
Evidence required: coordinator tests。  
Acceptance criteria: Remove 有单一 fail-closed coordination point。  
Rollback / failure behavior: 任一参与者失败即中止删除。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A2-007～P2A2-008

## P2A2-013 — Request A/B Isolation 与 Backend Continuity Gate

Phase: Phase 2A.2  
Type: integration-test  
Goal: 关闭 2A.2 Gate，并证明公开 Source/Git 连续可用、CodeGraph 明确 unavailable。  
Why now: 防止 Authority cutover 后 Source/Git 断档或 CodeGraph 偷读 Global Active。  
Dependencies: P2A2-001～P2A2-012、P2A3-011 (H)  
Blocked by: P2A3 Serena Source compatibility route  
Allowed scope: MCP/IPC/integration tests。  
Forbidden scope: 实现 Rust Source。  
Contract references: §52 Phase 2A.2 与 Backend Continuity；§53 Workspace/Isolation  
Implementation requirements: 并发 A/B Source/Git、missing ID、Discovery no-binding、known ID reuse；CodeGraph 不 advertise/不执行旧 route。  
Non-goals: Provider capacity 全矩阵。  
Tests required: 2A.2 Gate 全部场景。  
Evidence required: concurrent request trace 与 test totals。  
Acceptance criteria: 无共享 ActiveWorkspace/Session Binding；Source/Git 连续可用；CodeGraph 只在 2D 新 Adapter 就绪后恢复。  
Rollback / failure behavior: 失败阻止 2A.2 Gate 和 Phase 2B。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: P2A3-012～P2A3-014

---

---

# Phase 2A.3 — Workspace Capability Registry / Serena Runtime

## P2A3-001 — Capability Descriptor 与 Object-safe Provider Port

Phase: Phase 2A.3  
Type: implementation  
Goal: 定义 capability descriptor、readiness/stage/action/health 和显式 Lease 的 Provider Port。  
Why now: Manager 与 Serena Adapter 的公共边界。  
Dependencies: P2A2-001、P1-008 (H)  
Blocked by: None  
Allowed scope: 新 workspace capability domain module/tests。  
Forbidden scope: AgentProvider 合并、动态 Plugin ABI。  
Contract references: §10.7～§10.9；§41～§42  
Implementation requirements: `call(lease,runtime,tool)`；公共 DTO 不暴露 PID/port/root。  
Non-goals: Registry/Runtime。  
Tests required: object safety、DTO serialization、lease/runtime identity mismatch。  
Evidence required: unit/compile tests。  
Acceptance criteria: Core 无 Serena/CodeGraph 专用字段。  
Rollback / failure behavior: 纯 domain 可独立移除。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P0-005 evidence

## P2A3-002 — WorkspaceCapabilityRegistry 与 Manager Shell

Phase: Phase 2A.3  
Type: implementation  
Goal: 实现 built-in 注册、descriptor lookup 和 Manager 的无进程骨架。  
Why now: RuntimeSlot 必须由通用 Registry 驱动。  
Dependencies: P2A3-001 (H)  
Blocked by: None  
Allowed scope: capability registry/manager modules。  
Forbidden scope: providerId 特判、process start。  
Contract references: §10.7；§41；§52 Phase 2A.3  
Implementation requirements: duplicate ID/tool fail；第三 Provider 不改 Core。  
Non-goals: 动态加载 Provider。  
Tests required: duplicate/unknown/list/third fake provider。  
Evidence required: registry tests。  
Acceptance criteria: Manager 仅依赖 trait/descriptor。  
Rollback / failure behavior: 注册失败阻止对应 capability startup。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P2A3-007

## P2A3-003 — RuntimeSlot State、Acquire Single-flight 与 In-flight Guard

Phase: Phase 2A.3  
Type: implementation  
Goal: 建立 `(providerId,workspaceId,generation)` Slot 和安全 acquire。  
Why now: 所有生命周期策略的基础。  
Dependencies: P2A3-002 (H)  
Blocked by: None  
Allowed scope: capability manager/runtime slot/tests。  
Forbidden scope: Serena process implementation。  
Contract references: §10.8～§10.9 Acquire/Single-flight  
Implementation requirements: stopped/starting/ready/error/stopping；同 Workspace 一次 startup。  
Non-goals: capacity/LRU。  
Tests required: concurrent acquire、guard count、generation mismatch。  
Evidence required: deterministic concurrency tests。  
Acceptance criteria: 两次首次调用只产生一个 start future。  
Rollback / failure behavior: startup failure 只标目标 Slot error。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A3-007

## P2A3-004 — Capacity Accounting 与 LRU Eviction

Phase: Phase 2A.3  
Type: implementation  
Goal: 实现 per-Provider `maxInstances` 和 zero-in-flight LRU eviction。  
Why now: 跨 Workspace 并发必须受真实容量约束。  
Dependencies: P2A3-003 (H)  
Blocked by: None；Serena 实际 `maxInstances`/`idleTimeout` 取值留到 P2A3-008，P0-005 (E) 已 completed/resolved

Allowed scope: capability manager policy/tests。  
Forbidden scope: retarget live runtime、驱逐 in-flight slot。  
Contract references: §10.8～§10.9 Capacity/Idle Eviction  
Implementation requirements: 用 FakeWorkspaceCapabilityProvider 实现通用 `maxInstances`/in-flight/LRU/BUSY/stop-failure 算法；无安全 Slot 时返回 provider busy。  
Non-goals: 动态配置 UI。  
Tests required: Fake Provider 的 capacity available、idle LRU、all in-flight busy、stop failure。  
Evidence required: scheduler tests。  
Acceptance criteria: 隔离始终成立；并发只在容量允许时成立。  
Rollback / failure behavior: stop failure 保留 handle 和容量。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## P2A3-005 — Idle Timeout 与 Stop Single-flight

Phase: Phase 2A.3  
Type: implementation  
Goal: 实现 idle stop 和 eviction/remove/shutdown 共享的 stop single-flight。  
Why now: 防止重复 kill 与 orphan。  
Dependencies: P2A3-003 (H)  
Blocked by: None  
Allowed scope: runtime slot lifecycle/tests。  
Forbidden scope: 停止 in-flight runtime、删除 Registry/index。  
Contract references: §10.9 Failure/Shutdown；§10.9 Workspace Remove  
Implementation requirements: guard release 后重新判断 idle；stop 失败保留 ownership。  
Non-goals: Host shutdown wiring。  
Tests required: timeout、in-flight deferral、concurrent stop requests。  
Evidence required: lifecycle tests。  
Acceptance criteria: 每个 Slot 同时最多一个 stop。  
Rollback / failure behavior: 停止失败进入 error 且不启动替代进程。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A3-004

## P2A3-006 — Remove 与 Host Shutdown Runtime Coordination

Phase: Phase 2A.3  
Type: implementation  
Goal: 将 RuntimeSlot 接入 Workspace Remove 和 Host shutdown。  
Why now: Registry 删除和进程 ownership 必须闭合。  
Dependencies: P2A2-012、P2A3-005 (H)  
Blocked by: None  
Allowed scope: capability manager、remove coordinator、`lib.rs` shutdown、tests。  
Forbidden scope: 删除磁盘/index、忽略 stop failure。  
Contract references: §10.9 Workspace Remove；§51.4；§55 Capability Runtime  
Implementation requirements: remove 阻止 acquire/in-flight；先 stop idle；shutdown 收敛所有 live slot。  
Non-goals: Provider-specific kill 逻辑。  
Tests required: remove/acquire race、stop failure、shutdown no orphan fake handles。  
Evidence required: lifecycle integration tests。  
Acceptance criteria: stop 失败时 Registry entry 保留。  
Rollback / failure behavior: fail closed 保留 authority/handle。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A3-007

## P2A3-007 — Serena Adapter Shell 与 Readiness/Version Probe

Phase: Phase 2A.3  
Type: implementation  
Goal: 注册 Serena capability，并投影 installation/readiness/stages/actions。  
Why now: process start 前先冻结真实 readiness。  
Dependencies: P2A3-002、P0-004 (E)  
Blocked by: Serena CLI workflow evidence  
Allowed scope: Serena capability adapter/probe/tests。  
Forbidden scope: 启动 process、把 project.yml 当 Workspace authority。  
Contract references: §8.2 Serena Readiness；§11.1～§11.2；§41～§42  
Implementation requirements: binary missing→unavailable；project config absent→not_prepared/auto_preparable。  
Non-goals: index 执行。  
Tests required: missing/incompatible/ready/absent config。  
Evidence required: probe fixtures/tests。  
Acceptance criteria: Serena failure 不影响 Core 或其他 Provider。  
Rollback / failure behavior: 单独 unregister/mark unavailable。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: P2A3-003、P2A3-006

## P2A3-008 — Serena Workspace-scoped Process Startup

Phase: Phase 2A.3  
Type: implementation  
Goal: 每个 live Slot 启动独立 Serena process/endpoint/client。  
Why now: 替代 global Serena process + activate switching。  
Dependencies: P2A3-003、P2A3-004、P2A3-007、P0-005 (H/E, completed/resolved)

Blocked by: None；P0-005 evidence 与 approved per-slot `SERENA_HOME` DCR 已解除此前 blocker

Allowed scope: Serena adapter runtime、process launcher、tests。  
Forbidden scope: 复用后 retarget、改变现有 Agent Runtime。  
Contract references: §10.7～§10.9；§11.2～§11.3  
Implementation requirements: 每个 live Slot 的 `SERENA_HOME` 与 `(providerId=serena, workspaceId, workspaceGeneration)` Runtime identity 一致，不同 live Slot 不共享 writable Home/config；启动前在该 Slot Home 生成/验证最小受管 global config，保持 `trusted_project_path_patterns=[]`、loopback、受管 context 与固定 Tool allowlist。首次 `--project <canonicalRoot>` 只能更新本 Slot Home，`projects` 列表不得作为 Authority；ready 后验证 active/canonical Root 与 Lease 一致。stop/idle eviction 可持久复用 Home，Workspace Remove 不删除 Workspace 下 `.serena`，不新增 retention/GC。Serena `maxInstances > 1` 的首版具体值由本任务基于 P0-005 资源证据冻结。

Non-goals: auto prepare/index。  
Tests required: A/B identity、各 Slot 独立 Home/config、same-workspace single-flight、容量允许时 A/B 真正并发、capacity busy、crash isolation；首次启动只写本 Slot Home 且 `projects` 不参与 Root Authority；idle stop/reacquire 复用受管 Home，Workspace Remove 不删除 `.serena`。

Evidence required: process fixture/integration results。  
Acceptance criteria: A/B 永不共享可 retarget process。  
Rollback / failure behavior: 目标 capability unavailable/error，不恢复 global process。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## P2A3-009 — Serena Project Auto-prepare

Phase: Phase 2A.3  
Type: implementation  
Goal: 首次 Semantic acquire 可创建默认 Project Configuration 并继续调用。  
Why now: 未配置项目不能被误判为不可用。  
Dependencies: P2A3-008 (H)  
Blocked by: P0-004 evidence  
Allowed scope: Serena adapter prepare path/tests。  
Forbidden scope: index、onboarding、Workspace registration mutation。  
Contract references: §8.2；§11.2；§41～§42  
Implementation requirements: 仅 Project Configuration + Runtime Activation；postcondition Root 校验。  
Non-goals: Local explicit index。  
Tests required: absent config、concurrent prepare、failure cleanup、Windows casing。  
Evidence required: created config 和调用继续证据。  
Acceptance criteria: 首次调用无需用户先手工初始化项目。  
Rollback / failure behavior: prepare 失败返回 capability error，不改变 Registry。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A3-013

## P2A3-010 — Serena Semantic Call Routing

Phase: Phase 2A.3  
Type: implementation  
Goal: Semantic Tool 经 Lease→Manager→Serena Slot 调用。  
Why now: 关闭 Serena 正常 execution path。  
Dependencies: P2A3-008、P2A3-009 (H)  
Blocked by: None  
Allowed scope: Serena MCP adapter/tool routing/tests。  
Forbidden scope: payload root、Desktop/session/global active lookup。  
Contract references: §10.4；§10.7；§11.2  
Implementation requirements: runtime identity 必须匹配 Lease identity。  
Non-goals: Source cutover。  
Tests required: A/B calls、runtime mismatch、missing ID、cancel。  
Evidence required: routing trace/tests。  
Acceptance criteria: Semantic request 只访问显式 Workspace。  
Rollback / failure behavior: 无安全 Slot 时返回 busy/unavailable。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A3-013

## P2A3-011 — Serena-backed Source Compatibility Facade

Phase: Phase 2A.3  
Type: implementation  
Goal: 现有四个 Source Tool 临时经 request Lease 与 Serena Slot 工作。  
Why now: 闭合 2A.2 Backend Continuity。  
Dependencies: P2A2-004、P2A3-008～P2A3-010 (H)  
Blocked by: None  
Allowed scope: `mcp/source_read.rs`、Broker routing、Serena capability adapter tests。  
Forbidden scope: 第二套 Runtime 架构、global activate、公共字段 rename。  
Contract references: §52 Backend Continuity；§55 Source Read  
Implementation requirements: 保持 `relative_path`；caller root 永不进入 backend。  
Non-goals: Rust Source 实现。  
Tests required: A/B Source、missing ID、selection mismatch、Serena unavailable。  
Evidence required: compatibility integration tests。  
Acceptance criteria: 删除 ActiveWorkspace 后 Source 仍可用且不串线。  
Rollback / failure behavior: 无安全 Lease route 时禁用 Source Tool。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2A3-012～P2A3-013

## P2A3-012 — Explicit Serena Actions 与 Preparation Activity

Phase: Phase 2A.3  
Type: implementation  
Goal: 实现 Local Serena prepare/build_index actions 与安全 Preparation Activity。  
Why now: 自动准备和可选索引必须明确分离。  
Dependencies: P2A3-007、P2A3-009、P1-007 (H)  
Blocked by: None  
Allowed scope: capability action handler、Serena adapter、Activity projection tests。  
Forbidden scope: onboarding action、Remote query 隐式 index、暴露 command/stdout/root。  
Contract references: §10.9 Preparation Activity；§11；§29；§41  
Implementation requirements: action authority=`local_human`；single-flight。  
Non-goals: CodeGraph actions；Onboarding 仍属 Agent workflow，只能在 Health 中作为独立 Stage 展示。  
Tests required: explicit action、duplicate action、safe progress/privacy。  
Evidence required: action/telemetry tests。  
Acceptance criteria: Semantic ready 不依赖 index/onboarding。  
Rollback / failure behavior: action 失败只影响对应 stage。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: P2A3-011

## P2A3-013 — Descriptor-driven Capability Health Projection

Phase: Phase 2A.3  
Type: implementation  
Goal: 按 Workspace 动态投影 capability status/readiness/runtime/stages/actions。  
Why now: 后续 UI 与 CodeGraph 必须复用统一 DTO。  
Dependencies: P2A3-002、P2A3-003、P2A3-007 (H)  
Blocked by: None  
Allowed scope: capability product projection、Workspace query DTO/tests。  
Forbidden scope: UI 写死 Serena、公开 PID/port/root/error details。  
Contract references: §41；§51.4  
Implementation requirements: Provider/Workspace failure 隔离；stopped 不等于 unavailable。  
Non-goals: Health UI。  
Tests required: ready/not_prepared/unavailable/error/stopped/fake third provider。  
Evidence required: DTO snapshots。  
Acceptance criteria: 新 Provider 无需修改 Core DTO。  
Rollback / failure behavior: provider projection 失败仅隐藏/标错该 Provider。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: P2A3-011～P2A3-012

## P2A3-014 — Serena Runtime Isolation Gate

Phase: Phase 2A.3  
Type: integration-test  
Goal: 关闭 Phase 2A.3 Gate。  
Why now: Runtime isolation 是硬保证。  
Dependencies: P2A3-001～P2A3-013 (H)  
Blocked by: None；P0-005 (E) 已 completed/resolved，approved per-slot `SERENA_HOME` DCR 已满足该 Gate 前置条件
Allowed scope: capability/Serena integration tests。  
Forbidden scope: Rust Source cutover。  
Contract references: §52 Phase 2A.3；§53 Capability Runtime；§56.70～§90  
Implementation requirements: lazy/single-flight/capacity/LRU/busy/remove/shutdown/privacy。  
Non-goals: CodeGraph。  
Tests required: Phase 2A.3 Gate 全矩阵。  
Evidence required: process identity 和 test totals。  
Acceptance criteria: 隔离始终成立，并发只在 capacity 允许时成立。  
Rollback / failure behavior: 任一失败阻止 2B；不恢复 retarget。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: P2A2-013

---

---

# Phase 2B — Rust Source Read

## P2B-001 — Shared Rust Source Read Infrastructure

Phase: Phase 2B  
Type: implementation  
Goal: 建立 Lease/path resolution、bounded output、cancel、provenance 共用基础。  
Why now: 四个 Tool 不应重复安全逻辑。  
Dependencies: P2A2-013、P2A3-014 (H)  
Blocked by: None  
Allowed scope: 新 Source adapter/infrastructure、shared tests。  
Forbidden scope: Source Write、Git、字段 rename。  
Contract references: §12.1～§12.2；§51.2  
Implementation requirements: 只接收服务端 Lease；不长持 Registry lock。  
Non-goals: 具体 Tool 行为。  
Tests required: boundary、junction、cancel、provenance helper。  
Evidence required: unit tests。  
Acceptance criteria: 后续 Tool 只组合基础设施，不自行解析 root。  
Rollback / failure behavior: Rust adapter 可禁用并保留 Lease-routed compatibility backend。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## P2B-002 — source_read_file

Phase: Phase 2B  
Type: implementation  
Goal: 将 `source_read_file` 切换为 Rust。  
Why now: 最基础、最易验证的 Source cutover。  
Dependencies: P2B-001 (H)  
Blocked by: None  
Allowed scope: Source adapter、registry handler、focused tests。  
Forbidden scope: `relative_path`→`path` rename、写操作。  
Contract references: §12.1～§12.3；§50 Source  
Implementation requirements: default 32 KiB、hard 128 KiB；full raw SHA；binary/UTF-8 policy；cancel/provenance。  
Non-goals: 其他 Source Tool。  
Tests required: bounds/truncation/full SHA/binary/junction/cancel。  
Evidence required: tool contract tests。  
Acceptance criteria: 无 Serena 调用且结果契约稳定。  
Rollback / failure behavior: 可回兼容 backend，但必须继续使用 Lease。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2B-003～P2B-005（P2B-001 完成后）

## P2B-003 — source_list_dir

Phase: Phase 2B  
Type: implementation  
Goal: 将目录枚举切换为 Rust。  
Why now: 独立验证 hidden/symlink/output budget。  
Dependencies: P2B-001 (H)  
Blocked by: None  
Allowed scope: Source adapter/list handler/tests。  
Forbidden scope: 改公共 `relative_path`。  
Contract references: §12.4；§51.2  
Implementation requirements: Workspace-relative；bounded；ignore/hidden/symlink contract。  
Non-goals: recursive search。  
Tests required: empty/nested/hidden/symlink/junction/cancel/budget。  
Evidence required: focused tests。  
Acceptance criteria: 不越过 Lease Root。  
Rollback / failure behavior: 保持兼容 backend route。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P2B-002、P2B-004～P2B-005

## P2B-004 — source_find_file

Phase: Phase 2B  
Type: implementation  
Goal: 将文件名查找切换为 Rust。  
Why now: 与内容搜索分离以控制行为边界。  
Dependencies: P2B-001 (H)  
Blocked by: None  
Allowed scope: Source adapter/find handler/tests。  
Forbidden scope: 内容正则搜索、字段 rename。  
Contract references: §12.5；§51.2  
Implementation requirements: bounded traversal/result、hidden/ignore/symlink/cancel/provenance。  
Non-goals: CodeGraph search。  
Tests required: match/no-match/limit/hidden/junction/cancel。  
Evidence required: focused tests。  
Acceptance criteria: 只返回 Workspace-relative 结果。  
Rollback / failure behavior: 可回 Lease-routed 兼容 backend。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P2B-002～P2B-003、P2B-005

## P2B-005 — source_search_pattern

Phase: Phase 2B  
Type: implementation  
Goal: 将内容模式搜索切换为 Rust。  
Why now: 搜索有独立 cancellation/output 风险。  
Dependencies: P2B-001 (H)  
Blocked by: None  
Allowed scope: Source adapter/search handler/tests。  
Forbidden scope: Shell command注入、CodeGraph替代、字段 rename。  
Contract references: §12.6；§51.2  
Implementation requirements: bounded matches/context/output；binary/hidden/ignore；及时 cancel。  
Non-goals: 新搜索语言。  
Tests required: literal/regex contract、large tree、binary、cancel、budget。  
Evidence required: focused tests。  
Acceptance criteria: 搜索不越界且不会无界输出。  
Rollback / failure behavior: 可回 Lease-routed compatibility backend。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2B-002～P2B-004

## P2B-006 — 移除 Serena-backed Source Compatibility Route

Phase: Phase 2B  
Type: migration  
Goal: 四个 Source handler 全部切到 Rust 后移除临时 facade。  
Why now: 防止双 backend 漂移。  
Dependencies: P2B-002～P2B-005 (H)  
Blocked by: None  
Allowed scope: Source routing、Serena compatibility adapter、tests。  
Forbidden scope: 删除 Serena Semantic capability。  
Contract references: §11；§12；§52 Backend Continuity；§55 Source Read  
Implementation requirements: Tool schema/provenance 保持稳定。  
Non-goals: Source Adapter 最终 2D Port 封装。  
Tests required: no Serena call、Serena missing 仍可 Source read。  
Evidence required: routing tests/forbidden dependency check。  
Acceptance criteria: 四个 Source Read 不依赖 Serena process。  
Rollback / failure behavior: 临时回退仍必须 Lease-routed。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: None

## P2B-007 — Rust Source Read Integration Gate

Phase: Phase 2B  
Type: integration-test  
Goal: 关闭 Phase 2B Gate。  
Why now: 四个独立 Tool 需要统一安全回归。  
Dependencies: P2B-006 (H)  
Blocked by: None  
Allowed scope: Source integration/MCP tests。  
Forbidden scope: Source Write。  
Contract references: §52 Phase 2B；§53 Source Read；§56.11～§14、§95  
Implementation requirements: A/B isolation、limits、cancel、provenance、junction。  
Non-goals: Provider runtime tests。  
Tests required: 完整 Source Read matrix。  
Evidence required: totals 和关键错误码。  
Acceptance criteria: 所有 Gate PASS，Serena unavailable 不影响 Source。  
Rollback / failure behavior: 失败阻止 2C。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

---

---

# Phase 2C — Rust Source Write

## P2C-001 — Source Write Domain、Errors 与 Limits

Phase: Phase 2C  
Type: implementation  
Goal: 冻结六 Tool 共用输入、结果、稳定错误和 hard limits。  
Why now: 防止各 Tool 产生不同写契约。  
Dependencies: P2B-007 (H)  
Blocked by: None  
Allowed scope: Source write domain/schema/tests。  
Forbidden scope: 实际文件 commit、Remote enable。  
Contract references: §13.1～§13.4；§13.15；§50 Source  
Implementation requirements: expectedSha256、1-based inclusive range、binary/text、size limits。  
Non-goals: Tool implementation。  
Tests required: validation/error mapping/serialization。  
Evidence required: contract tests。  
Acceptance criteria: 非法输入在触碰文件前失败。  
Rollback / failure behavior: write capability 保持 unavailable。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P2C-002、P2C-005

## P2C-002 — WorkspaceWriteGuard

Phase: Phase 2C  
Type: implementation  
Goal: 实现 per-Workspace refcount，只用于阻止 Remove。  
Why now: Write 生命周期与 Registry 删除必须协调。  
Dependencies: P2A2-012 (H)  
Blocked by: None  
Allowed scope: workspace write guard、remove coordinator、tests。  
Forbidden scope: 复用 Agent Claim、串行不同 target。  
Contract references: §8.6；§13.12；§56.96  
Implementation requirements: acquire 验证 Lease generation；Drop 释放 refcount。  
Non-goals: Commit mutex。  
Tests required: concurrent guards、remove busy、release 后 remove。  
Evidence required: concurrency tests。  
Acceptance criteria: Guard 不限制同 Workspace 不同文件并发。  
Rollback / failure behavior: guard 状态不确定时拒绝 Remove/Write commit。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2C-001、P2C-003

## P2C-003 — TargetCommitMutex 与 Locked Revalidation

Phase: Phase 2C  
Type: implementation  
Goal: 按 canonical target path 串行 commit 并在锁内重验 SHA/path/Lease。  
Why now: OCC 的原子决胜点。  
Dependencies: P2A2-002、P2C-002 (H)  
Blocked by: None  
Allowed scope: keyed mutex/commit coordinator/tests。  
Forbidden scope: `LockFileEx`、跨进程事务声明、全 Workspace mutex。  
Contract references: §13.2；§13.12；§51.3  
Implementation requirements: expectedSha256 在锁内验证；无关 target 可并发。  
Non-goals: 文件 replace 实现。  
Tests required: same-target race、different-target concurrency、external edit conflict。  
Evidence required: deterministic concurrency tests。  
Acceptance criteria: 同 SHA 的两个写至多一个成功。  
Rollback / failure behavior: 重验失败不写文件。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2C-001、P2C-004～P2C-005

## P2C-004 — Crash-safe Atomic Replace Helper

Phase: Phase 2C  
Type: implementation  
Goal: 实现同目录临时文件、flush/replace/cleanup 的 crash-safe helper。  
Why now: 所有修改现有文件的 Tool 共用 commit primitive。  
Dependencies: P2C-003 (H)  
Blocked by: None  
Allowed scope: Source atomic file helper、真实 child-process tests。  
Forbidden scope: 修改 Tool semantics、跨文件事务。  
Contract references: §13.13；§51.3  
Implementation requirements: crash 不产生半文件；临时文件可收敛；Windows documented ambiguous native failure 只经 canonical target re-read 投影为 success、safe IO failure 或 `SOURCE_COMMIT_STATE_UNKNOWN`，不自动恢复。
Non-goals: LockFileEx。  
Tests required: replace success、permission failure、pre/post-replace crash。  
Evidence required: child-process crash/reopen filesystem assertions。  
Acceptance criteria: success 时 target 为完整新版；safe failure 时 target 为完整旧版；Windows documented ambiguous native failure 返回 `SOURCE_COMMIT_STATE_UNKNOWN` 后必须 re-read，且绝不产生半文件。
Rollback / failure behavior: pre-commit/native-safe failure 保留原文件并清理本次临时文件；ambiguous native failure 不做自动 rollback/recovery。
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2C-005

## P2C-005 — Newline Policy

Phase: Phase 2C  
Type: implementation  
Goal: 实现 LF/CRLF 检测、tie-break 与保留规则。  
Why now: 行编辑 Tool 需要统一文本输出。  
Dependencies: P2C-001 (H)  
Blocked by: None  
Allowed scope: Source text helper/tests。  
Forbidden scope: 文件 commit、编码自动转换。  
Contract references: §13.11  
Implementation requirements: tie 时 first newline wins；无换行按冻结默认。  
Non-goals: formatter。  
Tests required: LF/CRLF/mixed/tie/no newline。  
Evidence required: pure helper tests。  
Acceptance criteria: 所有 Tool 使用同一 newline helper。  
Rollback / failure behavior: 无法判定按冻结规则，不猜测平台格式。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: P2C-002～P2C-004

## P2C-006 — create_text_file

Phase: Phase 2C  
Type: implementation  
Goal: 实现只创建不存在文本文件。  
Why now: 与 overwrite 语义分离。  
Dependencies: P2C-001～P2C-004 (H)  
Blocked by: None  
Allowed scope: Source write adapter/create handler/tests。  
Forbidden scope: 覆盖已存在目标、自动建越界父路径。  
Contract references: §13.8；§13.13～§13.15  
Implementation requirements: already exists 稳定失败；limits/boundary/guard/provenance。  
Non-goals: write existing。  
Tests required: create/existing/missing parent/junction/cancel。  
Evidence required: focused tests。  
Acceptance criteria: 已存在文件字节完全不变。  
Rollback / failure behavior: 失败不留下目标或半文件。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P2C-007～P2C-011

## P2C-007 — write_text_file

Phase: Phase 2C  
Type: implementation  
Goal: 实现带 expectedSha256 的完整文本覆盖。  
Why now: 验证 OCC/atomic replace 主路径。  
Dependencies: P2C-001～P2C-005 (H)  
Blocked by: None  
Allowed scope: write handler/tests。  
Forbidden scope: 无 SHA overwrite、binary 写入。  
Contract references: §13.2；§13.9；§13.12～§13.15  
Implementation requirements: 锁内 SHA/Lease/path 重验；保留 newline 政策。  
Non-goals: line edits。  
Tests required: success/stale/missing SHA/external edit/limits/crash。  
Evidence required: focused tests。  
Acceptance criteria: stale 版本永不覆盖。  
Rollback / failure behavior: conflict 返回稳定错误，原文件不变。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2C-006、P2C-008～P2C-011

## P2C-008 — insert_lines

Phase: Phase 2C  
Type: implementation  
Goal: 实现 1-based insertion 与 N+1 append。  
Why now: 独立固定插入边界和空输入。  
Dependencies: P2C-001～P2C-005 (H)  
Blocked by: None  
Allowed scope: insert handler/tests。  
Forbidden scope: delete/replace 行为。  
Contract references: §13.4～§13.5；§13.11～§13.15  
Implementation requirements: empty input invalid；OCC/guard/atomic replace。  
Non-goals: 多文件 edit。  
Tests required: first/middle/N+1/out-of-range/empty/stale SHA。  
Evidence required: focused tests。  
Acceptance criteria: 行号行为与冻结 contract 完全一致。  
Rollback / failure behavior: 非法范围不写文件。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P2C-006～P2C-007、P2C-009～P2C-011

## P2C-009 — delete_lines

Phase: Phase 2C  
Type: implementation  
Goal: 实现 1-based inclusive 删除。  
Why now: 删除全部行和空文件结果需独立验证。  
Dependencies: P2C-001～P2C-005 (H)  
Blocked by: None  
Allowed scope: delete handler/tests。  
Forbidden scope: 删除文件本身。  
Contract references: §13.6；§13.11～§13.15  
Implementation requirements: 删除 1..N 得到空文件；OCC/atomic replace。  
Non-goals: replace range。  
Tests required: 3..5、1..N、invalid range、stale SHA。  
Evidence required: focused tests。  
Acceptance criteria: closed range 无 off-by-one。  
Rollback / failure behavior: 失败保留原内容。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P2C-006～P2C-008、P2C-010～P2C-011

## P2C-010 — replace_lines

Phase: Phase 2C  
Type: implementation  
Goal: 实现 inclusive range 替换与空 replacement 删除。  
Why now: 与字符串替换的歧义规则不同。  
Dependencies: P2C-001～P2C-005 (H)  
Blocked by: None  
Allowed scope: replace-lines handler/tests。  
Forbidden scope: content match 模式。  
Contract references: §13.7；§13.11～§13.15  
Implementation requirements: 先按 `delete_lines` 移除闭区间及其 trailing separator，再在原 `startLine` 按 `insert_lines` separator ownership 插入；非空 replacement 使用修改前 snapshot newline style 规范化，empty content=同 range 删除；OCC/atomic replace。range 到 EOF 时不隐式保留原 final newline。
Non-goals: formatter。  
Tests required: first/middle/end/empty/invalid/stale；empty 与 delete byte-equivalent；LF/CRLF/mixed；EOF 四组（原文件有/无 final newline × replacement 有/无 terminal newline）。
Evidence required: focused tests。  
Acceptance criteria: 只修改指定闭区间；EOF 结果严格遵循 delete+insert 组合，不隐式保留 final newline。
Rollback / failure behavior: 失败不提交。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P2C-006～P2C-009、P2C-011

## P2C-011 — replace_content

Phase: Phase 2C  
Type: implementation  
Goal: 实现 `first/all`、expectedMatches 和空 oldContent 约束。  
Why now: 这是六 Tool 中歧义风险最高的独立行为。  
Dependencies: P2C-001～P2C-005 (H)  
Blocked by: None  
Allowed scope: replace-content handler/tests。  
Forbidden scope: 正则扩展、新 mode。  
Contract references: §13.10、§13.10 first/all；§13.12～§13.15  
Implementation requirements: oldContent 空→invalid；all 无 max→invalid；数量不符→ambiguous。  
Non-goals: regex replace。  
Tests required: first/all/no match/multi match/expectedMatches/stale。  
Evidence required: focused tests。  
Acceptance criteria: 每种歧义均确定性失败。  
Rollback / failure behavior: 匹配不确定时不写。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2C-006～P2C-010

## P2C-012 — Remote Source Write Disabled Policy

Phase: Phase 2C  
Type: implementation  
Goal: 默认不向 Remote MCP 暴露直接 Source Write。  
Why now: Local 实现完成不等于 Remote 授权。  
Dependencies: P2C-006～P2C-011 (H)  
Blocked by: None  
Allowed scope: MCP registry/feature exposure tests。  
Forbidden scope: 新 enable 机制、token/permission 设计。  
Contract references: §14～§14.1；§50 Source  
Implementation requirements: 默认 schema/list/call 均 unavailable。  
Non-goals: 实现 Remote enable。  
Tests required: remote list/call disabled，Local IPC 可用。  
Evidence required: MCP contract tests。  
Acceptance criteria: 默认 Remote 无写 Tool。  
Rollback / failure behavior: exposure 不确定时继续禁用。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

## P2C-013 — Source Write OCC/Boundary/Crash Gate

Phase: Phase 2C  
Type: integration-test  
Goal: 关闭 Phase 2C Gate。  
Why now: 各 Tool 需共享并发与崩溃证据。  
Dependencies: P2C-001～P2C-012 (H)  
Blocked by: None  
Allowed scope: Source write integration/child-process tests。  
Forbidden scope: Provider/Agent 状态机修改。  
Contract references: §52 Phase 2C；§53 Source Write；§56.15～§20、§96～§98  
Implementation requirements: same-target OCC、different-target 并发、remove race、junction、真实 crash。  
Non-goals: 跨进程锁保证。  
Tests required: 完整 Source Write matrix。  
Evidence required: deterministic totals 和 filesystem snapshots。  
Acceptance criteria: 所有 Gate PASS；无半文件。  
Rollback / failure behavior: 失败时可整体 disable Write，Read/Agent 保持可用。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

---

# Phase 2D — Git / CodeGraph / Capability Health

## P2D-001 — Source Adapter 收敛到 WorkspaceCapabilityProvider

Phase: Phase 2D  
Type: implementation  
Goal: 用通用 Provider Port 包装已完成的 Rust Source。  
Why now: 完成最终 Capability routing，不改 Source 业务语义。  
Dependencies: P2C-013、P2A3-013 (H)  
Blocked by: None  
Allowed scope: Source adapter registration/routing/tests。  
Forbidden scope: 改四读六写契约。  
Contract references: §10.7；§12～§13；§52 Phase 2D  
Implementation requirements: `call` 显式接收 Lease；无 providerId 特判。  
Non-goals: Git/CodeGraph。  
Tests required: third-provider registry、Source regression、lease mismatch。  
Evidence required: adapter tests。  
Acceptance criteria: Source 业务测试原样通过。  
Rollback / failure behavior: adapter 可单独 unregister。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: P2D-002、P2D-004

## P2D-002 — Lease-rooted Git Provider Adapter 收敛

Phase: Phase 2D  
Type: implementation  
Goal: 将 2A.2 已经 Lease-rooted 的 Git backend 包装进 WorkspaceCapabilityProvider。  
Why now: 完成最终 Adapter 收敛，不在 2D 才首次切换 Git Authority。  
Dependencies: P2A3-013、P2A2-009、P2A2-013 (H)  
Blocked by: None  
Allowed scope: `mcp/git.rs`、Git adapter/registry/tests。  
Forbidden scope: 首次实现 Git Lease route、caller root、Git 功能扩展。  
Contract references: §10.4；§10.7；§52 Phase 2D  
Implementation requirements: Provider.call 显式接收 Lease；保留 `git -C lease.canonicalRoot` 和非 Git 稳定错误。  
Non-goals: 重新迁移 Git 请求 Schema 或 path 语义。  
Tests required: A/B cwd、missing/unknown ID、non-Git、cancel。  
Evidence required: command invocation tests。  
Acceptance criteria: Adapter 切换前后 Git Root/Tool 行为相同且只来自 Lease。  
Rollback / failure behavior: 无安全 Lease 时 Git unavailable。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2D-001、P2D-004

## P2D-003 — Git Adapter Path Contract Preservation

Phase: Phase 2D  
Type: contract-test  
Goal: 验证 Provider Adapter 收敛后仍保留 2A.2 的 Git Workspace-relative `path` 语义。  
Why now: 避免 Adapter 封装绕过已落地的 WorkspacePathResolver。  
Dependencies: P2D-002、P2A2-009 (H)  
Blocked by: None  
Allowed scope: Git Adapter path contract tests。  
Forbidden scope: 将字段改为 `relative_path`。  
Contract references: §10.4；§10.7；§51.2  
Implementation requirements: 不重复实现 path resolver；验证 optional path、absolute/UNC/escape 的既有结果保持。  
Non-goals: 改 Git 输出。  
Tests required: diff/log valid path、absolute/UNC/`..`/junction。  
Evidence required: focused tests。  
Acceptance criteria: 公共字段仍为 `path`，Adapter 不绕过 2A.2 path validation。  
Rollback / failure behavior: Gate 失败回退 Adapter 封装，保留已安全的 Git Lease backend。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: P2D-004

## P2D-004 — CodeGraph Adapter Shell 与 Readiness

Phase: Phase 2D  
Type: implementation  
Goal: 注册 CodeGraph capability，使用 `status --json` 判定 readiness。  
Why now: index 目录存在不能代替可信状态；2A.2 已停用旧 Global Active route。  
Dependencies: P2A3-002、P2A2-010、P0-006 (H/E)  
Blocked by: CodeGraph CLI evidence  
Allowed scope: CodeGraph adapter/probe/tests。  
Forbidden scope: query 时自动 init/sync/rebuild。  
Contract references: §8.2 CodeGraph Readiness；§41～§42  
Implementation requirements: 校验 initialized/projectPath/index.state 和 canonical Root。  
Non-goals: RuntimeSlot/process。  
Tests required: missing/not_initialized/ready/stale/error/root mismatch。  
Evidence required: JSON fixture tests。  
Acceptance criteria: readiness 不只检查 `.codegraph/`。  
Rollback / failure behavior: capability unavailable/error，不影响 Core。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: P2D-001～P2D-003

## P2D-005 — CodeGraph Explicit Init/Sync/Rebuild Actions

Phase: Phase 2D  
Type: implementation  
Goal: 实现仅 Local Human 可触发的 index actions。  
Why now: 准备副作用必须与普通 query 分离。  
Dependencies: P2D-004、P0-006 (H/E)  
Blocked by: CodeGraph command evidence  
Allowed scope: capability prepare actions、Local IPC、tests。  
Forbidden scope: Remote query 隐式执行、自动定时 rebuild。  
Contract references: §8.2；§41～§42；§51.4  
Implementation requirements: init/sync/rebuild 明确 action；single-flight progress。  
Non-goals: 新权限系统。  
Tests required: explicit authority、duplicate action、remote denial、error projection。  
Evidence required: command/action tests。  
Acceptance criteria: query 永不创建或更新 index。  
Rollback / failure behavior: action 失败保留现有 index 并标对应 stage。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2D-003

## P2D-006 — CodeGraph RuntimeSlot Integration 与 Capacity

Phase: Phase 2D  
Type: implementation  
Goal: 将 CodeGraph process/client 接入通用 Slot、single-flight、LRU、idle timeout。  
Why now: 完成 A/B runtime 隔离和容量策略。  
Dependencies: P2D-004、P2A3-003～P2A3-006、P0-007 (H/E)  
Blocked by: CodeGraph multi-process evidence  
Allowed scope: CodeGraph runtime adapter/manager tests。  
Forbidden scope: retarget live process、query 自动建索引。  
Contract references: §10.7～§10.9；§42；§52 Phase 2D  
Implementation requirements: all in-flight→`CODEGRAPH_BUSY`；crash 只影响目标 Slot。  
Non-goals: Health UI。  
Tests required: same-workspace single-flight、A/B、capacity、LRU、idle、crash。  
Evidence required: process identity tests。  
Acceptance criteria: Workspace 隔离硬保证，跨 Workspace 并发取决于 capacity。  
Rollback / failure behavior: 单独 unregister CodeGraph；不恢复 global Binding。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2D-005

## P2D-007 — Capability Health UI

Phase: Phase 2D  
Type: UI  
Goal: 动态展示 core/providers 的 status/readiness/runtime/stages/actions。  
Why now: 后端统一 DTO 已稳定。  
Dependencies: P2A3-013、P2D-004～P2D-006 (H)  
Blocked by: None  
Allowed scope: Project/Capability UI、types/api/page tests。  
Forbidden scope: 写死 Serena/CodeGraph ID、修改 Manager contract。  
Contract references: §41；§52 Phase 2D  
Implementation requirements: 三维状态正交；动作来自 Descriptor；不显示 PID/port/root。  
Non-goals: Agent UI。  
Tests required: unavailable/not_prepared/stopped/error、fake 第三 Provider、action feedback。  
Evidence required: frontend tests/screenshots。  
Acceptance criteria: 新 Provider 无需新增 UI 分支。  
Rollback / failure behavior: 某 Provider 失败不把 Desktop 整体标红。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: P2D-008

## P2D-008 — Capability Remove/Shutdown Cleanup

Phase: Phase 2D  
Type: integration-test  
Goal: 覆盖 Source/Git/Serena/CodeGraph 的 Remove 和 Host shutdown 收敛。  
Why now: 所有 built-in adapter 到位后验证 ownership。  
Dependencies: P2D-001～P2D-006 (H)  
Blocked by: None  
Allowed scope: capability integration/shutdown tests。  
Forbidden scope: 新后台清理机制。  
Contract references: §10.9；§51.4；§55 Capability Runtime  
Implementation requirements: remove 阻止 acquire/in-flight；stop 失败保留 entry；无 orphan。  
Non-goals: installer process 测试。  
Tests required: concurrent remove/call/shutdown、stop failure。  
Evidence required: process/registry assertions。  
Acceptance criteria: 所有 live provider runtime 均可收敛。  
Rollback / failure behavior: cleanup 不确定时保留 handle/Registry。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P2D-007

## P2D-009 — Multi-workspace Capability Integration Gate

Phase: Phase 2D  
Type: integration-test  
Goal: 关闭 Phase 2D Gate。  
Why now: 最终 Capability Platform 必须整体证明不串线。  
Dependencies: P2D-001～P2D-008 (H)  
Blocked by: P0-006、P0-007 (E)  
Allowed scope: MCP/Capability/Health integration tests。  
Forbidden scope: Activity/Usage 实现。  
Contract references: §52 Phase 2D；§53 Capability Runtime/Git；§56.68～§94  
Implementation requirements: Source/Git/Serena/CodeGraph 均先 resolve Lease；CodeGraph 只在新 Adapter 的 readiness/runtime/call 均可用后恢复 advertise；capacity 语义统一。  
Non-goals: Manual E2E。  
Tests required: Phase 2D 完整 Gate。  
Evidence required: A/B trace、busy/LRU/health 结果。  
Acceptance criteria: 所有目标 Provider 无 global process retarget。  
Rollback / failure behavior: 单 Provider 可禁用，不恢复隐式 Authority。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

---

---

# Phase 3 — Agent Activity Observe

## P3-001 — summaryCode Deterministic Mapping

Phase: Phase 3  
Type: implementation  
Goal: 实现唯一纯函数 `derive_summary_code`。  
Why now: Store/Observe/UI 必须共享同一语义。  
Dependencies: P2D-009 (H)  
Blocked by: None  
Allowed scope: `agent/activity.rs`、pure tests。  
Forbidden scope: Store/Observe/UI。  
Contract references: §24～§24.1；§29  
Implementation requirements: finalizing/reconciling 优先级固定；只输出 allowlist code。  
Non-goals: 自由文本摘要。  
Tests required: 全输入组合表。  
Evidence required: table-driven tests。  
Acceptance criteria: 任一输入组合只有一个结果。  
Rollback / failure behavior: 未知或非法 Activity 组合返回 `AGENT_ACTIVITY_CONTRACT_ERROR`，不得猜测 generic summaryCode。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: P3-002

## P3-002 — Activity Schema Migration

Phase: Phase 3  
Type: migration  
Goal: 增加 summary、activity_sequence 与 bounded history 表。  
Why now: Store update 前需要持久化结构。  
Dependencies: P0-003、P3-001 (H)  
Blocked by: None  
Allowed scope: 新 Agent schema migration、`store.rs` migration tests。  
Forbidden scope: lifecycle revision 语义修改、自动 prune。  
Contract references: §25～§27.1；§54.4  
Implementation requirements: sequence 非负；history `ON DELETE RESTRICT`；历史任务不伪造 history。  
Non-goals: 查询/observe。  
Tests required: old DB migration、empty history、constraints。  
Evidence required: schema tests。  
Acceptance criteria: 旧 Execution 可派生 current summary，history 保持空。  
Rollback / failure behavior: migration transaction 失败不升级版本。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## P3-003 — Activity Store Update 与 History Query

Phase: Phase 3  
Type: implementation  
Goal: semantic change 递增 sequence 并 append；heartbeat 只刷新时间。  
Why now: activity revision 和 observe 依赖权威 store。  
Dependencies: P3-002 (H)  
Blocked by: None  
Allowed scope: StateStore activity transactions/history query/tests。  
Forbidden scope: increment `executions.revision`、history prune。  
Contract references: §25～§27；§52 Phase 3  
Implementation requirements: Activity 与 lifecycle CAS 解耦；query 有 server limit+cursor。  
Non-goals: MCP Observe。  
Tests required: semantic change/heartbeat/concurrency/pagination。  
Evidence required: DB assertions。  
Acceptance criteria: activity-only update 不改变 execution revision。  
Rollback / failure behavior: observability 失败不改变 lifecycle。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## P3-004 — Observe Product Semantics

Phase: Phase 3  
Type: implementation  
Goal: 支持 knownActivityRevision、wakeOn、wakeReason、snapshot coalescing。  
Why now: Store 语义稳定后接入 long-poll。  
Dependencies: P3-003 (H)  
Blocked by: None  
Allowed scope: product observe/control tests。  
Forbidden scope: 改公共 wait 上限、逐事件流。  
Contract references: §28.1～§28.9  
Implementation requirements: activity mode 仍因 control/terminal/result 唤醒；initial mismatch 立即返回。  
Non-goals: MCP schema/description。  
Tests required: control/activity/timeout/result/mismatch/coalescing/disconnect。  
Evidence required: observe tests。  
Acceptance criteria: `unchanged` 仍只描述 control revision。  
Rollback / failure behavior: 可临时回 control-only，但保留 schema/data。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## P3-005 — Work Adapter 与 MCP Activity Contract

Phase: Phase 3  
Type: migration  
Goal: 将 Activity Observe 参数与结果完整穿过 Work Adapter/MCP。  
Why now: 防止 schema 有字段但 adapter 丢弃。  
Dependencies: P3-004、P0-002 (H)  
Blocked by: None  
Allowed scope: orchestration DTO、work adapter、registry descriptions/tests。  
Forbidden scope: UI。  
Contract references: §28；§39；§50 Activity  
Implementation requirements: waitMs 默认 15000、0..=20000；非法值稳定错误；description 说明不要只看 unchanged。  
Non-goals: Activity history UI。  
Tests required: schema→adapter→product E2E、20001 invalid、v1 token mismatch。  
Evidence required: MCP tests/hash。  
Acceptance criteria: `wakeOn=activity` 真正到达 Observe。  
Rollback / failure behavior: 非法输入不启动等待。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P3-006

## P3-006 — Activity Privacy Gate

Phase: Phase 3  
Type: contract-test  
Goal: 证明 Activity 不持久化或投影敏感执行细节。  
Why now: Telemetry 开放前必须关闭泄漏边界。  
Dependencies: P3-003、P1-007 (H)  
Blocked by: None  
Allowed scope: Activity/event/product tests。  
Forbidden scope: 新安全机制、修改 Provider 命令执行。  
Contract references: §23；§29；§51.5  
Implementation requirements: 拒绝 reasoning/argv/stdout/stderr/env/prompt/diff/source。  
Non-goals: 日志系统重构。  
Tests required: forged event、serialization、DB/product inspection。  
Evidence required: privacy regression results。  
Acceptance criteria: public/store 只出现 allowlisted 语义字段。  
Rollback / failure behavior: 不安全 event drop，不影响 lifecycle。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: P3-005

## P3-007 — Activity Observe Integration Gate

Phase: Phase 3  
Type: integration-test  
Goal: 关闭 Phase 3 Gate。  
Why now: DB、Product、MCP 需联合验证。  
Dependencies: P3-001～P3-006 (H)  
Blocked by: None  
Allowed scope: Agent product/MCP integration tests。  
Forbidden scope: Usage。  
Contract references: §52 Phase 3；§53 Activity；§56.28～§36、§99～§100  
Implementation requirements: finalizing、heartbeat、revision 解耦、wake、privacy、disconnect。  
Non-goals: UI。  
Tests required: 完整 Activity matrix。  
Evidence required: totals 和 revision snapshots。  
Acceptance criteria: Phase 3 Gate 全部 PASS。  
Rollback / failure behavior: 失败阻止 Phase 4。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

---

---

# Phase 4 — Usage

## P4-001 — Public Usage Domain 与 Validation

Phase: Phase 4  
Type: implementation  
Goal: 定义 UsageSnapshot、Completeness、revision 及严格整数校验。  
Why now: DB 和 Codex parser 需要稳定公共模型。  
Dependencies: P3-007、P0-008 (H/E)  
Blocked by: Codex Usage contract evidence  
Allowed scope: Agent usage domain/tests。  
Forbidden scope: Provider 私有 identity、公共层计算 total。  
Contract references: §30～§31.1；§50 Usage  
Implementation requirements: null≠0；integer 范围严格；total 只接受 Provider 值。  
Non-goals: DB/parser。  
Tests required: null/zero/float/negative/string/overflow。  
Evidence required: unit tests。  
Acceptance criteria: 非法数字返回 `USAGE_EVENT_INVALID`。  
Rollback / failure behavior: 无可信 usage 时保持 unknown/null。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: None

## P4-002 — Usage Database Migration

Phase: Phase 4  
Type: migration  
Goal: 增加公共 execution_usage 与 Codex private checkpoint/state 表。  
Why now: parser 前先建立原子持久化边界。  
Dependencies: P4-001、P0-003 (H)  
Blocked by: None  
Allowed scope: 新 Agent schema migration、store records/tests。  
Forbidden scope: Activity/lifecycle schema 改动、历史补 0。  
Contract references: §37～§38；§54.5  
Implementation requirements: CHECK/FK 完整；旧 Execution 无行→unknown。  
Non-goals: update 算法。  
Tests required: migrate/constraints/old history/restart。  
Evidence required: DB tests。  
Acceptance criteria: 公共与 Provider-private 表清晰分离。  
Rollback / failure behavior: migration 失败原子回滚。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## P4-003 — Codex Usage Event Parser 与 Identity Binding

Phase: Phase 4  
Type: implementation  
Goal: 按 pinned wire contract 解析累计 usage 并发布安全 event。  
Why now: 只有 Provider Adapter 能理解 Codex 字段。  
Dependencies: P4-001、P1-007、P0-008 (H/E)  
Blocked by: Codex wire evidence  
Allowed scope: `agent/codex/protocol.rs`、provider telemetry tests。  
Forbidden scope: 公共层读取 thread/turn、silent cast/clamp。  
Contract references: §23；§31；§38  
Implementation requirements: publish 前校验 execution/runtime/thread/turn。  
Non-goals: delta/store lifecycle。  
Tests required: valid/invalid/duplicate/out-of-order/wrong identity。  
Evidence required: pinned wire fixture tests。  
Acceptance criteria: 不可信 event 无法进入 Store。  
Rollback / failure behavior: invalid event 丢弃并诊断，不影响 Execution。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P4-002

## P4-004 — Baseline Capture、Delta 与 Continue

Phase: Phase 4  
Type: implementation  
Goal: 实现原子 nullable baseline 和可信 delta。  
Why now: Continue Usage 不能用旧 checkpoint 猜测。  
Dependencies: P4-002、P4-003 (H)  
Blocked by: P0-008 terminal/checkpoint evidence  
Allowed scope: Usage store service、Codex checkpoint adapter/tests。  
Forbidden scope: partial field subtraction、`current-0`。  
Contract references: §32～§33；§38  
Implementation requirements: participating fields 全 known 才算 delta；Continue 优先同步 checkpoint。  
Non-goals: terminal grace。  
Tests required: fresh/continue/missing/null/stale/different lineage/restart。  
Evidence required: baseline/delta DB assertions。  
Acceptance criteria: 不完整 baseline 产生全 unknown delta。  
Rollback / failure behavior: 证据不足降级 unknown，不影响执行。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## P4-005 — Terminal Coverage、Grace、Freeze 与 Late Events

Phase: Phase 4  
Type: implementation  
Goal: 实现 accepting→terminal_grace→frozen telemetry lifecycle。  
Why now: Execution terminal 与 Usage complete 必须解耦。  
Dependencies: P4-003～P4-004 (H)  
Blocked by: P0-008 terminal evidence  
Allowed scope: Usage store/projector、runtime teardown hook、tests。  
Forbidden scope: 延迟 Claim release、修改 Execution terminal。  
Contract references: §34～§36  
Implementation requirements: 2000ms grace；teardown/expiry/final checkpoint 冻结；late identity 严格。  
Non-goals: UI。  
Tests required: partial/complete、+1s accepted、expired/teardown rejected、duplicate/regression。  
Evidence required: deterministic clock tests。  
Acceptance criteria: Usage 错误不影响 Claim/terminal。  
Rollback / failure behavior: 不确定 coverage 保持 partial；frozen event drop。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## P4-006 — Usage Product Projection

Phase: Phase 4  
Type: implementation  
Goal: 将 Usage 及列表摘要投影到 Execution Product DTO。  
Why now: UI 必须消费后端事实且避免 N+1。  
Dependencies: P4-002、P4-005 (H)  
Blocked by: None  
Allowed scope: `agent/product.rs`、history/list queries、DTO/types tests。  
Forbidden scope: UI 相加 total、读取 Codex private identity。  
Contract references: §39；§40；§51.6  
Implementation requirements: detail 完整 usage；list 返回 total/completeness/providerId。  
Non-goals: 前端显示。  
Tests required: absent/unknown/partial/complete/zero/historical/list query count。  
Evidence required: DTO snapshots。  
Acceptance criteria: 无 usage 行投影 unknown/null。  
Rollback / failure behavior: 可隐藏 UI 字段但不能回填 0。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: None

## P4-007 — Usage Regression Gate

Phase: Phase 4  
Type: integration-test  
Goal: 关闭 Phase 4 Gate。  
Why now: wire、DB、delta、terminal 需联合证明。  
Dependencies: P4-001～P4-006 (H)  
Blocked by: P0-008 (E)  
Allowed scope: Codex/provider/product integration tests。  
Forbidden scope: UI。  
Contract references: §52 Phase 4；§53 Usage；§56.37～§42、§101  
Implementation requirements: fresh/continue/restart/null/duplicate/regression/coverage/grace/freeze。  
Non-goals: Manual 真实 UI。  
Tests required: 完整 Usage matrix。  
Evidence required: test totals 和 pinned contract 引用。  
Acceptance criteria: 所有 Gate PASS 且 public 不推导 total。  
Rollback / failure behavior: 失败阻止 Phase 5；Execution 功能保持可用。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

---

---

# Phase 5 — Agent UI

## P5-001 — Provider Display

Phase: Phase 5  
Type: UI  
Goal: 从 Provider Descriptor 展示名称、版本和 session label。  
Why now: Product DTO 已稳定。  
Dependencies: P4-007 (H)  
Blocked by: None  
Allowed scope: Agent frontend types/presentation/detail/list tests。  
Forbidden scope: Runtime/DB/MCP contract、写死 Codex 名称。  
Contract references: §16；§21.1；§39～§40  
Implementation requirements: 历史 opaque 字段仅 display。  
Non-goals: Activity/Usage。  
Tests required: known/unknown provider、missing version、historical task。  
Evidence required: frontend tests。  
Acceptance criteria: Provider 名称来自 Descriptor。  
Rollback / failure behavior: 缺 descriptor 显示安全 fallback。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: P5-002～P5-004

## P5-002 — Activity Display

Phase: Phase 5  
Type: UI  
Goal: 展示 summary、最近活动和 silence 状态，不推断 stalled。  
Why now: Activity DTO 稳定。  
Dependencies: P4-007、P3-007 (H)  
Blocked by: None  
Allowed scope: `ExecutionDetails.tsx`、presentation helpers/tests。  
Forbidden scope: 修改 Observe、显示 raw command/reasoning。  
Contract references: §24；§29；§39～§40  
Implementation requirements: summaryCode 映射固定文案；null 安全。  
Non-goals: Activity history 完整 UI。  
Tests required: phases、null、age buckets、historical task。  
Evidence required: frontend tests/screenshots。  
Acceptance criteria: UI 不把 quiet/prolonged 解释为失败。  
Rollback / failure behavior: 缺 activity 显示 unknown。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: P5-001、P5-003～P5-004

## P5-003 — Task Detail Usage

Phase: Phase 5  
Type: UI  
Goal: 在详情展示 Total、breakdown、context window、completeness。  
Why now: Usage Product 已稳定。  
Dependencies: P4-007 (H)  
Blocked by: None  
Allowed scope: `ExecutionDetails.tsx`、format helper/tests。  
Forbidden scope: 相加 breakdown、后端 contract 修改。  
Contract references: §30；§39；§40.1  
Implementation requirements: null→`—`；0→`0`；partial 明确标注。  
Non-goals: 列表 hover。  
Tests required: unknown/partial/complete/zero/large numbers。  
Evidence required: frontend tests/screenshots。  
Acceptance criteria: Total 严格使用 `totalTokens`。  
Rollback / failure behavior: 字段缺失显示 unknown。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: P5-001～P5-002、P5-004

## P5-004 — Task List/Hover Usage

Phase: Phase 5  
Type: UI  
Goal: 列表和 hover 展示 Provider 及 total usage 且无 N+1。  
Why now: list summary 字段已稳定。  
Dependencies: P4-006、P5-001 (H)  
Blocked by: None  
Allowed scope: `AgentPanel.tsx`、`ProjectTaskNavigation.tsx`、tests。  
Forbidden scope: hover 请求 detail、修改 history API。  
Contract references: §40.2  
Implementation requirements: unknown/partial/complete 格式一致。  
Non-goals: 详情布局。  
Tests required: hover 内容、分页、query-count/no detail call。  
Evidence required: frontend request tests。  
Acceptance criteria: hover 只消费 list payload。  
Rollback / failure behavior: 缺 summary 显示 `总 Token：—`。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: P5-002～P5-003

## P5-005 — UI Compatibility 与 Phase 5 Gate

Phase: Phase 5  
Type: integration-test  
Goal: 验证 Provider/Activity/Usage 在历史和新 Execution 上的完整显示。  
Why now: 后端/UI 边界需最终验收。  
Dependencies: P5-001～P5-004 (H)  
Blocked by: P0-010  
Allowed scope: 前端 contract/component tests。  
Forbidden scope: Runtime/DB 修改。  
Contract references: §52 Phase 5；§56.43～§45  
Implementation requirements: unknown/partial/complete/zero/historical/no N+1。  
Non-goals: Installer。  
Tests required: `npm test`、lint/build。  
Evidence required: totals/screenshots。  
Acceptance criteria: Phase 5 Gate 全部 PASS。  
Rollback / failure behavior: UI 可隐藏新字段，不能伪造数据。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: None

---

---

# Phase 6 — Installer / Release

## P6-001 — Tauri NSIS currentUser/WebView2 Bundle

Phase: Phase 6  
Type: release  
Goal: 启用 NSIS x64 current-user installer 和 downloadBootstrapper。  
Why now: 正式发布不再使用 bare exe。  
Dependencies: P5-005、P0-009 (H/E)  
Blocked by: Installer environment evidence  
Allowed scope: `tauri.conf.json`、必要 bundle resources、focused build verification。  
Forbidden scope: offline installer、新安装器框架。  
Contract references: §43～§43.2  
Implementation requirements: bundle active；targets nsis；currentUser；silent WebView2 bootstrapper。  
Non-goals: CI 发布。  
Tests required: local NSIS build、config validation。  
Evidence required: installer artifact 和 metadata。  
Acceptance criteria: 生成可运行 NSIS installer。  
Rollback / failure behavior: 不发布 bare exe 替代。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P6-002～P6-005

## P6-002 — Portable/Installed Shared Single-instance

Phase: Phase 6  
Type: implementation  
Goal: 确保 portable 和 installed 共享固定 single-instance identity。  
Why now: 防止两个 Host 同时写 StateStore。  
Dependencies: P0-009、P5-005 (H/E)  
Blocked by: Windows baseline  
Allowed scope: Tauri single-instance setup、`lib.rs`、tests。  
Forbidden scope: 新 data migration 子系统。  
Contract references: §45；§51；§56.51、§107  
Implementation requirements: 后启动者在打开 StateStore writer 前退出/聚焦已有实例。  
Non-goals: 跨用户实例协调。  
Tests required: portable→installed、installed→portable。  
Evidence required: process/DB writer 证据。  
Acceptance criteria: 两个方向都只有一个 Host。  
Rollback / failure behavior: 无法证明时阻止 installer 发布。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P6-001、P6-003～P6-005

## P6-003 — Shared Config/Data Path Verification

Phase: Phase 6  
Type: contract-test  
Goal: 验证固定 identifier 下 portable/installed 解析同一 config/data 路径。  
Why now: 设计明确不新增迁移系统。  
Dependencies: P0-009、P6-001 (H/E)  
Blocked by: 可安装构建  
Allowed scope: path probes/integration tests。  
Forbidden scope: 搬移用户数据、改变 identifier。  
Contract references: §45；§54.6  
Implementation requirements: 覆盖 config、agent-state、OAuth state。  
Non-goals: 数据清理。  
Tests required: portable/installed 同用户路径比较、中文用户名。  
Evidence required: resolved path 记录。  
Acceptance criteria: 路径完全一致。  
Rollback / failure behavior: 不一致则阻止发布并进入 DCR。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: P6-002、P6-004～P6-005

## P6-004 — Installed Autostart Path

Phase: Phase 6  
Type: implementation  
Goal: 安装版 autostart 指向 installed executable。  
Why now: 升级后不能继续启动旧 portable。  
Dependencies: P6-001 (H)  
Blocked by: None  
Allowed scope: autostart setup/commands/tests。  
Forbidden scope: 删除 portable 文件、改变用户开关。  
Contract references: §46  
Implementation requirements: 安装/升级后刷新路径且保持 enable 状态。  
Non-goals: 新后台服务。  
Tests required: enable/disable、upgrade path、missing old portable。  
Evidence required: registry/autostart target。  
Acceptance criteria: 启动项不引用旧 portable path。  
Rollback / failure behavior: 更新失败保留用户可见错误，不创建双条目。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P6-002～P6-003、P6-005

## P6-005 — Product Version Consistency Gate

Phase: Phase 6  
Type: release  
Goal: 强制 tauri/Cargo/npm/tag 版本一致。  
Why now: 正式 artifact 必须有单一产品版本。  
Dependencies: P5-005 (H)  
Blocked by: None  
Allowed scope: version-check script/CI config/tests。  
Forbidden scope: 自动发布新版本、替换 tag。  
Contract references: §44；§48  
Implementation requirements: tauri.conf 为 authority；tag 格式 `vX.Y.Z`。  
Non-goals: changelog/release notes 自动化。  
Tests required: match/mismatch/malformed tag。  
Evidence required: version gate output。  
Acceptance criteria: 任一不一致 CI 失败。  
Rollback / failure behavior: 不发布 artifact。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: P6-001～P6-004

## P6-006 — GitHub Actions Quality、NSIS Artifact 与 Uninstall Policy

Phase: Phase 6  
Type: release  
Goal: 将 release workflow 切到完整 quality gates 和 NSIS artifact。  
Why now: 本地 installer 与版本 Gate 稳定后更新发布链。  
Dependencies: P6-001、P6-005、P0-010 (H)  
Blocked by: None  
Allowed scope: `.github/workflows/release.yml`、`src-tauri/build.rs`、installer verification。  
Forbidden scope: 上传 bare exe、跳过失败 Gate。  
Contract references: §47～§49  
Implementation requirements: 删除 `--no-bundle`；验证 installer；uninstall 保留 AppData。  
Non-goals: 删除全部用户数据选项。  
Tests required: workflow syntax、local command parity、uninstall fixture。  
Evidence required: CI dry-run/actual tagged build evidence。  
Acceptance criteria: Release 只附 NSIS installer。  
Rollback / failure behavior: 撤销 Release 并使用新版本/tag 修复，不替换原 tag。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: P6-002～P6-004

## P6-007 — Clean Windows Installer Gate

Phase: Phase 6  
Type: integration-test  
Goal: 关闭 Phase 6 自动化和安装环境 Gate。  
Why now: Phase 7 前必须确认 artifact 可安装。  
Dependencies: P6-001～P6-006 (H)  
Blocked by: P0-009 (E)  
Allowed scope: clean Windows VM、installer/uninstaller evidence。  
Forbidden scope: 测试中修复业务代码。  
Contract references: §43～§49；§52 Phase 6；§53 Installer  
Implementation requirements: WebView2 present/online absent/offline limitation、currentUser、中文用户名、data preservation。  
Non-goals: 全产品 E2E。  
Tests required: install/launch/upgrade/uninstall/version/artifact。  
Evidence required: installer log、screenshots、paths、hash。  
Acceptance criteria: Phase 6 Gate 全部 PASS。  
Rollback / failure behavior: 任一失败阻止 Release/Phase 7 正式验收。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

---

---

# Phase 7 — Manual E2E Acceptance

以下任务均为只读/人工验收，不写业务代码。

## P7-001 — Existing User Upgrade

Phase: Phase 7  
Type: manual-e2e  
Goal: 验证旧用户升级后项目和数据保持。  
Why now: 首个真实升级入口。  
Dependencies: P6-007 (H)  
Blocked by: 旧版本用户 fixture  
Allowed scope: 隔离 Windows 用户/VM。  
Forbidden scope: 修改数据修复结果。  
Contract references: §54；§56.3～§4  
Implementation requirements: Precondition 旧项目/config/state；安装新版并启动。  
Non-goals: 新项目注册。  
Tests required: 升级前后 Registry/Execution/AppData 比较。  
Evidence required: config/state hash、UI 截图、日志。  
Acceptance criteria: 旧项目保留且 startup Serena sync 未覆盖；否则 FAIL。  
Rollback / failure behavior: 恢复 VM snapshot 并记录 FAIL。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

## P7-002 — Manual Register Non-Git Workspace 与 Discovery

Phase: Phase 7  
Type: manual-e2e  
Goal: 验证 Local Human 注册非 Git 目录并由 ChatGPT 发现 ID。  
Why now: Workspace Registry 主用户路径。  
Dependencies: P7-001 (H)  
Blocked by: None  
Allowed scope: 临时非 Git 目录。  
Forbidden scope: 手工编辑 config。  
Contract references: §8；§52 Phase 7  
Implementation requirements: picker→inspect→register→`workspace_list`。  
Non-goals: Provider 启动。  
Tests required: 注册后立即出现；selection/list 不启动 Runtime。  
Evidence required: UI、MCP 响应、process list。  
Acceptance criteria: 获得 workspaceId 且无 implicit activation/process；否则 FAIL。  
Rollback / failure behavior: UI remove 临时 entry，不删目录。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: None

## P7-003 — Request A/B Concurrent Isolation

Phase: Phase 7  
Type: manual-e2e  
Goal: 真实证明同一 ChatGPT 任务显式复用 A/B ID 且不串线。  
Why now: Workspace Authority 的核心验收。  
Dependencies: P7-002 (H)  
Blocked by: 两个可辨识 Workspace  
Allowed scope: 只读 Source/Git 请求。  
Forbidden scope: `workspace_activate` 作为前置。  
Contract references: §10.4～§10.7；§56.55～§65  
Implementation requirements: 一次 list 后并发 A Source 和 B Git/Source。  
Non-goals: Runtime capacity 压力。  
Tests required: 缺 ID 错误；已知 ID 不重复 list；每请求仍显式 ID。  
Evidence required: request/response transcript、文件 provenance。  
Acceptance criteria: 分别解析 A/B Lease 且无 Session/global binding；否则 FAIL。  
Rollback / failure behavior: 无状态回滚，记录 FAIL。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

## P7-004 — Serena Lazy Runtime 与 Capacity

Phase: Phase 7  
Type: manual-e2e  
Goal: 验证首次 Semantic 调用 auto-prepare、lazy start 和容量语义。  
Why now: Serena 不再是 startup 依赖。  
Dependencies: P7-003 (H)  
Blocked by: Serena installed；最终候选环境

Allowed scope: 临时 Workspace/Serena config。  
Forbidden scope: 手工预热 Runtime、global activate。  
Contract references: §10.9；§11；§41～§42  
Implementation requirements: selection 无进程；首次调用创建最小 config；不做 index/onboarding。  
Non-goals: CodeGraph。  
Tests required: same-workspace single-flight；A/B 使用独立 per-slot Home 并在容量允许时并发；容量满时按 LRU/BUSY 契约处理。

Evidence required: process/endpoint/root/activity。  
Acceptance criteria: 永不 retarget；行为符合 capacity 策略。  
Rollback / failure behavior: stop runtime/remove 临时 entry。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: P7-005～P7-006、P7-008

## P7-005 — CodeGraph Explicit Index Lifecycle

Phase: Phase 7  
Type: manual-e2e  
Goal: 验证普通 query 不初始化，Local action 才 init/sync/rebuild。  
Why now: Local Human Authority 最终验收。  
Dependencies: P7-003 (H)  
Blocked by: CodeGraph installed  
Allowed scope: 临时未初始化 Workspace。  
Forbidden scope: 预建 `.codegraph`。  
Contract references: §8.2；§41～§42  
Implementation requirements: query→not initialized；UI action→progress→ready→query。  
Non-goals: Serena index。  
Tests required: init、sync、rebuild、A/B capacity。  
Evidence required: status JSON、目录变化、UI/MCP 响应。  
Acceptance criteria: Remote query 无副作用；Local action 成功。  
Rollback / failure behavior: 删除临时 Workspace 目录。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: P7-004、P7-006、P7-008

## P7-006 — Rust Source Read Manual Acceptance

Phase: Phase 7  
Type: manual-e2e  
Goal: 真实验证四个 Rust Source Tool。  
Why now: 确认无 Serena 依赖和实际边界行为。  
Dependencies: P7-003 (H)  
Blocked by: 测试 Workspace fixture  
Allowed scope: 只读 fixture。  
Forbidden scope: 修改文件。  
Contract references: §12；§52 Phase 7  
Implementation requirements: 保留 `relative_path`；覆盖 limits/hidden/binary/junction/cancel/provenance。  
Non-goals: Write。  
Tests required: 四 Tool 各至少一成功一失败。  
Evidence required: MCP transcript 和 SHA。  
Acceptance criteria: Serena 停止时仍 PASS 且不越界。  
Rollback / failure behavior: 无状态回滚。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P7-004～P7-005、P7-008

## P7-007 — Source Write OCC Manual Acceptance

Phase: Phase 7  
Type: manual-e2e  
Goal: 验证六个 Write Tool、OCC 和 crash-safe 结果。  
Why now: 写能力需真实文件证据。  
Dependencies: P7-006 (H)  
Blocked by: 可丢弃 fixture  
Allowed scope: 临时 Workspace 文件。  
Forbidden scope: 用户真实项目、Remote direct write。  
Contract references: §13～§14  
Implementation requirements: capture SHA→write；stale conflict；same-target 竞争；different-target 并发。  
Non-goals: 跨进程锁保证。  
Tests required: 六 Tool、newline、junction、remove busy、crash fixture。  
Evidence required: before/after hashes、内容、错误码。  
Acceptance criteria: 至多一个同 SHA 写成功且无半文件。  
Rollback / failure behavior: 恢复/删除临时 fixture。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: P7-009

## P7-008 — Agent Execute 与 Continue Workspace Freeze

Phase: Phase 7  
Type: manual-e2e  
Goal: 验证 start 冻结 Workspace、Continue 继承且不接受新 ID。  
Why now: Agent Workspace 双 Authority 风险必须真实排除。  
Dependencies: P7-003 (H)  
Blocked by: Codex available  
Allowed scope: 临时 Workspace 和测试任务。  
Forbidden scope: 修改 Execution 状态机。  
Contract references: §5；§10.6；§52 Phase 7  
Implementation requirements: start(A) 后 UI 选 B；continue 不带 workspaceId；query/cancel 按 executionId。  
Non-goals: Usage 精度。  
Tests required: execute、continue、cancel、restart recovery。  
Evidence required: Execution snapshots、Claim/terminal evidence。  
Acceptance criteria: 所有相关 Execution 保持 A identity。  
Rollback / failure behavior: cancel 并等待权威 terminal/claim release。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: P7-004～P7-006

## P7-009 — Observe Activity Manual Acceptance

Phase: Phase 7  
Type: manual-e2e  
Goal: 验证真实任务的 activity wake 和安全摘要。  
Why now: deterministic 测试之外验证真实通知链。  
Dependencies: P7-008 (H)  
Blocked by: 可产生 read/edit/test 阶段的任务  
Allowed scope: Agent query transcript。  
Forbidden scope: 读取/展示 reasoning 或 raw command。  
Contract references: §24～§29  
Implementation requirements: knownActivityRevision、wakeOn=activity、wakeReason、timeout。  
Non-goals: Usage。  
Tests required: activity change、control change、heartbeat、terminal、disconnect。  
Evidence required: revision/wake transcript。  
Acceptance criteria: 正确唤醒且无敏感内容。  
Rollback / failure behavior: cancel 任务并记录 FAIL。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: P7-007

## P7-010 — Usage Manual Acceptance

Phase: Phase 7  
Type: manual-e2e  
Goal: 验证 fresh/continue/terminal/late usage 展示和持久化。  
Why now: 最终确认 pinned Codex wire 在真实运行中成立。  
Dependencies: P7-008、P7-009 (H)  
Blocked by: pinned Codex binary  
Allowed scope: 测试 Execution 和只读 DB/DTO 证据。  
Forbidden scope: 手工修正 token 数字、相加 total。  
Contract references: §30～§40  
Implementation requirements: 检查 unknown/partial/complete/zero；continue baseline；restart。  
Non-goals: Provider 计费解释。  
Tests required: terminal coverage、grace 内 late、freeze 后 drop。  
Evidence required: wire、DB、Product、UI 四层对照。  
Acceptance criteria: 数字和 completeness 符合 Provider Contract。  
Rollback / failure behavior: 删除测试任务仅按现有产品能力；不改历史 usage。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

## P7-011 — Installer/Uninstaller Final Manual Acceptance

Phase: Phase 7  
Type: manual-e2e  
Goal: 对最终发布 artifact 完成 clean Windows 全路径验收。  
Why now: Release 前最后一个 human gate。  
Dependencies: P7-001～P7-010、P6-007 (H/A)  
Blocked by: clean Windows VM 与候选 installer  
Allowed scope: 安装、升级、启动、自启、卸载。  
Forbidden scope: 现场修改 artifact 或替换同一 tag。  
Contract references: §43～§49；§52 Phase 7；§56  
Implementation requirements: currentUser、WebView2、single-instance、shared data、autostart、uninstall preserve data。  
Non-goals: offline WebView2 installer。  
Tests required: clean install、upgrade、portable 交叉启动、uninstall/reinstall。  
Evidence required: installer hash/log/screenshots/path/process/data。  
Acceptance criteria: §56 checklist 全部 PASS；否则不 Release。  
Rollback / failure behavior: 撤销候选 Release，以新版本号修复。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

---

---

# Dependency DAG

## 主实施链

```text
P0-001 ─H→ P1-001 → P1-002 → P1-003 → P1-004
                                      ├─H→ P1-005
                                      ├─H→ P1-006
                                      └─H→ P1-007
P0-010 ─H───────────────────────────────────┐
P1-005 + P1-006 + P1-007 ─H→ P1-008 ─H→ Phase 1 Gate
```

```text
Phase 1 Gate
  ├─H→ P2A1-001 ─┐
  └─H→ P2A1-002 ─┴→ P2A1-003
                         ├→ P2A1-006
                         ├→ P2A1-007
                         ├→ P2A1-008
                         ├→ P2A1-009
                         ├→ P2A1-010
                         └→ P2A1-011
P2A1-005 + P2A1-006..009 ─H→ P2A1-012
P2A1-011 ─S→ P2A1-012 (仅 Import UI 入口)
P2A1-001..010 + P2A1-012 ─H→ P2A1-013 ─H→ Phase 2A.1 Registry Gate
P2A1-011 ─S→ P2A1-013 (若提供 Import，additive/idempotent 是专项 Gate)
```

```text
Phase 2A.1 Registry Gate
  └─H→ P2A2-001
       ├→ P2A2-002
       ├→ P2A2-003
       ├→ P2A2-004
       ├→ P2A2-005
       ├→ P2A2-006 → P2A2-007 → P2A2-008
       ├→ P2A2-009 (Git Lease route)
       ├→ P2A2-010 (CodeGraph unavailable)
       └→ P2A2-012 (Remove typed coordination)
```

## 2A.2 Backend Continuity 特殊交付链

```text
P2A2-001 + P2A2-004
  └─H→ P2A3-001 → P2A3-002 → P2A3-003
                               ├→ P2A3-004
                               ├→ P2A3-005 → P2A3-006
                               └→ P2A3-007
P2A3-003 + P2A3-004 + P2A3-007 + P0-005(E, completed/resolved)
  └─H→ P2A3-008 → P2A3-009 → P2A3-010 → P2A3-011

P2A3-003 ─H→ P2A3-004 (Fake Provider capacity/LRU；不依赖 P0-005)
P2A2-009 + P2A2-010 + P2A3-011 ─H→ P2A2-011 (撤除 Global Active routing)
P2A2-001..012 + P2A3-011 ─H→ P2A2-013
```

这条交叉依赖不合并 Phase。它表示：2A.2 Authority 代码可以先完成，但必须等最小 Lease-routed Serena Source route 后，才能声明 2A.2 可运行 Gate 完成。

```text
P2A3-001..013 + P0-005(E, completed/resolved) ─H→ P2A3-014
P2A2-013 + P2A3-014 ─H→ Phase 2A.3 Gate
```

## Source Read / Write

```text
Phase 2A.3 Gate
  └→ P2B-001
      ├→ P2B-002
      ├→ P2B-003
      ├→ P2B-004
      └→ P2B-005
P2B-002..005 → P2B-006 → P2B-007 → Phase 2B Gate
```

```text
Phase 2B Gate
  ├→ P2C-001
  ├→ P2C-002
  └→ P2C-003 → P2C-004
P2C-001 → P2C-005
P2C-001..005
  ├→ P2C-006
  ├→ P2C-007
  ├→ P2C-008
  ├→ P2C-009
  ├→ P2C-010
  └→ P2C-011
P2C-006..011 → P2C-012 → P2C-013 → Phase 2C Gate
```

## Capability Adapter / CodeGraph

```text
Phase 2C Gate + P2A3-013
  ├→ P2D-001
  └→ P2D-002 (Adapter only) → P2D-003 (path preservation test)

P0-006(E) + P2A3-002 + P2A2-010
  └→ P2D-004 → P2D-005

P0-007(E) + P2D-004 + P2A3-003..006
  └→ P2D-006

P2D-004..006 + P2A3-013
  └→ P2D-007

P2D-001..006
  └→ P2D-008

P2D-001..008 + P0-006(E) + P0-007(E)
  └→ P2D-009 → Phase 2D Gate
```

## Activity / Usage / UI

```text
Phase 2D Gate
  ├→ P3-001
  └→ P3-002 → P3-003 → P3-004 → P3-005
                         └────────→ P3-006
P3-001..006 → P3-007 → Phase 3 Gate
```

```text
Phase 3 Gate + P0-008(E)
  └→ P4-001 → P4-002
       └────────→ P4-003
P4-002 + P4-003 → P4-004 → P4-005 → P4-006
P4-001..006 + P0-008(E) → P4-007 → Phase 4 Gate
```

```text
Phase 4 Gate
  ├→ P5-001
  ├→ P5-002
  ├→ P5-003
  └→ P5-004
P5-001..004 + P0-010 → P5-005 → Phase 5 Gate
```

## Installer / Manual Acceptance

```text
Phase 5 Gate + P0-009(E)
  ├→ P6-001
  ├→ P6-002
  ├→ P6-004
  └→ P6-005

P6-001 → P6-003
P6-001 + P6-005 + P0-010 → P6-006
P6-001..006 + P0-009(E) → P6-007 → Phase 6 Gate
```

```text
Phase 6 Gate
  → P7-001 → P7-002 → P7-003
                         ├→ P7-004
                         ├→ P7-005
                         ├→ P7-006 → P7-007
                         └→ P7-008 → P7-009 → P7-010

P7-001..010 ─A→ P7-011
P7-011 PASS ─A→ Release acceptance
```

软依赖：

```text
P0-004 ─S→ P2A1-011
P0-001 ─S→ 每个后续测试任务的回归比较
P0-002 ─S→ 所有 MCP schema/hash 更新任务
P0-003 ─S→ 所有 DB/config migration 任务
```

---

---

# 各 Phase Gate

| Phase | Gate |
|---|---|
| Phase 0 | 相关 baseline 已记录；各 external evidence 只对依赖阶段生效；`npm test` 正式入口可用 |
| Phase 1 | Runtime/Recovery/Cancel/requestKey/Claim/atomic release 无回归；公共层无 Codex 私有控制依赖 |
| Phase 2A.1 | `ManagerConfig.workspaces` 唯一 Authority；startup Serena sync 停止；CRUD/restart/concurrency PASS；显式 Import 若提供，additive/idempotent 必须 PASS |
| Phase 2A.2 | request workspaceId→Lease 唯一 Authority；无 fallback；Execution 冻结；A/B 隔离；公开 Source/Git 连续可用；CodeGraph 新 Adapter 前 unavailable/不 advertise |
| Phase 2A.3 | Serena Workspace-scoped Slot、single-flight、capacity/LRU/BUSY、remove/shutdown、无 retarget 全部 PASS |
| Phase 2B | 四个 Source Read 全 Rust；`relative_path` 保持；bounds/SHA/cancel/provenance/junction PASS |
| Phase 2C | 六个 Write Tool、WriteGuard、target mutex、locked OCC、atomic replace、Remote disabled PASS |
| Phase 2D | 已 Lease-rooted 的 Git 收敛为 Provider Adapter；Source/Git/Serena/CodeGraph 都显式接收 Lease；新 CodeGraph Adapter ready 后才恢复 advertise；Local Human actions/Health/cleanup/隔离 PASS |
| Phase 3 | summaryCode、activity sequence/history、observe wake、revision 解耦、privacy PASS |
| Phase 4 | Usage wire/baseline/delta/coverage/grace/freeze/product PASS；不影响 lifecycle |
| Phase 5 | Provider/Activity/Usage UI 覆盖 unknown/partial/complete/zero/history；无 N+1 |
| Phase 6 | 版本一致；quality gates 全绿；NSIS currentUser；single-instance/data path/autostart/uninstall PASS |
| Phase 7 | 所有真实 Manual E2E 场景与 §56 checklist PASS，才可 Release |

---

---

# External Evidence 对应阻塞范围

| Evidence | Task | 阻塞 Phase | 不阻塞 |
|---|---|---|---|
| Serena CLI/project workflow | P0-004 | 依赖具体行为的 P2A3 Serena 任务 | Phase 1、2A.1、2A.2 基础 |
| Serena multi-process/shared config | P0-005 (completed/resolved) | 已满足的 Phase 2A.3 Serena 实际容量参数/process startup 证据 | Phase 1、2A.1、2A.2、通用 Manager capacity/LRU |
| CodeGraph status/init/sync contract | P0-006 | Phase 2D CodeGraph readiness/actions | Phase 1～2C |
| CodeGraph multi-workspace/process | P0-007 | Phase 2D RuntimeSlot/capacity | Phase 1～2C |
| Codex Usage wire/terminal coverage | P0-008 | Phase 4 | Phase 1～3 |
| Installer/data path/single-instance environment | P0-009 | Phase 6 | Phase 1～5 |
| Manual clean Windows acceptance | P7-011 | 最终 Release | 前述实现任务 |

---

---

# NEXT 10 TASKS

1. `P0-001` — 当前质量与测试基线
2. `P0-002` — MCP Tool 与 Schema 基线
3. `P0-003` — Workspace、Config 与 State Migration 基线
4. `P0-004` — Serena CLI 与 Project Workflow Evidence（独立外部证据线）
5. `P0-010` — 正式化前端测试入口
6. `P1-001` — Provider Domain Types（主实施线）
7. `P1-002` — Object-safe AgentProvider Port
8. `P0-005` — Serena 多进程/Shared SERENA_HOME Evidence（P0-004 后尽早并行）
9. `P1-003` — ProviderRegistry
10. `P1-004` — Codex Registration Adapter

P0-005 Evidence 已 completed/resolved；其 per-slot Home DCR 为 Serena 真实 process/capacity 集成提供已满足的前置证据，不影响 Phase 1 或通用 Capability Manager。

---

---

# DESIGN_BLOCKER

None

冻结设计中没有未决实施阻断矛盾。P0-005 shared-config evidence 已通过 approved per-slot `SERENA_HOME` DCR resolved；2A.2 Backend Continuity 继续通过跨阶段 hard dependency 显式处理。

```text
Implementation planning readiness:
READY
```

---
