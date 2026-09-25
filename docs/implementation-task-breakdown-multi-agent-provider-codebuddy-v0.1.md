# SerenaDesktop Multi-Agent Provider / CodeBuddy ACP Implementation Task Breakdown V0.1

依据冻结候选方案 technical-design-multi-agent-provider-codebuddy-v0.1.md（LF 规范化 SHA-256: 7795e6dbc893fe451e19c6915245d664481832d60d2040713a32d02f57006fa0）以及当前仓库代码基线拆分。

设计状态：

~~~text
DESIGN FREEZE CANDIDATE
CodeBuddy / Multi-Agent Provider
V0.1
2026-09-21
~~~

本清单用于后续实际开发推进。任务顺序是默认执行顺序；除明确标记可并行外，后一个任务只有在前置 Gate 通过后才开始。

标记说明：

- H：hard dependency，未完成不得开始后续任务。
- S：soft dependency，可先准备但不得提前切换 Authority。
- E：external evidence gate，需要真实 CodeBuddy / Windows / ACP 证据。
- A：human acceptance gate，需要本地 UI 或真实 ChatGPT 流程人工验收。
- DESIGN_BLOCKER：当前设计或真实协议证据不足，Agent 必须停止实现并返回证据，禁止自行扩展架构或猜测协议。

所有 Implementation Task 必须控制在 small 或 medium blast radius；不得把一个 Phase 整体塞给 Agent 一次完成。

---

# 0. 推进协议

后续 ChatGPT / Codex 按以下规则执行本清单。

1. 一次只派发一个最小任务，除非任务卡明确允许并行。
2. Agent 开始前必须读取：
   - 本任务卡；
   - 对应 Contract references；
   - 直接依赖任务的 Evidence。
3. Agent 不得自行跨入下一任务。
4. 遇到下列情况立即返回 DESIGN_BLOCKER：
   - 真实代码基线与任务卡关键事实不一致；
   - schema / request identity / Runtime Evidence 需要新增未冻结 Authority；
   - CodeBuddy / ACP wire 与设计假设不一致；
   - 为通过测试需要降低 Claim / Runtime / unknown fail-closed 安全语义。
5. 每个任务完成必须返回：
   - 修改文件；
   - 核心行为变化；
   - 实际执行命令；
   - PASS / FAIL / NOT_RUN；
   - 测试数量或关键断言；
   - Git diff 摘要；
   - 未解决风险。
6. 不以“编译通过”替代任务 Acceptance。
7. 不以 Provider terminal 替代 Claim Release Evidence。
8. 不允许自动 fallback 到其他 Provider。
9. 不提交 Git，除非 Host 单独要求。
10. 一个 Task Gate 未通过时，不继续下一个 Task。

推荐每次派发 Agent 时只引用一个 Task ID，例如：

~~~text
严格执行 CB1A-001。
只允许修改任务卡 Allowed scope。
完成所有 Tests required 并返回 Evidence。
若发现超出冻结契约的问题，返回 DESIGN_BLOCKER，不进入下一任务。
~~~

---

# Phase 0 — Freeze Baseline / Preflight

## CB0-001 — 当前构建与测试基线

Phase: Phase 0  
Type: contract-test  
Goal: 固定进入 Multi-Agent 改造前的 Rust、前端、MCP、Git 基线。  
Why now: 后续任何失败都必须能区分历史问题与本次回归。  
Dependencies: None  
Blocked by: None  
Allowed scope: 只读运行已有测试 / build / lint / git 命令。  
Forbidden scope: 修复任何失败、格式化仓库、修改依赖。  
Contract references: 设计 §2、§32；现有 Agent Platform Phase 0 baseline。  
Implementation requirements: 记录命令、exit code、测试数量、首个失败和 Git status；明确当前新增设计文档状态。  
Non-goals: 清理历史 warning。  
Tests required: npm test、npm run build、cargo fmt --check、cargo check、相关 cargo test、git diff --check。  
Evidence required: 基线命令矩阵。  
Acceptance criteria: 每项 Gate 都有 PASS / FAIL / NOT_RUN；历史失败被单独标识。  
Rollback / failure behavior: 只登记，不修复。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: CB0-002、CB0-003

## CB0-002 — Multi-Provider Hardcode Inventory Freeze

Phase: Phase 0  
Type: contract-test  
Goal: 冻结当前所有 Codex-only provider、Product、Usage、Runtime、MCP 硬编码点。  
Why now: 防止 CB1A/1C 只改到显眼路径而遗漏隐式 Codex Authority。  
Dependencies: None  
Blocked by: None  
Allowed scope: src-tauri/src/agent、src-tauri/src/mcp、src/types.ts、Agent UI 只读。  
Forbidden scope: 修改代码。  
Contract references: 设计 §2.1、§12、§20.1、§27。  
Implementation requirements: 至少覆盖 execution::Provider、insert_execution、schema CHECK、ProviderProduct、Usage private state、Runtime codex_* 列、TaskManager route。  
Non-goals: 给出重构方案。  
Tests required: source search + existing provider architecture tests。  
Evidence required: 文件/符号清单，标记“必须泛化 / 必须保持 Provider-private”。  
Acceptance criteria: 后续任务的 Allowed scope 能覆盖清单中所有阻塞项。  
Rollback / failure behavior: 漏项即补清单，不改代码。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: CB0-001、CB0-003

## CB0-003 — Agent State v11 Migration Fixture Baseline

Phase: Phase 0  
Type: contract-test  
Goal: 固定当前 schema v11 真实结构及 Execution / Runtime / Claim / Usage / CommandRun fixture，作为 v12 直接迁移输入；可额外保留 frozen v9 fixture 用于历史兼容链路。\
Why now: schema_v12 是本轮最高风险持久化变更；既有 v10 Runtime containment 和 v11 CommandRun migration 不重用、不改写。\
Dependencies: None  
Blocked by: None  
Allowed scope: StateStore tests、临时 SQLite fixture；不修改生产 schema。  
Forbidden scope: 提前创建 schema_v12；修改既有 schema_v9/v10/v11。\
Contract references: 设计 §12.5～§12.7。  
Implementation requirements: v11 fixture 至少包含 completed、pending、unknown、Runtime evidence、Workspace Claim、Work link、Codex Usage state、v10 platform/containment 字段及触发器、v11 command_runs/work_command_links；可额外保留 frozen v9 fixture。\
Non-goals: 执行 v12 migration。\
Tests required: 当前 v11 能打开、读取、restart；记录 PRAGMA foreign_key_check；如保留 v9 fixture，仅验证历史 v9→v10→v11 链路。\
Evidence required: fixture shape + user_version=11 + 核心字段 snapshot；可选 v9 历史 fixture shape。\
Acceptance criteria: v12 后可逐字段验证“原值保留 / general 默认 / hash 不重写”，并保留 v10/v11 既有契约。\
Rollback / failure behavior: fixture 不完整则补 fixture，不能进入 CB1A-003。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: CB0-001、CB0-002

---

# Phase 1A — CB-001A Multi-Provider Persistence Migration

## CB1A-001 — AgentTaskRole Domain 与 General Compatibility

Phase: Phase 1A  
Type: implementation  
Goal: 建立固定 Role domain，并让所有现有 Execution 创建调用显式具有 General 默认。  
Why now: schema_v12 增加 task_role 前先闭合 Rust domain。\
Dependencies: CB0-001 (H)  
Blocked by: None  
Allowed scope: src-tauri/src/agent/execution.rs、直接构造 CreateExecutionInput 的调用与 focused tests。  
Forbidden scope: schema migration、request hash v3、MCP taskRole。  
Contract references: 设计 §4.2、§9、§12.5、CB-001A。  
Implementation requirements:
- 新增 AgentTaskRole: development/testing/review/analysis/general；
- CreateExecutionInput 增加 task_role；
- serde / compatibility default = General；
- 所有现有 Rust struct literal 显式或等价补 General；
- canonicalize_request 仍保持 execution-request-v2，不包含 task_role。
Non-goals: 路由策略。  
Tests required: enum serde、invalid role、legacy deserialize/default、v2 canonical bytes/hash fixed regression。  
Evidence required: focused Rust tests + v2 hash 未变化证据。  
Acceptance criteria: 编译通过；所有旧创建路径语义仍为 General；request hash 完全不变。  
Rollback / failure behavior: 若新增字段导致必须提前变更 request identity，返回 DESIGN_BLOCKER。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: None

## CB1A-002 — Execution Provider Identity 收敛

Phase: Phase 1A  
Type: implementation  
Goal: 消除 execution::Provider 单变体 Authority，使 Execution creation 使用通用 ProviderId 语义。  
Why now: schema 放开 provider 前，Rust 域必须能表达第二 Provider。  
Dependencies: CB1A-001 (H)  
Blocked by: None  
Allowed scope: agent/execution.rs、agent/provider domain、直接构造 CreateExecutionInput 的调用与 tests。  
Forbidden scope: ProviderRegistry admission、CodeBuddy Provider、schema_v12。\
Contract references: 设计 §2.1、§12.1。  
Implementation requirements:
- 不新增第二套 provider identifier；
- CreateExecutionInput.provider 与 provider::ProviderId 使用同一 validation / wire string 语义，或建立单向无损桥接且只有一个 Authority；
- 现有 Codex 路径仍持久化语义 codex；
- request v2 tuple 的 provider wire bytes 保持兼容。
Non-goals: 写入 codebuddy 数据库。  
Tests required: codex fixed hash、fake provider id round-trip、invalid whitespace/control ProviderId rejection。  
Evidence required: provider type dependency search，证明 execution domain 不再只有 Codex enum。  
Acceptance criteria: 无新增 provider-specific enum 分支；v2 request identity 不变。  
Rollback / failure behavior: 发现 ProviderId 迁移会破坏历史 hash，停在本任务设计补丁。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## CB1A-003 — schema_v12 与 Store 写入切换

Phase: Phase 1A  
Type: implementation  
Goal: 完成 v11→v12 的单事务持久化迁移并让 Store 显式写 provider + task_role。\
Why now: 第二 Provider 真正进入 StateStore 的核心 Gate。  
Dependencies: CB0-003、CB1A-002 (H)  
Blocked by: v11 fixture 未冻结\
Allowed scope:
- src-tauri/src/agent/store.rs；
- 新 schema_v12.sql 或专用 v12 migration helper；
- schema/runtime store 查询的必要同步；
- migration focused tests。
Forbidden scope: request hash v3、Product 投影、CodeBuddy ACP；修改既有 schema_v9/v10/v11 migration。\
Contract references: 设计 §12.1～§12.7。  
Implementation requirements:
- migrate() 复用已有 TransactionBehavior::Immediate transaction，不嵌套 BEGIN；
- user_version 从当前 11 支持到 12，`version < 12` 时在既有 migration 后追加 v12；
- executions.provider 移除 CHECK(provider='codex')；
- task_role TEXT NOT NULL DEFAULT 'general'；
- insert_execution 显式写 provider 和 task_role，不依赖 SQL 默认；
- runtime_instances 增加 provider 并按冻结列映射泛化 process identity；
- 保留 v10 Runtime platform/containment 字段及验证触发器、v11 command_runs/work_command_links 与其索引和外键；
- historical runtime provider 只允许 codex；
- thread_id/turn_id 保持兼容字段；
- historical request_hash 原值复制，不重算。
Non-goals: Provider routing。  
Tests required:
- real v11→v12 primary fixture；
- frozen v9→v10→v11→v12 transitive compatibility fixture（CB0-003 未保留时在本任务测试中构造 v9 fixture）；
- empty→latest；
- rollback on injected failure；
- future-version rejection 继续拒绝高于 v12 的版本（现有测试将 12 用作未支持版本）；
- foreign_key_check；
- indexes/triggers/FKs presence；
- 新 Execution 在 CB1B 前仍能以 v2 + general 创建。
Evidence required: migration before/after matrix。  
Acceptance criteria:
- pre-v12 task_role 全为 general；
- request_hash byte-identical；
- Runtime/Execution evidence 不增强；
- codebuddy provider string 可合法持久化；
- Codex existing tests 无回归。
Rollback / failure behavior: 任一 FK/trigger/evidence 不可证明，migration 事务回滚并返回 DESIGN_BLOCKER。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## CB1A-004 — Runtime Provider Ownership Validation

Phase: Phase 1A  
Type: implementation  
Goal: 把 Execution.provider == RuntimeInstance.provider 纳入所有 Runtime Evidence 写入和读取边界。  
Why now: schema 能保存第二 Provider 后，必须防止跨 Provider Runtime Evidence 污染。  
Dependencies: CB1A-003 (H)  
Blocked by: None  
Allowed scope: agent/store/runtime_store.rs、transactions、recovery/finalize focused helpers/tests。  
Forbidden scope: CodeBuddy Runtime 实现、改变 ReleaseBasis。  
Contract references: 设计 §12.2、§21、§31.9、§31.19。  
Implementation requirements: first bind、provider terminal evidence、termination evidence、startup reconciliation input、finalization re-read 都验证 provider ownership；Provider mismatch 不释放 Claim。  
Non-goals: 新 Release kind。  
Tests required: runtime provider mismatch for bind/evidence/finalize/recovery；Codex happy path。  
Evidence required: focused tests + provider mismatch fail-closed matrix。  
Acceptance criteria: Provider ID 只用于 ownership validation，永远不成为 Release authorization。  
Rollback / failure behavior: mismatch → stable error / unknown，Claim retained。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## CB1A-005 — Persistence Gate Regression

Phase: Phase 1A  
Type: contract-test  
Goal: 对 CB-001A 做完整持久化 Gate，确认可独立合入。  
Why now: 后续 hash/product 改造必须建立在稳定 v12 上。\
Dependencies: CB1A-004 (H)  
Blocked by: None  
Allowed scope: tests / evidence only；只修本阶段缺陷。  
Forbidden scope: 进入 v3 hash 或 Product neutralization。  
Contract references: 设计 CB-001A Gate。  
Implementation requirements: 使用真实 v11 fixture + fresh DB，并验证 frozen v9→v10→v11→v12 历史兼容链路。\
Tests required: migration、Store、Runtime、Recovery、Claim、request v2 fixed regression、git diff --check。  
Evidence required: PASS matrix。  
Acceptance criteria: v12 Gate 全 PASS；可在没有 CodeBuddy Runtime 的情况下正常使用 Codex。\
Rollback / failure behavior: Gate FAIL 时只修 1A 范围。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

---

# Phase 1B — CB-001B Request Identity v3 / Legacy Compatibility

## CB1B-001 — execution-request-v3 Canonicalization

Phase: Phase 1B  
Type: implementation  
Goal: 将 task_role 纳入新 Execution request identity。  
Why now: provider/role 已持久化后，幂等身份必须包含用户路由意图。  
Dependencies: CB1A-005 (H)  
Blocked by: None  
Allowed scope: agent/execution.rs + fixed-vector tests。  
Forbidden scope: legacy retry Store logic、MCP routing。  
Contract references: 设计 §9.1、CB-001B。  
Implementation requirements: 新请求只生成 execution-request-v3；provider 保持原 v2 位置/语义；追加 task_role 的位置和 framing 固定。  
Non-goals: 改历史 hash。  
Tests required: fixed bytes/hash；provider change conflict vector；taskRole change conflict vector。  
Evidence required: canonical tuple documentation + test vectors。  
Acceptance criteria: v3 deterministic；fresh requests 不再生成 v2。  
Rollback / failure behavior: 若 tuple 无法保持 bounded legacy compatibility，返回 DESIGN_BLOCKER。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

## CB1B-002 — Historical v2/v1 Retry Compatibility

Phase: Phase 1B  
Type: implementation  
Goal: 允许合法历史 Execution 在升级后按原 requestKey 重试，而不重写历史 hash。  
Why now: v3 上线后旧 v2/v1 persisted rows 仍必须保持幂等语义。  
Dependencies: CB1B-001 (H)  
Blocked by: None  
Allowed scope: execution legacy hash helpers、Store request match / creation transaction、tests。  
Forbidden scope: 宽松 Prompt 比较、hash backfill、provider fallback。  
Contract references: 设计 §9.1。  
Implementation requirements:
- current v3 exact match 优先；
- 仅历史 task_role=general 行允许 bounded v2 compatibility；
- 继续保留 pre-workspace-generation / pre-C2 bounded rules；
- generation/provider/mode/parent identity 必须一致；
- 不更新 persisted request_hash。
Non-goals: 新兼容版本框架。  
Tests required: v3 retry、v2 general retry、v2 role mismatch、provider mismatch、generation mismatch、pre-C2 continuation。  
Evidence required: retry matrix。  
Acceptance criteria: 合法历史 retry 返回原 Execution；不合法组合稳定 conflict。  
Rollback / failure behavior: 不确定历史身份 → conflict，不猜测。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## CB1B-003 — Request Identity Gate

Phase: Phase 1B  
Type: contract-test  
Goal: 关闭 requestKey / hash / continuation 所有回归。  
Why now: 后续 Routing 才能安全携带 provider + role。  
Dependencies: CB1B-002 (H)  
Blocked by: None  
Allowed scope: tests / evidence。  
Forbidden scope: MCP DTO。  
Contract references: 设计 CB-001B Gate。  
Tests required: execution/request/store/continue/retry 全套 focused + existing tests。  
Evidence required: current/legacy matrix。  
Acceptance criteria: fresh v3；historical v2/v1 bounded；历史 DB hash 无写入变化。  
Rollback / failure behavior: FAIL 只修 1B。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: None

---

# Phase 1C — CB-001C Product / Usage Provider Neutralization

## CB1C-001 — ProviderProduct Registry Projection

Phase: Phase 1C  
Type: implementation  
Goal: 移除 Product 层只认识 Codex 的投影。  
Why now: 第二 Provider Execution 必须能被 query/observe/list 正常展示。  
Dependencies: CB1B-003 (H)  
Blocked by: None  
Allowed scope: agent/product.rs、ProviderRegistry read APIs、focused tests。  
Forbidden scope: CodeBuddy-specific Product branch、Provider health 第二套字段。  
Contract references: 设计 §27。  
Implementation requirements:
- persisted provider id 是 Execution identity；
- registered provider 优先取 descriptor；
- unregistered/historical provider 安全 fallback 为 id/displayName=id/version=null；
- query/observe/list 不因 provider 未注册失败。
Non-goals: provider management catalog。  
Tests required: codex、fake codebuddy、unknown historical provider。  
Evidence required: Product JSON fixtures。  
Acceptance criteria: generic Product 不出现 codebuddy 特判。  
Rollback / failure behavior: descriptor 不可用时降级展示，不伪造 Codex。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: None

## CB1C-002 — taskRole Product Projection

Phase: Phase 1C  
Type: implementation  
Goal: ExecutionView 稳定返回创建时冻结的 taskRole。  
Why now: 当前 Routing Policy 与历史 Execution Routing 必须可观察地分离。  
Dependencies: CB1C-001 (H)  
Blocked by: None  
Allowed scope: Product DTO/projector、list/observe/detail tests、前端 type 的最小兼容更新。  
Forbidden scope: Role Routing UI。  
Contract references: 设计 §10.1、§27。  
Implementation requirements: role 从 persisted execution 投影；不得读取当前 ManagerConfig 改写历史 role。  
Non-goals: 当前 role policy 展示。  
Tests required: role remains frozen after fixture policy changes；historical general row。  
Evidence required: Product DTO snapshot。  
Acceptance criteria: ExecutionView provider + taskRole 能作为 frozen routing identity 被读取。  
Rollback / failure behavior: persisted invalid role → contract error，不推断。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: CB1C-003

## CB1C-003 — Non-Codex Usage Gate

Phase: Phase 1C  
Type: implementation  
Goal: 保持 Codex Usage 私有实现，同时让非 Codex Execution 稳定返回 unknown/null。  
Why now: CodeBuddy 不能误写 codex_execution_usage_state。  
Dependencies: CB1C-001 (H)  
Blocked by: None  
Allowed scope: agent/store/usage.rs、usage projector/Product usage tests。  
Forbidden scope: 实现 CodeBuddy usage。  
Contract references: 设计 §20.1、§27。  
Implementation requirements:
- Codex private functions显式拒绝非 Codex；
- generic Product 对非 Codex 不进入 Codex baseline/grace/freeze；
- public usage 未存在时 unknown/null；
- 不能写 provider_id='codex'。
Non-goals: 抽象 Codex Thread epoch。  
Tests required: fake codebuddy execution observe/list/detail；DB assert 无 codex private row；Codex usage regressions。  
Evidence required: DB row matrix + Product snapshot。  
Acceptance criteria: 第二 Provider 不触发 Codex Usage side effect。  
Rollback / failure behavior: 未支持 provider usage → unknown，不报整个 observe failure。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: CB1C-002

## CB1C-004 — Multi-Provider Persistence/Product Gate

Phase: Phase 1C  
Type: contract-test  
Goal: 在无真实 CodeBuddy 的情况下用 Fake Provider 完成数据→Product 垂直验证。  
Why now: 平台层必须先独立于 ACP 成立。  
Dependencies: CB1C-002、CB1C-003 (H)  
Blocked by: None  
Allowed scope: tests / fixtures。  
Forbidden scope: 真 CodeBuddy process。  
Contract references: 设计 CB-001C Gate。  
Tests required: fake provider create/read/list/observe/restart；unknown historical provider；Usage unknown。  
Evidence required: PASS matrix。  
Acceptance criteria: Provider-Agnostic persistence + Product 基线成立。  
Rollback / failure behavior: 失败只修 Phase 1。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: None

---

# Phase 2 — CB-002 Local Provider Policy

## CB2-001 — ManagerConfig AgentProviderSettings

Phase: Phase 2  
Type: implementation  
Goal: 增加 Provider enabled 与 roleRouting 的本地持久化配置。  
Why now: Local Human Authority 必须先于 Remote routing 上线。  
Dependencies: CB1C-004 (H)  
Blocked by: None  
Allowed scope: src-tauri/src/config.rs、config tests。  
Forbidden scope: ProviderRegistry health、MCP schema、UI。  
Contract references: 设计 §5。  
Implementation requirements:
- 保留 agent_enabled 总开关；
- providers[providerId].enabled；
- roleRouting 五个固定 Role → optional provider；
- 旧配置默认 Codex enabled、CodeBuddy disabled、全部 Role→Codex；
- ProviderId / Role validation；
- 未注册 provider 允许保留配置值用于可诊断展示，但不能执行。
Non-goals: 自动发现 Provider。  
Tests required: old config deserialize、round-trip、invalid role/provider id、default migration。  
Evidence required: config JSON fixtures。  
Acceptance criteria: 重启后策略保持；旧用户行为不变。  
Rollback / failure behavior: invalid config fail with stable validation error。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: None

## CB2-002 — Provider Admission Policy

Phase: Phase 2  
Type: implementation  
Goal: 在 Registry health 之外增加 Local enabled admission gate。  
Why now: disabled 不能等价 unregister/unavailable。  
Dependencies: CB2-001 (H)  
Blocked by: None  
Allowed scope: provider/control admission helper、TaskManager/Product route focused tests。  
Forbidden scope: Remote MCP fields、UI。  
Contract references: 设计 §6、§10。  
Implementation requirements:
- registered → enabled → health → capability 顺序；
- execute/start/continue/resume pending 受 enabled gate；
- cancel/startup_reconcile 不受 disabled gate；
- disabled stable error AGENT_PROVIDER_DISABLED；
- 单次 execution-local failure 不改变 global health。
Non-goals: CodeBuddy Contract Gate。  
Tests required: disabled start/continue/resume、disabled cancel/reconcile、unavailable distinction。  
Evidence required: routing matrix。  
Acceptance criteria: disabled/health/capability 三层事实可区分。  
Rollback / failure behavior: disabled 不取消 running Execution。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## CB2-003 — Local IPC for Provider Policy

Phase: Phase 2  
Type: implementation  
Goal: 只允许本地 Desktop 管理 Provider enabled / Role Routing / health refresh。  
Why now: UI 需要稳定本地 mutation surface，Remote MCP 只能读。  
Dependencies: CB2-002 (H)  
Blocked by: None  
Allowed scope: Tauri commands/API、Supervisor config lock/persist、tests。  
Forbidden scope: Remote MCP mutation tool。  
Contract references: 设计 §26。  
Implementation requirements:
- settings_get；
- set_enabled；
- set_role_route；
- refresh_health；
- atomic config save；
- toggle 本身不 spawn Agent Runtime。
Non-goals: UI 页面。  
Tests required: command validation、concurrent config save、restart persistence、no-runtime-spawn assertion。  
Evidence required: local IPC contract tests。  
Acceptance criteria: 所有策略 mutation 只来自 Local IPC。  
Rollback / failure behavior: persist failure 不改变内存 Authority。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: None

## CB2-004 — Frontend Config Model Synchronization

Phase: Phase 2  
Type: implementation  
Goal: 前端类型和 controller 能无损读写新的 Agent Provider 配置。  
Why now: 防止 Rust Config 已升级但 TS fixture / save path 丢字段。  
Dependencies: CB2-003 (H)  
Blocked by: None  
Allowed scope: src/types.ts、src/api.ts、src/app/useAppController.ts、相关 test fixtures。  
Forbidden scope: Agent 管理页面视觉重构。  
Contract references: 设计 CB-002。  
Implementation requirements: ManagerConfig 类型、initialConfig、partial save、fixture 同步；未知 provider routing 不应被前端序列化丢失。  
Non-goals: role dropdown。  
Tests required: npm test focused + build。  
Evidence required: frontend contract diff。  
Acceptance criteria: config round-trip 不丢 Provider policy。  
Rollback / failure behavior: 类型不一致即阻塞 Phase 3。  
Risk: low  
Estimated blast radius: medium  
Can run in parallel with: None

## CB2-005 — Drain / Pending Claim Policy Gate

Phase: Phase 2  
Type: contract-test  
Goal: 固定 disable 时 running / pending / claim 的后台语义。  
Why now: UI 前必须保证“停用”不会误杀或误释放。  
Dependencies: CB2-004 (H)  
Blocked by: None  
Allowed scope: backend tests + read-only Product helper。  
Forbidden scope: Force Unlock。  
Contract references: 设计 §6.2、§10.3、§25.2～§25.3。  
Tests required:
- running execution survives disable；
- new execution rejected；
- pending resumable retains Claim；
- cancel remains callable；
- re-enable then resume possible；
- startup reconcile disabled provider still runs。
Evidence required: state/claim matrix。  
Acceptance criteria: Drain 和 pending Claim 语义全部 PASS。  
Rollback / failure behavior: 未证明时 UI 不提供 disable。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

---

# Phase 3 — CB-003 Provider Discovery / Remote Routing Contract

## CB3-001 — Provider Catalog Product Snapshot

Phase: Phase 3  
Type: implementation  
Goal: 建立 provider catalog + current roleRouting 的只读 Product DTO。  
Why now: MCP 和 UI 都应消费同一事实源。  
Dependencies: CB2-005 (H)  
Blocked by: None  
Allowed scope: Agent Product service、ProviderRegistry read methods、tests。  
Forbidden scope: MCP schema、CodeBuddy-specific health logic。  
Contract references: 设计 §7。  
Implementation requirements: descriptor/version/enabled/health/availableForNewExecution/capabilities + roleRouting；query 不启动 Runtime。  
Non-goals: provider mutation。  
Tests required: codex、disabled、unavailable、unregistered route target、capability false。  
Evidence required: Product JSON snapshot。  
Acceptance criteria: Catalog 区分 registration / enabled / health / capability / policy。  
Rollback / failure behavior: snapshot read failure 不改变 Provider state。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: None

## CB3-002 — agent_query providers MCP Contract

Phase: Phase 3  
Type: implementation  
Goal: 将 Provider Catalog 暴露为 agent_query(action=providers)。  
Why now: ChatGPT 必须在下发任务前读取用户策略。  
Dependencies: CB3-001 (H)  
Blocked by: None  
Allowed scope: mcp/orchestration dto/router/schema/tests。  
Forbidden scope: mutation Provider policy。  
Contract references: 设计 §7、§26。  
Implementation requirements: strict schema；无 workspaceId；无 Runtime side effect；output 复用 Product DTO。  
Non-goals: start routing。  
Tests required: schema、tool descriptor/hash、call tool、no spawn assertion。  
Evidence required: MCP input/output examples。  
Acceptance criteria: Remote 只能读取策略。  
Rollback / failure behavior: query failure 不影响 Provider。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: None

## CB3-003 — Start DTO Compatibility Parser

Phase: Phase 3  
Type: implementation  
Goal: 增加 taskRole/providerId，同时保持旧 agent_execute start 的 bounded compatibility。  
Why now: 这是公开 MCP breaking-change 风险点。  
Dependencies: CB3-002 (H)  
Blocked by: None  
Allowed scope: mcp/orchestration/dto.rs、parse/registry schema tests。  
Forbidden scope: 真正 dispatch、Role policy enforcement。  
Contract references: 设计 §8.0。  
Implementation requirements:
- two fields both present → explicit；
- both absent → legacy general；
- only one → INVALID_PARAMS；
- continue/cancel/resume schemas不新增 providerId/taskRole。
Non-goals: 自动 fallback。  
Tests required: complete compatibility matrix。  
Evidence required: JSON parse/schema matrix。  
Acceptance criteria: 旧客户端调用仍可被解析，新客户端 schema 明确。  
Rollback / failure behavior: ambiguous request 必须 invalid，不猜测。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

## CB3-004 — Start Routing Authority

Phase: Phase 3  
Type: implementation  
Goal: 在 Execution 创建前根据 Local Human Policy 解析并验证 taskRole + providerId。  
Why now: Remote task routing 的真正 Authority cutover。  
Dependencies: CB3-003 (H)  
Blocked by: None  
Allowed scope: Product work_adapter / TaskManager admission / src-tauri/src/agent/store/transactions/product.rs 及 focused creation transaction tests。\
Forbidden scope: 自动换 Role、自动 fallback、修改配置。  
Contract references: 设计 §8～§9。  
Implementation requirements:
- explicit role/provider exact policy match；
- legacy → taskRole=general + resolve current general provider；
- unconfigured role → AGENT_ROLE_NOT_CONFIGURED；
- mismatch → AGENT_ROLE_PROVIDER_MISMATCH；
- disabled/unavailable/capability errors保持独立；
- provider + role 在创建时冻结；
- creation/retry race 必须重读必要 Authority。
Non-goals: CodeBuddy implementation。  
Tests required: all route errors、requestKey retry、policy changed before create、provider disabled after query。  
Evidence required: routing matrix。  
Acceptance criteria: SerenaDesktop 不解析 Prompt 分类任务。  
Rollback / failure behavior: policy uncertainty → reject, no Execution created。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## CB3-005 — Continue / Resume Routing + MCP Gate

Phase: Phase 3  
Type: implementation + contract-test  
Goal: 关闭 Continue/Resume 的 Provider/Role 继承和 public contract。  
Why now: Start 切换后最容易出现“继续任务重新选 Provider”的泄漏。  
Dependencies: CB3-004 (H)  
Blocked by: None  
Allowed scope: Product continue/resume、MCP tests、Provider capability projection。  
Forbidden scope: CodeBuddy Session implementation。  
Contract references: 设计 §10、CB-003 Gate。  
Implementation requirements:
- Continue 请求不接受 workspaceId/providerId/taskRole；
- child 从 source 继承 provider/taskRole；
- disabled provider 阻止新 Continue；
- ResumePending 使用原 Execution provider/role；
- routing error 后 agent_query(providers) 可给出恢复信息；
- advertised capability = implementation ∩ passed Evidence Gate。
Non-goals: 让 CodeBuddy canContinue=true。  
Tests required:
- explicit / legacy Start；
- half-field INVALID_PARAMS；
- general unconfigured；
- frozen routing vs current policy；
- Continue inheritance；
- capability false。
Evidence required: MCP contract matrix + full agent contract tests。  
Acceptance criteria: CB-003 Gate 全 PASS。  
Rollback / failure behavior: 不允许把 Continue 转成 fresh Start。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

---

# Phase 4 — CB-004 Agent 管理 UI

## CB4-001 — Agent 菜单与 Provider Cards

Phase: Phase 4  
Type: implementation  
Goal: 左侧“Agent”升级为“Agent 管理”，展示动态 Provider 状态。  
Why now: 后端管理契约已稳定，可开始 UI。  
Dependencies: CB3-005 (H)  
Blocked by: None  
Allowed scope: AgentPanel / src/agentPresentation.ts / navigation / types / styles / focused tests。\
Forbidden scope: 修改 Runtime 或 Routing。  
Contract references: 设计 §25～§25.2。  
Implementation requirements: 卡片展示 enabled、health、version、protocol、runtime state、active executions；Idle available/stopped 是正常状态；disable running provider 显示 draining。  
Non-goals: Role editor。  
Tests required: Codex only、Codex+CodeBuddy descriptor fixture、disabled/unavailable/draining。  
Evidence required: DOM tests + build，必要时截图由 Host 验收。  
Acceptance criteria: UI 不按 providerId 写布局特判。  
Rollback / failure behavior: Provider metadata 缺失用安全 fallback。  
Risk: medium  
Estimated blast radius: medium  
Can run in parallel with: None

## CB4-002 — Role Routing Editor

Phase: Phase 4  
Type: implementation  
Goal: 用户本地显式配置 development/testing/review/analysis/general 的首选 Provider。  
Why now: 这是多 Agent 分工的核心用户入口。  
Dependencies: CB4-001 (H)  
Blocked by: None  
Allowed scope: AgentPanel/role section/local IPC client/tests。  
Forbidden scope: Remote mutation、自动 fallback。  
Contract references: 设计 §4、§5、§25.3。  
Implementation requirements:
- 一个 Role 最多一个 Provider；
- 可以清空，显示“未指定 Agent”；
- disabled Provider 仍保留绑定并显示“已停用”；
- 不自动改绑；
- 保存失败回滚 UI draft。
Non-goals: Provider priority list。  
Tests required: set/clear/restart fixture/disabled binding。  
Evidence required: frontend test matrix。  
Acceptance criteria: Role Routing 只有 ManagerConfig 一套 Authority。  
Rollback / failure behavior: 保存失败不修改 persisted policy。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: None

## CB4-003 — Pending Claim / Unsupported Version UX Gate

Phase: Phase 4  
Type: implementation + A  
Goal: 补齐停用 Provider 和未知 CodeBuddy 版本的可行动反馈。  
Why now: 防止用户看到“无法执行”却不知道恢复路径。  
Dependencies: CB4-002 (H)  
Blocked by: None  
Allowed scope: AgentPanel、相关 product snapshot、tests。  
Forbidden scope: Force Unlock、用户强制放行未知版本。  
Contract references: 设计 §10.3、§14.2.1、§25.3、CB-004 Gate。  
Implementation requirements:
- pending/resumable Claim → 查看任务 / 取消任务 / 重新启用；
- 无 Force Unlock；
- unsupported version → “尚未经过当前 SerenaDesktop 兼容性验证”；
- supported-version table release-owned，UI 不提供 override。
Non-goals: 安装/降级 CodeBuddy。  
Tests required: pending claim fixture、unsupported version fixture、disabled role fixture、Agent UI regression。  
Evidence required: npm test/build + Host screenshot acceptance。  
Acceptance criteria: CB-004 Gate 全 PASS。  
Rollback / failure behavior: 无安全动作时只显示信息，不绕过后台 Authority。  
Risk: low  
Estimated blast radius: medium  
Can run in parallel with: None

---

# Phase 5 — CB-005 CodeBuddy ACP Contract Probe

## CB5-001 — CodeBuddy Binary / Version / Hash Probe

Phase: Phase 5  
Type: contract-test (E)  
Goal: 固定本机真实 CodeBuddy binary identity 和版本发现规则。  
Why now: 后续任何 ACP 行为必须绑定固定 binary 证据。  
Dependencies: CB4-003 (H)  
Blocked by: 本机已安装且可运行 CodeBuddy  
Allowed scope: 临时目录、CodeBuddy CLI、docs/tasks/evidence/CB-005。  
Forbidden scope: 修改产品 Runtime、自动安装/升级/登录。  
Contract references: 设计 §14.2～§14.2.1、§28。  
Implementation requirements: absolute path、--version 原始输出、parsed version、SHA-256；证据脱敏。  
Non-goals: ACP Session。  
Tests required: found / missing / malformed version；实际 binary probe。  
Evidence required: verification.md + binary.sha256。  
Acceptance criteria: supported-version 候选可以精确引用 binary。  
Rollback / failure behavior: binary 不可用则 Phase 5 BLOCKED，不改产品代码。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: None

## CB5-002 — Managed Pipe → Official ACP SDK Initialize Probe

Phase: Phase 5  
Type: contract-test (E)  
Goal: 证明 SerenaDesktop 外部受管 stdin/stdout 可以接入官方 Rust ACP SDK。  
Why now: 决定首版用官方 SDK 还是最小 NDJSON fallback。  
Dependencies: CB5-001 (H)  
Blocked by: Windows + CodeBuddy binary  
Allowed scope: test/probe harness、临时 process；不进入生产 Provider。  
Forbidden scope: 让 SDK 自己成为 Process Authority。  
Contract references: 设计 §14.3、CB-005。  
Implementation requirements: 受控 spawn/pipe；ACP initialize；记录 protocolVersion/capabilities；证明 SDK 可消费 external streams。  
Non-goals: Workspace write。  
Tests required: initialize success、EOF、invalid protocol version、clean shutdown。  
Evidence required: initialize.jsonl + SDK/version details。  
Acceptance criteria: official SDK path PASS；若 FAIL，明确冻结最小 NDJSON fallback method set。  
Rollback / failure behavior: 禁止为适配 SDK 放弃 Job-at-creation。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

## CB5-003 — Fresh Session / Prompt / Activity Contract

Phase: Phase 5  
Type: contract-test (E)  
Goal: 固定 session/new、cwd、session/prompt、session/update、terminal 的真实 wire。  
Why now: Fresh Start 是第一版核心能力。  
Dependencies: CB5-002 (H)  
Blocked by: ACP initialize PASS  
Allowed scope: 临时 Workspace + Probe harness。  
Forbidden scope: 修改真实项目。  
Contract references: 设计 §15、§19、§21、§28.1。  
Implementation requirements: exact session id、cwd、request identity、terminal stop reason、update ordering；检查是否需要 conversationRequestId。  
Non-goals: Continue。  
Tests required: read-only prompt、isolated write prompt、terminal/error、activity update。  
Evidence required: fresh-session.jsonl + sanitized summary。  
Acceptance criteria: canExecute 所需 wire 全部一手证明。  
Rollback / failure behavior: 缺少 exact identity → canExecute 不得开放。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

## CB5-004 — Cancel / Permission Contract

Phase: Phase 5  
Type: contract-test (E)  
Goal: 固定 session/cancel 和 session/request_permission 的真实收敛语义。  
Why now: Cancel 与 Permission 都涉及已发生副作用后的安全终态。  
Dependencies: CB5-003 (H)  
Blocked by: 可稳定触发长任务/权限请求  
Allowed scope: 临时 Workspace + Probe harness。  
Forbidden scope: bypassPermissions 默认、真实用户项目破坏性命令。  
Contract references: 设计 §18、§22、§28.3～§28.5。  
Implementation requirements: cancel terminal 是否必达、timeout、permission options、deny 后是否有 terminal、已有副作用场景。  
Non-goals: 自动批准。  
Tests required: cancel before/after side effect、permission deny、permission response malformed。  
Evidence required: cancellation.jsonl、permission.jsonl。  
Acceptance criteria: canCancel 与 permission fail-closed 路径有证据。  
Rollback / failure behavior: 无 terminal 时只能依赖后续 Runtime termination，不伪造 cancelled。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

## CB5-005 — Continuation / Usage / Crash Contract Freeze

Phase: Phase 5  
Type: contract-test + DCR (E)  
Goal: 冻结所有可选高级能力和 Provider-private persistence 所需真实字段。  
Why now: Runtime/Store 实现不能猜 session recovery 和 usage 语义。  
Dependencies: CB5-004 (H)  
Blocked by: CodeBuddy 实际能力  
Allowed scope: Probe harness、evidence docs、临时 Workspace。  
Forbidden scope: 直接改 Provider capabilities=true。  
Contract references: 设计 §16、§20、§23、§28。  
Implementation requirements:
- 跨 Runtime 恢复真实 method name/params；
- exact session lineage；
- Usage cumulative/per-turn/reset/late event；
- crash windows；
- Provider-private state 必需字段；
- 若需要新 DB schema，形成 bounded DCR / next schema migration contract。
Non-goals: 强行让 Continue/Usage 成功。  
Tests required: R1→R2 continuation、restart、usage multi-turn、crash before/after terminal。  
Evidence required: continuation.jsonl、usage.jsonl、crash-recovery.md、final verification.md。  
Acceptance criteria:
- Continue = proven 或 explicitly unsupported；
- Usage = proven 或 explicitly unsupported；
- Runtime safety requirements明确；
- CodeBuddy private store schema 输入已冻结。
Rollback / failure behavior: 未证明能力保持 false，不阻塞 Fresh Start。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

---

# Phase 6 — CB-006 CodeBuddy Runtime Foundation

## CB6-001 — CodeBuddy Discovery / Registration / Admission Health

Phase: Phase 6  
Type: implementation  
Goal: 注册 CodeBuddyProvider skeleton，并实现无进程 binary/version Admission Health。  
Why now: Runtime 之前先闭合 provider presence。  
Dependencies: CB5-005 (H)  
Blocked by: supported-version evidence  
Allowed scope: agent/codebuddy/discovery.rs、provider registration bootstrap、Registry tests。  
Forbidden scope: execute/session、自动安装、unknown version override。  
Contract references: 设计 §6.1、§14.2.1。  
Implementation requirements:
- where.exe / absolute path；
- version parse；
- supported-version table；
- deterministic incompatibility → Unavailable；
- execution-local error不在此层。
Non-goals: ACP initialize。  
Tests required: missing、supported、unsupported、malformed version。  
Evidence required: focused tests。  
Acceptance criteria: Desktop 在 CodeBuddy 缺失时仍正常启动；Codex 不受影响。  
Rollback / failure behavior: CodeBuddy unavailable only。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: None

## CB6-002 — CodeBuddy Windows Job-at-Creation Launcher

Phase: Phase 6  
Type: implementation  
Goal: 为 CodeBuddy 建立与现有安全模型同级的 Windows Process ownership。  
Why now: ACP 不能先于 Runtime containment。  
Dependencies: CB6-001 (H)  
Blocked by: Windows  
Allowed scope: agent/codebuddy/windows_launcher.rs 或安全抽取的共享低层 launcher、tests。  
Forbidden scope: spawn 后 AssignProcessToJobObject、shell launcher、修改 Codex 安全语义。  
Contract references: 设计 §13、§23.1、§31.7～§31.9。  
Implementation requirements: PROC_THREAD_ATTRIBUTE_JOB_LIST、KILL_ON_JOB_CLOSE、no breakaway、non-inheritable Job、explicit stdio handle list、absolute path/argv。  
Non-goals: ACP parsing。  
Tests required: membership from first runnable instant、child process containment、handle inheritance、terminate job、policy validation。  
Evidence required: Windows launcher tests。  
Acceptance criteria: 无 Job escape window。  
Rollback / failure behavior: launcher uncertainty → no process published，Runtime unknown/quarantined as applicable。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## CB6-003 — Managed ACP Transport / Initialize

Phase: Phase 6  
Type: implementation  
Goal: 将受管 CodeBuddy process 的 stdin/stdout 连接到 Probe 冻结的 ACP client path。  
Why now: 建立 Runtime protocol handshake。  
Dependencies: CB6-002 (H)  
Blocked by: CB5-002 SDK/fallback decision  
Allowed scope: codebuddy/client.rs、protocol.rs、runtime.rs focused tests。  
Forbidden scope: session/prompt、Workspace writes。  
Contract references: 设计 §14.3、§15。  
Implementation requirements: initialize protocolVersion/capabilities exact validation；deterministic incompatible 才 mark global Unavailable；stdio EOF/timeout 只作为 execution-local failure。  
Non-goals: Session。  
Tests required: initialize success、major mismatch、missing capability、EOF、timeout、invalid NDJSON。  
Evidence required: focused tests tied to Probe fixture。  
Acceptance criteria: Contract Health 分类符合 §6.1。  
Rollback / failure behavior: 协议失败终止受管 Job。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## CB6-004 — CodeBuddy Provider-Private State Store

Phase: Phase 6  
Type: implementation  
Goal: 按 CB5-005 冻结字段持久化 CodeBuddy session/runtime/prompt provenance。  
Why now: acceptance、continue、recovery 都需要 durable private identity。  
Dependencies: CB6-003、CB5-005 (H)  
Blocked by: Provider-private schema DCR 如有  
Allowed scope: agent/codebuddy/store.rs + 对应新 schema migration / tests。  
Forbidden scope: 将 session_id 写入 thread_id/turn_id、把 private identity放入 public Provider Port。  
Contract references: 设计 §15.2、§16、§23。  
Implementation requirements: exact execution/runtime/session/prompt identity；OCC/transaction；历史 Codex 行不受影响；若需 v12 之后的独立 schema migration，版本与迁移必须由 CB5-005 DCR 明确，不复用 v12。\
Non-goals: Result recovery。  
Tests required: persist/read/restart/corruption/provider mismatch。  
Evidence required: schema + store matrix。  
Acceptance criteria: private state 只由 CodeBuddy Adapter 解释。  
Rollback / failure behavior: private state缺失/冲突 → fail-closed，不推断。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## CB6-005 — Runtime Termination Evidence / Startup Reconcile

Phase: Phase 6  
Type: implementation  
Goal: CodeBuddy Runtime 能形成 provider-neutral termination evidence，并在启动时安全 reconcile。  
Why now: canRecover 的真正 Gate。  
Dependencies: CB6-004 (H)  
Blocked by: None  
Allowed scope: codebuddy/runtime.rs、recovery.rs、TaskManager startup reconcile adapter、tests。  
Forbidden scope: Session continuation、Provider ID release 特判。  
Contract references: 设计 §23、§31.19、CB-006 Gate。  
Implementation requirements:
- Job active processes zero / managed job destroyed；
- original runtime ownership；
- PID 不作 termination evidence；
- unresolved → unknown + Claim retained；
- startup_reconcile report 只返回 generic ProviderReconcile kinds；
- canRecover 在本任务 Gate PASS 前保持 false。
Non-goals: Fresh prompt。  
Tests required: host restart、orphan runtime、missing Job evidence、runtime provider mismatch、disabled Provider startup reconcile。  
Evidence required: recovery matrix。  
Acceptance criteria: Gate PASS 后 CodeBuddy 才允许 canRecover=true。  
Rollback / failure behavior: evidence uncertainty retain Claim。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

---

# Phase 7 — CB-007 CodeBuddy Fresh Execution Vertical Slice

## CB7-001 — CodeBuddyProvider Descriptor / Capabilities

Phase: Phase 7  
Type: implementation  
Goal: 以真实 Adapter 注册 CodeBuddy，并只 advertise 已实现+已证明能力。  
Why now: Fresh execute 前先锁死能力表。  
Dependencies: CB6-005 (H)  
Blocked by: None  
Allowed scope: agent/codebuddy/provider.rs、Registry tests。  
Forbidden scope: 提前 canContinue/tokenUsage=true。  
Contract references: 设计 §7 capability rule、§33。  
Implementation requirements: display/version；canExecute 初始按 CB5/7 Gate；canCancel 根据 CB5-004；canRecover 根据 CB6-005；activity根据实现；continue/usage false。  
Non-goals: execute body。  
Tests required: exact capability combinations。  
Evidence required: descriptor snapshot。  
Acceptance criteria: capability 永不超过 evidence。  
Rollback / failure behavior: 未证明能力 false。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: None

## CB7-002 — Fresh Session / Workspace Binding / Acceptance

Phase: Phase 7  
Type: implementation  
Goal: Start 创建受管 Runtime，初始化 ACP，session/new 绑定 frozen canonical root，并建立 acceptance boundary。  
Why now: 这是 Provider side-effect 前最关键边界。  
Dependencies: CB7-001 (H)  
Blocked by: None  
Allowed scope: codebuddy/provider/runtime/client/store、focused TaskManager tests。  
Forbidden scope: Continue、Usage。  
Contract references: 设计 §15.1、§17。  
Implementation requirements:
- cwd 只来自 Execution canonical_workspace_root；
- private session identity durable 后再 acceptance；
- acceptance 必须早于 session/prompt side-effect boundary且满足 Probe 证据；
- no client fs/terminal capabilities；
- workspace/provider/runtime identity exact。
Non-goals: terminal/result。  
Tests required: wrong cwd、session identity mismatch、acceptance ordering、initialize failure no acceptance。  
Evidence required: ordering test trace。  
Acceptance criteria: session/new 前后 crash window 可分类。  
Rollback / failure behavior: acceptance 前失败 → rejection，不 dispatch。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## CB7-003 — Prompt / Terminal / Result Mapping

Phase: Phase 7  
Type: implementation  
Goal: session/prompt 正常执行并映射 ProviderRunResult。  
Why now: 完成最小真实执行链。  
Dependencies: CB7-002 (H)  
Blocked by: None  
Allowed scope: CodeBuddy provider/client/protocol result assembly、tests。  
Forbidden scope: Claim release shortcut、Usage。  
Contract references: 设计 §21。  
Implementation requirements: exact session/prompt identity；terminal stop reason mapping；result completeness 保守；ProviderRunResult 不含 safety evidence。  
Non-goals: Crash result recovery。  
Tests required: completed/failed/cancelled-ish terminal fixtures、malformed/late/wrong-session response。  
Evidence required: ACP fixture mapping tests。  
Acceptance criteria: wrong identity event drop/reject，不污染 Execution。  
Rollback / failure behavior: terminal不确定 → interrupted/reconciling path，不 completed。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## CB7-004 — ACP Activity Projection

Phase: Phase 7  
Type: implementation  
Goal: session/update 经 CodeBuddy Adapter 映射成安全 Activity。  
Why now: ChatGPT / UI 需要执行过程反馈。  
Dependencies: CB7-003 (H)  
Blocked by: None  
Allowed scope: codebuddy adapter + AgentEventSink/telemetry tests。  
Forbidden scope: 原样暴露 agent message/command/stdout、生命周期 Authority。  
Contract references: 设计 §19。  
Implementation requirements: Read/Edit/Command/Test/Build/Tool 安全分类；无法可靠分类→Tool；identity validation before publish。  
Non-goals: Usage。  
Tests required: known/unknown tool update、wrong session、late update、forged terminal activity。  
Evidence required: Activity snapshots。  
Acceptance criteria: Activity 不改变 terminal/Claim。  
Rollback / failure behavior: 无法映射就 drop/Tool，不推测。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: None

## CB7-005 — Fresh Execution Finalization / Atomic Claim Release

Phase: Phase 7  
Type: implementation + integration-test  
Goal: Fresh CodeBuddy 执行在 terminal 后终止 Runtime，形成 evidence，并走既有 atomic finalization。  
Why now: 完整闭合 workspace_write 安全。  
Dependencies: CB7-004 (H)  
Blocked by: Windows real runtime test  
Allowed scope: codebuddy provider/runtime、generic finalize integration、isolated test Workspace。  
Forbidden scope: provider==codebuddy release shortcut。  
Contract references: 设计 §21.1、§23、§31。  
Implementation requirements:
- terminal result persist；
- terminate Job；
- ActiveProcesses==0 / approved termination evidence；
- generic release_evidence_kind=runtime_terminated；
- terminal + Claim release atomic；
- Codex same_runtime_cleanup path不变。
Non-goals: Continue。  
Tests required: real isolated workspace write、terminal-before-termination crash window、termination failure、claim retention、Codex regression。  
Evidence required: state transition + DB evidence matrix。  
Acceptance criteria: Fresh Start vertical slice PASS；canExecute 才可 advertise true。  
Rollback / failure behavior: termination evidence不足 → unknown/Claim retained。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

---

# Phase 8 — CB-008 Cancel / Permission / Continue / Recovery

## CB8-001 — CodeBuddy Cancel Runtime Path

Phase: Phase 8  
Type: implementation  
Goal: session/cancel 与现有 cancel_requested/cancelling 状态机对齐。  
Why now: Fresh Start 可用后先闭合用户取消。  
Dependencies: CB7-005 (H)  
Blocked by: None  
Allowed scope: codebuddy provider cancel/client/runtime tests。  
Forbidden scope: cancel 直接释放 Claim。  
Contract references: 设计 §22。  
Implementation requirements: persisted cancel intent first；exact session cancel；bounded wait；terminal若有则保存；最终 Runtime termination evidence 收敛。  
Non-goals: Continue。  
Tests required: cancel before side effect、during write、terminal arrives、timeout、provider unavailable但 registered。  
Evidence required: cancellation matrix。  
Acceptance criteria: canCancel=true 仅在本任务 PASS。  
Rollback / failure behavior: timeout → terminate/reconcile，不伪造 cancelled。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## CB8-002 — Permission Deny Convergence

Phase: Phase 8  
Type: implementation  
Goal: 实现 session/request_permission 的首版 fail-closed client response。  
Why now: 真实 CodeBuddy 任务可能在正常流程触发权限请求。  
Dependencies: CB8-001 (H)  
Blocked by: CB5-004 Permission contract  
Allowed scope: codebuddy client/provider/activity/runtime tests。  
Forbidden scope: auto approve、permanent allow、Remote approval UI。  
Contract references: 设计 §18。  
Implementation requirements:
- exact identity；
- Probe-frozen deny response；
- Activity安全提示；
- deny不是 terminal；
- 有 terminal按真实 terminal；
- 无 terminal→Runtime termination→reconciling/interrupted。
Non-goals: 权限管理系统。  
Tests required: deny before/after file side effect、terminal/no-terminal、malformed permission request。  
Evidence required: permission matrix。  
Acceptance criteria: deny 本身从不授权 Claim release。  
Rollback / failure behavior: uncertainty fail-closed。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

## CB8-003 — CodeBuddy Continuation（条件任务）

Phase: Phase 8  
Type: implementation / conditional  
Goal: 若 CB5-005 证明跨 Runtime Session continuation，接入 Continue；否则明确 SKIPPED_UNSUPPORTED。  
Why now: Continue 是增强能力，不阻塞 Fresh Start。  
Dependencies: CB8-002、CB5-005 (H)  
Blocked by: continuation contract may be unsupported  
Allowed scope: codebuddy provider private continuation/store/client tests。  
Forbidden scope: replay parent prompt、复用旧 Runtime、换 Provider。  
Contract references: 设计 §16、§10.1。  
Implementation requirements:
- source Execution provider/taskRole frozen；
- new child Execution；
- new Runtime；
- Probe-frozen recovery method；
- exact session/cwd/lineage validation；
- bind后 acceptance；
- no dual load/resume guessing。
Non-goals: 模拟 continuation。  
Tests required: R1→R2 success、wrong session、wrong cwd、missing history、disabled Provider、current policy changed。  
Evidence required: continuation test matrix。  
Acceptance criteria: PASS 才 canContinue=true；若 unsupported，记录证据且保持 false。  
Rollback / failure behavior: validation失败 → AGENT_CONTINUE_NOT_ALLOWED / Provider error，不 fresh replay。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## CB8-004 — Crash / Result Recovery Gate

Phase: Phase 8  
Type: implementation + contract-test  
Goal: 把 CodeBuddy crash windows、result recovery、runtime termination recovery 与 startup reconcile 闭合。  
Why now: 完成生产级异常路径。  
Dependencies: CB8-003 or SKIPPED_UNSUPPORTED (H)  
Blocked by: None  
Allowed scope: codebuddy recovery/private store/TaskManager recovery tests。  
Forbidden scope: 新 Runtime 证明旧 Runtime 消失、结果恢复替代 termination evidence。  
Contract references: 设计 §23。  
Implementation requirements:
- crash before prompt；
- after side effect；
- after terminal before persist；
- old Runtime termination evidence独立；
- exact history recovery若可用；
- result incomplete可 interrupted；
- missing Job evidence → unknown + Claim。
Non-goals: 提高 result completeness。  
Tests required: full crash matrix + restart twice idempotency。  
Evidence required: recovery matrix。  
Acceptance criteria: 没有旧 Runtime evidence 时绝不释放 Claim。  
Rollback / failure behavior: unknown fail-closed。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

---

# Phase 9 — CB-009 CodeBuddy Usage（条件能力）

## CB9-001 — CodeBuddy Usage Projector（条件任务）

Phase: Phase 9  
Type: implementation / conditional  
Goal: 仅在 CB5-005 Usage contract 被证明时实现 CodeBuddy 公共 Usage。  
Why now: Usage 是可选 telemetry，不应阻塞 Agent 基础能力。  
Dependencies: CB8-004、CB5-005 (H)  
Blocked by: usage contract may be unsupported  
Allowed scope: codebuddy usage adapter、generic execution_usage writer、tests。  
Forbidden scope: 复用 Codex thread epoch/baseline、猜 total、null→0。  
Contract references: 设计 §20、§33。  
Implementation requirements: provider_id=codebuddy；epoch/reset 规则完全按 Probe；public completeness unknown/partial/complete 保守投影。  
Non-goals: 生命周期 Authority。  
Tests required: fresh/multi-turn/restart/late event/counter regression。  
Evidence required: Usage matrix。  
Acceptance criteria: contract + implementation PASS 才 tokenUsage=true；否则 SKIPPED_UNSUPPORTED。  
Rollback / failure behavior: unsupported → tokenUsage=false + unknown/null。  
Risk: high  
Estimated blast radius: medium  
Can run in parallel with: None

## CB9-002 — Usage Product/UI Regression Gate

Phase: Phase 9  
Type: contract-test  
Goal: 确保 Codex 与 CodeBuddy Usage 可以并存，未支持 Provider 仍稳定显示 unknown。  
Why now: 防止 generic Product 再次被某 Provider private semantics 污染。  
Dependencies: CB9-001 or SKIPPED_UNSUPPORTED (H)  
Blocked by: None  
Allowed scope: Product/frontend tests。  
Forbidden scope: 修改 Runtime。  
Contract references: 设计 §20.1、CB-009。  
Tests required: Codex complete/partial、CodeBuddy supported or unknown、historical unknown provider。  
Evidence required: Product/UI snapshots。  
Acceptance criteria: Provider Usage 互不污染；tokenUsage advertising准确。  
Rollback / failure behavior: telemetry failure 不影响 Execution lifecycle。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: None

---

# Phase 10 — CB-010 Full Acceptance / Release Gate

## CB10-001 — Automated Full Regression Matrix

Phase: Phase 10  
Type: contract-test  
Goal: 跑完整自动化 Gate，确认 Multi-Agent 改造没有破坏现有 SerenaDesktop。  
Why now: 真实人工 E2E 前先排除机械回归。  
Dependencies: CB9-002 (H)  
Blocked by: None  
Allowed scope: tests only；发现问题只修对应 owning phase。  
Forbidden scope: 为过 Gate 扩设计。  
Contract references: 设计 §31、§32。  
Tests required:
- cargo fmt/check/clippy/test；
- npm test/build；
- MCP schema/contract；
- State migration；
- Runtime/Recovery/Claim；
- Provider routing；
- Agent UI；
- git diff --check。
Evidence required: command matrix + failing test owner mapping。  
Acceptance criteria: 新增回归为 0；历史基线差异解释完整。  
Rollback / failure behavior: FAIL 返回 owning Task，不跳到 manual E2E。  
Risk: medium  
Estimated blast radius: small  
Can run in parallel with: None

## CB10-002 — Local Agent 管理 Manual Acceptance

Phase: Phase 10  
Type: acceptance-test (A)  
Goal: 由 Host 在桌面应用真实验收 Provider 管理和角色路由 UI。  
Why now: 状态、文案、Drain、Pending Claim 无法只靠单测保证体验。  
Dependencies: CB10-001 (H)  
Blocked by: 本地构建可运行  
Allowed scope: 手工操作，不临时修改代码。  
Forbidden scope: 绕过 UI 直接改 config 伪造 PASS。  
Contract references: 设计 §25、CB-004 Gate。  
Tests required:
- Codex/CodeBuddy enable/disable；
- Role set/clear；
- disabled binding；
- unsupported version文案；
- running drain；
- pending claim warning；
- no Force Unlock。
Evidence required: Host PASS/FAIL 记录 + 必要截图。  
Acceptance criteria: 所有 UI contract 人工通过。  
Rollback / failure behavior: FAIL 回到 CB4 owning task。  
Risk: low  
Estimated blast radius: small  
Can run in parallel with: None

## CB10-003 — Real ChatGPT Multi-Agent E2E / Design Closeout

Phase: Phase 10  
Type: acceptance-test + release gate (A/E)  
Goal: 用真实 ChatGPT → MCP → Codex/CodeBuddy 完成一次开发+测试 Work，并关闭 Design Freeze。  
Why now: 这是本功能最终用户路径。  
Dependencies: CB10-002 (H)  
Blocked by: Remote MCP / CodeBuddy / Codex 可用  
Allowed scope: 隔离测试 Workspace 或 Host 指定项目；真实工具链。  
Forbidden scope: 自动 fallback、跳过 observe/review、未知 Claim 强制解锁。  
Contract references: 设计 §11、§32 CB-010、§33。  
Implementation requirements:
1. agent_query providers；
2. Work begin；
3. development → configured Provider（通常 Codex）；
4. observe + source/git review；
5. testing → configured Provider（通常 CodeBuddy）；
6. observe + result review；
7. Work finish；
8. legacy Start 缺 taskRole/providerId → general route 的真实兼容测试；
9. CodeBuddy disabled/unavailable failure path；
10. Desktop restart / recovery 至少一条真实验证。
Tests required: 上述人工真实流程。  
Evidence required: Execution IDs、Provider/Role、terminal/result、Claim release、关键日志、Host acceptance。  
Acceptance criteria:
- Work 可包含不同 Provider 的顺序 Execution；
- 每个 Execution provider/role 正确冻结；
- 不发生 Workspace 串线；
- 不发生自动 fallback；
- Claim 最终安全释放；
- unsupported capability 仍保持 false；
- Codex 原路径无回归。
Rollback / failure behavior: 任一安全 Gate 失败不得发布；按 owning phase 回修。  
Risk: high  
Estimated blast radius: small  
Can run in parallel with: None

---

# 11. 默认推进顺序

默认严格顺序：

~~~text
CB0-001
CB0-002
CB0-003

CB1A-001
CB1A-002
CB1A-003
CB1A-004
CB1A-005

CB1B-001
CB1B-002
CB1B-003

CB1C-001
CB1C-002
CB1C-003
CB1C-004

CB2-001
CB2-002
CB2-003
CB2-004
CB2-005

CB3-001
CB3-002
CB3-003
CB3-004
CB3-005

CB4-001
CB4-002
CB4-003

CB5-001
CB5-002
CB5-003
CB5-004
CB5-005

CB6-001
CB6-002
CB6-003
CB6-004
CB6-005

CB7-001
CB7-002
CB7-003
CB7-004
CB7-005

CB8-001
CB8-002
CB8-003
CB8-004

CB9-001
CB9-002

CB10-001
CB10-002
CB10-003
~~~

其中只有以下是“可以明确 SKIPPED_UNSUPPORTED 而不阻塞基础发布”的条件任务：

~~~text
CB8-003  CodeBuddy Continuation
CB9-001  CodeBuddy Usage
~~~

CB8-003 被跳过时：

~~~text
canContinue = false
Fresh Start / Cancel / Recovery 继续可发布
~~~

CB9-001 被跳过时：

~~~text
tokenUsage = false
公共 Usage = unknown/null
基础 Agent 功能继续可发布
~~~

---

# 12. Phase Gate 总览

| Gate | 必须证明 | 通过后允许 |
|---|---|---|
| Phase 0 | 当前 v11 baseline 与直接迁移 fixture 明确，可选 v9 历史 fixture | 修改数据契约 |
| Phase 1A | v12 / provider / runtime ownership 安全 | 第二 Provider 可持久化 |
| Phase 1B | v3 + legacy retry 正确 | role/provider 进入公开 Start |
| Phase 1C | Product/Usage Provider-neutral | Remote Provider Catalog |
| Phase 2 | Local Human Policy 有唯一 Authority | Remote routing |
| Phase 3 | MCP Start/Continue 契约闭合 | UI 与真实 Provider 接入 |
| Phase 4 | Agent 管理 UX 可用 | CodeBuddy Contract Probe 后产品化 |
| Phase 5 | CodeBuddy ACP 一手证据 | CodeBuddy Runtime |
| Phase 6 | Job ownership / startup reconcile 安全 | workspace_write Fresh Start |
| Phase 7 | Fresh Start + atomic release | Cancel/Continue/Recovery |
| Phase 8 | 异常与继续路径闭合 | 可选 Usage |
| Phase 9 | Usage 安全或明确 unsupported | 全量回归 |
| Phase 10 | 自动 + 人工 + ChatGPT E2E PASS | Design Freeze / release |

---

# 13. 必须保持的全局 Architecture Gates

任何任务完成后都不得破坏：

1. AgentTaskManager / Product / Work / MCP 不得出现新的 CodeBuddy 协议类型依赖。
2. Generic Control Plane 不得出现 provider == codebuddy 作为业务分支。
3. Execution 创建后 Provider immutable。
4. Continue 继承 Provider 与 taskRole。
5. Role Routing 是 Local Human Policy，不是 Provider Capability。
6. Remote MCP 不修改 Provider Policy。
7. Provider disabled 不等价 unregister / unavailable。
8. Cancel / startup_reconcile 对 disabled Provider 仍可达。
9. Provider Runtime 从第一个可运行时刻进入受管 Job。
10. PID death 不等于 Runtime termination evidence。
11. ACP Session recovery 不证明旧 Runtime 消失。
12. ProviderRunResult 不携带 Claim Release authorization。
13. Claim release 只依赖 persisted evidence。
14. terminal + Claim release 保持原子事务。
15. unknown side effect 不 replay。
16. 不自动 Provider fallback。
17. non-Codex Execution 不进入 Codex private Usage。
18. CodeBuddy Session identity 不写入 Codex thread_id / turn_id。
19. 未通过 Evidence Gate 的 capability 必须 false。
20. CodeBuddy / ACP 无法支持的增强能力可以明确 unsupported，但不得降低 Workspace Safety。

---

# 14. 完成定义

只有同时满足以下条件，CodeBuddy / Multi-Agent Provider V0.1 才可以从 Design Freeze Candidate 进入完成状态：

~~~text
Persistence Gate PASS
Request Identity Gate PASS
Product/Usage Neutralization PASS
Local Provider Policy PASS
MCP Routing PASS
Agent 管理 UI PASS
CodeBuddy Contract Probe PASS
Runtime Safety Gate PASS
Fresh Execution PASS
Cancel/Permission/Recovery PASS
Continue = PASS or explicitly unsupported
Usage = PASS or explicitly unsupported
Full Regression PASS
Local Manual Acceptance PASS
Real ChatGPT Multi-Agent E2E PASS
~~~

最终应能稳定表达：

~~~text
用户：
development → Codex
testing     → CodeBuddy

ChatGPT：
读取 Provider Catalog
→ 创建 Work
→ Codex Execution 实现
→ Review
→ CodeBuddy Execution 测试
→ Review
→ Finish Work

SerenaDesktop：
冻结 Workspace / Provider / Role
管理 Runtime
验证 Evidence
原子释放 Claim
不猜测、不串线、不自动 fallback
~~~
