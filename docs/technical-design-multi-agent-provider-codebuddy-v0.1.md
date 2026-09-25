# SerenaDesktop Multi-Agent Provider 与 CodeBuddy ACP 接入技术方案 V0.1

状态：Design Freeze Candidate  
日期：2026-09-21  
适用范围：SerenaDesktop Agent Platform V0.2 之后的多 Provider 增量设计

关联文档：

- docs/technical-design-agent-platform-v0.2.md
- docs/codex-agent-runtime.md
- docs/codex-agent-observe.md
- docs/core-work-orchestration.md
- docs/implementation-task-breakdown-agent-platform-v0.2-revision003.md

本文是现有 Agent Platform 的增量方案，不替换既有 Runtime Foundation、Workspace、Work、Execution、Claim、Observe、Usage 和 Provider Port 契约。目标是在当前 Provider-Agnostic Control Plane 上接入第二个真实 Agent Provider：CodeBuddy，并补齐本地 Agent 管理和基于用户角色配置的任务路由。

---

# 1. 目标

本版本完成三组能力：

1. SerenaDesktop 从单一 Codex Provider 扩展为至少支持 Codex + CodeBuddy 的多 Provider Agent Host；
2. 原“Agent”菜单升级为“Agent 管理”，本地用户可以显式启用、停用 Provider，并配置每类任务应优先交给哪个 Provider；
3. CodeBuddy 通过官方 ACP（Agent Client Protocol，智能体客户端协议）接入，首版采用 stdio NDJSON，由 SerenaDesktop 持有进程、Runtime 和安全收敛责任。

目标交互模型：

~~~text
Local Human
   │
   ├── Agent 管理
   │     ├── 启用 / 停用 Codex
   │     ├── 启用 / 停用 CodeBuddy
   │     └── 配置角色路由
   │
   ▼

ChatGPT
   │
   ├── agent_query(providers)
   │       ↓
   │   Provider Catalog + Role Routing Policy
   │
   ├── 判断当前任务角色
   │       ↓
   │   development / testing / review / analysis / general
   │
   └── agent_execute(start, taskRole, providerId, ...)
           ↓
      SerenaDesktop 校验用户策略
           ↓
      ProviderRegistry
       /        \
      /          \
 CodexProvider   CodeBuddyProvider
      │                │
 Codex App Server      ACP
      │                │
 Codex Runtime   CodeBuddy Runtime
~~~

核心边界：

> 用户决定哪些 Provider 可用以及角色分工；ChatGPT 负责识别当前任务角色并显式选择 Provider；SerenaDesktop 负责校验策略、持久化 Execution 身份、管理 Runtime 和安全收敛。

---

# 2. 当前基线

当前仓库已经具备多 Provider 所需的大部分公共骨架：

- ProviderId、ProviderDescriptor、ProviderCapabilities 已存在；
- AgentProvider 已是 object-safe Port；
- ProviderRegistry 已支持注册、查询、Health、Capabilities；
- execute / continue 走 health-gated get()；
- cancel / startup reconcile 可以走 registration-only get_registered()；
- Continue 已逐步迁移为 Provider-owned continuation validation；
- Execution 已持久化 provider；
- Work 与 Execution 已分离；
- Workspace 在 Execution 创建时冻结；
- Observe / Activity / Usage 已建立 Provider-Agnostic 投影边界。

现有目标设计已经明确：

~~~text
Agent Control Plane
        │
        ▼
ProviderRegistry
        │
        ▼
Arc<dyn AgentProvider>
~~~

CodeBuddy 必须作为新的 AgentProvider Adapter 接入，不建立第二套 Agent Control Plane。

## 2.1 当前仍存在的单 Provider 遗留

在真正写 CodeBuddy Adapter 前，需要先处理两处当前实现事实。

### Execution Provider 约束

初始 SQLite Schema 的 executions.provider 仍包含：

~~~sql
provider TEXT NOT NULL CHECK(provider = 'codex')
~~~

因此当前数据库物理契约仍不能合法保存：

~~~text
provider = codebuddy
~~~

### Rust Provider 类型与 Store 写入仍是单 Provider

当前 `CreateExecutionInput` 虽然已经携带 `provider`，但 `execution::Provider` 仍只有一个 `Codex` 变体；同时 `insert_execution()` 的 SQL 仍以字面量 `'codex'` 写入 `executions.provider`，没有使用请求中已校验的 Provider identity。

因此 CB-001 不能只修改数据库 CHECK，还必须同时闭合：

- Execution 创建输入的 Provider identity；
- `ProviderId` 与持久化 Provider 的单一 Authority；
- `insert_execution()` 的真实 provider 写入；
- Fake Provider、Store fixture 与 Product 测试。

### Product 与 Usage 仍有 Codex 专用路径

当前 `ProviderProduct::from_execution_provider()` 仍只认识 `codex`；Codex Usage Store 也明确包含 `provider_id='codex'`、`codex_execution_usage_state`、terminal grace 和 baseline 等私有逻辑。

这些私有 Codex Usage 逻辑本身不需要泛化，但公共 Product 必须能稳定展示任意已持久化 Provider；非 Codex Execution 不得误入 Codex Usage 私有状态。

### Runtime 表仍含 Codex 专有字段

当前 runtime_instances 包含：

~~~text
codex_executable_path
codex_version
protocol_schema_sha256
codex_pid
codex_process_start_token
~~~

但 Execution 的：

~~~text
runtime_instance_id
provider_terminal_evidence_runtime_instance_id
runtime_termination_evidence_runtime_instance_id
~~~

仍通过外键引用该表。

因此 CodeBuddy 不能绕开现有 Runtime binding 自行建一套与 Execution 无关的安全表，否则会破坏：

- Runtime ownership；
- immutable runtime binding；
- Runtime termination evidence；
- Claim recovery；
- atomic release；
- unknown fail-closed。

本方案将该问题限定为一个有界前置迁移，见第 12 节。

---

# 3. 非目标

首版不实现：

- 根据 Prompt 自动训练或推理一个“智能 Agent 路由器”；
- Provider 自动评分、自动排序；
- Provider 自动故障切换；
- 一个任务同时由多个 Provider 并行写同一 Workspace；
- Workflow DAG；
- Agent-to-Agent 直接通信；
- CodeBuddy Agent Teams；
- CodeBuddy Multitask；
- CodeBuddy HTTP ACP；
- CodeBuddy Web UI；
- CodeBuddy 内部非稳定 HTTP API；
- ACP 客户端代理文件系统；
- ACP 客户端代理终端；
- 动态 Provider Plugin ABI；
- Provider 市场；
- 自动安装 CodeBuddy；
- 自动登录 CodeBuddy；
- 自动修改用户 CodeBuddy 全局权限配置；
- 把 Provider Role 当作客观 Capability；
- 因 Provider 不可用而静默切换到另一个 Provider；
- Provider 常驻进程池或跨 Execution Runtime 复用。

首版只支持编译期内置 Provider：

~~~text
codex
codebuddy
~~~

未来 Trae 等 Provider 继续通过同一 AgentProvider Port 接入。

---

# 4. Agent Provider、Role 与 Routing Policy

必须区分三个概念。

## 4.1 Provider Capability

Capability 是 SerenaDesktop 根据 Provider Contract 得到的客观事实，例如：

~~~text
canExecute
canContinue
canCancel
canRecover
activity
tokenUsage
~~~

Capability 不由用户修改。

## 4.2 Task Role

Role 是 ChatGPT 对“这一项 Execution 要做什么”的业务分类。

首版固定五个：

~~~text
development
testing
review
analysis
general
~~~

语义：

| Role | 中文 | 典型任务 |
|---|---|---|
| development | 开发 | 功能实现、Bug 修复、重构 |
| testing | 测试 | 单测、集成测试、回归验证 |
| review | 评审 | Code Review、风险检查 |
| analysis | 分析 | 排查、阅读代码、技术分析 |
| general | 通用 | 无明确细分职责的执行任务 |

Role 不表示 Provider “只能”做这类任务。

## 4.3 Routing Policy

Routing Policy 是本地用户配置的：

~~~text
Role → Preferred Provider
~~~

例如：

~~~text
development → codex
testing     → codebuddy
review      → codebuddy
analysis    → codex
general     → codex
~~~

第一版固定：

> 一个 Role 最多绑定一个首选 Provider。

不实现：

~~~text
testing → [codebuddy, codex, trae]
~~~

这种优先级候选列表。

如果需要多候选和 fallback，在第三个真实 Provider 接入后再基于真实需求设计。

---

# 5. 配置模型

Provider 开关和 Role Routing 属于 Local Human Authority，保存在现有 ManagerConfig，而不是 Agent StateStore。

建议新增：

~~~rust
pub struct AgentProviderSettings {
    pub providers: BTreeMap<String, AgentProviderPolicy>,
    pub role_routing: BTreeMap<AgentTaskRole, Option<String>>,
}

pub struct AgentProviderPolicy {
    pub enabled: bool,
}
~~~

逻辑配置示例：

~~~json
{
  "agentEnabled": true,
  "agentProviders": {
    "providers": {
      "codex": {
        "enabled": true
      },
      "codebuddy": {
        "enabled": true
      }
    },
    "roleRouting": {
      "development": "codex",
      "testing": "codebuddy",
      "review": "codebuddy",
      "analysis": "codex",
      "general": "codex"
    }
  }
}
~~~

## 5.1 agentEnabled 保留为总开关

当前已有 agentEnabled，首版继续保留，作为 Agent Control Plane 的总开关。

Provider 级 enabled 表示：

> 当 Agent 总开关开启时，该 Provider 是否允许接收新的 Provider work。

关系：

~~~text
agentEnabled = false
    → 所有新 Agent 执行拒绝

agentEnabled = true
provider.enabled = false
    → 该 Provider 不接受新执行
~~~

## 5.2 迁移默认值

旧配置没有 agentProviders 时：

~~~text
codex.enabled = true
codebuddy.enabled = false

development → codex
testing     → codex
review      → codex
analysis    → codex
general     → codex
~~~

这样升级后原有 Codex 行为保持可用，CodeBuddy 只有在用户本地显式开启后才参与任务。

Provider Role 不单独重复存储在 Provider 配置中。

例如 UI 中显示：

~~~text
Codex
角色：开发、分析
~~~

直接从 roleRouting 反向投影，避免出现 Provider.roles 和 Role.preferredProvider 两套可互相冲突的配置 Authority。

---

# 6. Provider 启用 / 停用语义

Provider 始终保持编译期注册。

“停用”不能等价于从 ProviderRegistry unregister，因为已有 Execution 仍可能需要：

- cancel；
- startup reconcile；
- crash recovery；
- historical projection。

因此增加一层 Provider Admission Policy：

~~~text
ProviderRegistry
    │
    ├── registered
    ├── health
    └── admission enabled
~~~

新执行解析顺序：

~~~text
registered?
    ↓
enabled?
    ↓
health available?
    ↓
capability supported?
    ↓
execute
~~~

## 6.1 开启

用户开启 CodeBuddy：

~~~text
enabled = true
    ↓
Admission Health refresh
    ↓
可接收新任务
~~~

首版公开 `ProviderHealth` 继续只使用现有 `Available / Unavailable`，不新增第二套公开 Health 状态机。

其中 Admission Health 定义为：

~~~text
executable exists
+
version parseable
+
version belongs to SerenaDesktop supported-version table
~~~

Admission Health 不创建 Runtime、不连接 ACP、不创建 Session，因此 Provider toggle 和普通 `agent_query providers` 都不会 spawn CodeBuddy。

真正的协议兼容性在执行边界重新验证：

~~~text
agent_execute
    ↓
Create managed Runtime
    ↓
ACP initialize
    ↓
protocolVersion / required capabilities
    ↓
Contract Gate
~~~

Contract Gate 失败时，本次执行按 Provider failure 契约收敛，但只有**确定性 Provider 不兼容 / 不可用事实**才更新全局 Registry health。

冻结分类：

| 失败类型 | Registry health |
|---|---|
| executable 缺失 | `Unavailable` |
| version 不在 supported-version table | `Unavailable` |
| negotiated protocolVersion 确定不兼容 | `Unavailable` |
| required ACP method / capability 确定缺失 | `Unavailable` |
| 已验证的稳定认证前置缺失 | `Unavailable` |
| 单次 Runtime create/start 失败 | 保持当前 health，仅本次 Execution 失败 |
| stdio EOF / timeout / 临时 I/O | 保持当前 health |
| permission deny | 保持当前 health |
| 单次 Prompt / Session 操作失败 | 保持当前 health |

也就是说：

> 只有可重复、确定性的 Provider readiness / compatibility failure 才污染全局 Health；Execution-local failure 只影响本次 Execution。

Health 刷新时机首版固定为：

- Desktop 启动时做一次 Admission Health；
- 用户点击“重新检测”时刷新；
- enable 从 false → true 时刷新；
- execute Contract Gate 发现确定性 incompatibility 时标记 unavailable；
- 单次执行级错误不自动把 Provider 全局标红。

因此 `agent_query providers.health` 是最近一次 Admission/Contract 结果的快照，不是“下一次执行必然成功”的承诺；最终 execute 仍必须重新经过真实 Runtime Contract Gate。

合法 Idle 状态：

~~~text
CodeBuddy
enabled = true
health = available
runtime = stopped
~~~

表示 Provider 已通过无进程 Admission Health，目前没有执行中的 Runtime。

## 6.2 停用

用户停用一个存在 Running Execution 的 Provider 时采用 Drain 语义：

~~~text
enabled = false
    ↓
拒绝新的 Start / Continue / ResumePending
    ↓
既有 Running Execution 继续
    ↓
既有 Cancel 仍可调用
    ↓
startup reconcile 仍可调用
    ↓
最后一个在途 Execution 收敛
    ↓
Provider 进入 disabled
~~~

停用不能直接：

- kill Runtime；
- cancel Execution；
- release Claim；
- 删除 Provider 历史状态。

如果用户希望取消任务，应使用独立的 Execution Cancel 操作。

## 6.3 Disabled Provider 的恢复

即使 provider.enabled = false，启动恢复仍必须：

~~~text
get_registered(providerId)
    ↓
startup_reconcile
~~~

只要该 Provider 声明 canRecover = true，就不能因为用户停用了 Provider 而跳过历史安全收敛。

---

# 7. ChatGPT Provider Discovery

现有 agent_query 增加：

~~~text
action = providers
~~~

输入：

~~~json
{
  "action": "providers"
}
~~~

返回建议：

~~~json
{
  "providers": [
    {
      "id": "codex",
      "displayName": "Codex",
      "version": "0.153.4",
      "enabled": true,
      "health": "available",
      "availableForNewExecution": true,
      "capabilities": {
        "canExecute": true,
        "canContinue": true,
        "canCancel": true,
        "canRecover": true,
        "activity": true,
        "tokenUsage": true
      }
    },
    {
      "id": "codebuddy",
      "displayName": "CodeBuddy",
      "version": "2.153.0",
      "enabled": true,
      "health": "available",
      "availableForNewExecution": true,
      "capabilities": {
        "canExecute": true,
        "canContinue": false,
        "canCancel": true,
        "canRecover": false,
        "activity": true,
        "tokenUsage": false
      }
    }
  ],
  "roleRouting": {
    "development": "codex",
    "testing": "codebuddy",
    "review": "codebuddy",
    "analysis": "codex",
    "general": "codex"
  }
}
~~~

registered、enabled、health、capabilities、roleRouting 是不同事实。

CodeBuddy 的 capability advertisement 必须遵守：

> **advertised capability = 对应实现已完成 ∩ 对应 Evidence Gate 已通过。**

不同 capability 的证明来源分别冻结：

~~~text
canExecute
    = fresh ACP execution 实现完成
    + Fresh Session Contract PASS

canContinue
    = continuation 实现完成
    + 跨 Runtime Session continuation Contract PASS

canCancel
    = cancel 实现完成
    + session/cancel Contract PASS

canRecover
    = startup_reconcile 实现完成
    + Windows Runtime / Job Recovery Gate PASS

activity
    = session/update mapping 已实现并验证

tokenUsage
    = Usage Contract + Usage implementation PASS
~~~

因此 §7 中 CodeBuddy 示例代表“Fresh Start/Cancel/Activity 已可用，但 Continue、Startup Recovery、Usage 尚未通过各自 Gate”的保守首版形态。未证明能力默认 `false`，不能因为 ACP 文档存在相应字段就提前 advertise。

availableForNewExecution 仅是 Product projection，例如：

~~~text
agentEnabled
AND provider.enabled
AND provider.health == available
AND canExecute
~~~

它不替代底层 execute 时的再次校验。

## 7.1 Query 不启动 Runtime

agent_query providers：

- 不创建 Runtime；
- 不连接 CodeBuddy ACP；
- 不创建 Session；
- 不修改 Provider 配置；
- 不修改 Role Routing；
- 不建立 Provider Binding。

Remote MCP 只允许查询 Provider 和 Routing Policy。

修改 Provider 开关 / Role Routing 只允许本地 Tauri IPC。

---

# 8. Start Routing Contract

多 Provider 上线后，新的 agent_execute start 增加：

~~~text
taskRole
providerId
~~~

示例：

~~~json
{
  "action": "start",
  "workRunId": "wrk-1",
  "workspaceId": "project-4",
  "taskRole": "testing",
  "providerId": "codebuddy",
  "requestKey": "run-tests-v1",
  "prompt": "执行相关测试并报告失败原因"
}
~~~

Server 必须按当前本地配置重新校验：

~~~text
agentEnabled == true

provider registered

provider enabled

role configured

role.preferredProviderId == request.providerId

provider health available

provider.canExecute == true
~~~

然后才允许创建并 Dispatch Execution。

## 8.0 MCP Start 兼容策略

`taskRole` 与 `providerId` 对新客户端属于目标必填字段，但为了兼容升级前已经发布的 `agent_execute start`，首版保留一个有界兼容入口。

冻结矩阵：

| 请求形态 | 行为 |
|---|---|
| 同时提供 `taskRole + providerId` | 按新契约严格校验 Role Routing |
| 两者都缺失 | 视为 legacy Start：`taskRole=general`，`providerId` 由当前 `general` Routing Policy 解析 |
| 只提供其中一个 | `INVALID_PARAMS` |
| legacy Start 且 `general` 未配置 | `AGENT_ROLE_NOT_CONFIGURED` |
| 显式 Provider 与当前 Role Routing 不一致 | `AGENT_ROLE_PROVIDER_MISMATCH` |
| Provider disabled / unavailable | 返回对应稳定错误，不 fallback |

legacy Start 只用于协议升级兼容，工具描述和新 UI 均要求先通过 `agent_query providers` 获取当前策略，再显式发送 `taskRole + providerId`。

错误后的标准恢复路径：

~~~text
AGENT_PROVIDER_DISABLED
AGENT_PROVIDER_UNAVAILABLE
AGENT_ROLE_NOT_CONFIGURED
AGENT_ROLE_PROVIDER_MISMATCH
        ↓
agent_query(providers)
        ↓
读取当前 Local Human policy
        ↓
提示用户重新启用 / 修改本地角色路由 / 重试
~~~

ChatGPT 不得仅为了绕过当前 Routing Policy 而把同一任务重新标记成另一个 Role，也不得修改 Local Human 配置。

## 8.1 ChatGPT 负责识别 Role

SerenaDesktop 不解析 Prompt 来猜“开发 / 测试 / 评审”。

正确流程：

~~~text
ChatGPT 理解任务
    ↓
决定 taskRole
    ↓
agent_query(providers)
    ↓
读取用户 Role Routing
    ↓
显式 providerId
    ↓
agent_execute(start)
~~~

SerenaDesktop 只验证用户策略，不做 NLP / LLM 任务分类。

## 8.2 不自动 Fallback

例如：

~~~text
testing → codebuddy
~~~

但 CodeBuddy 当前 unavailable。

SerenaDesktop 返回明确错误。

禁止：

~~~text
CodeBuddy unavailable
    ↓
自动切到 Codex
~~~

因为 Provider 是否已经发生文件、命令或外部副作用，不能由上层猜测。

如果需要改用 Codex，由 ChatGPT / 用户重新决定并创建新的显式 Start。

## 8.3 Role / Provider 错误码

新增 Product / Routing 层稳定错误：

~~~text
AGENT_PROVIDER_DISABLED
AGENT_ROLE_NOT_CONFIGURED
AGENT_ROLE_PROVIDER_MISMATCH
~~~

taskRole 是固定 enum，非法值由 Schema / INVALID_PARAMS 处理。

Provider 自身已有错误码继续保持：

~~~text
AGENT_PROVIDER_NOT_FOUND
AGENT_PROVIDER_UNAVAILABLE
AGENT_PROVIDER_CAPABILITY_UNSUPPORTED
AGENT_PROVIDER_CONTRACT_ERROR
AGENT_PROVIDER_OPERATION_FAILED
~~~

disabled 与 unavailable 不能合并：

~~~text
disabled
= Local Human policy

unavailable
= Provider binary / runtime / contract health fact
~~~

---

# 9. Execution Provider 与 Role 冻结

Execution 创建时必须冻结：

~~~text
workspace_id
canonical_workspace_root
workspace_generation

provider
task_role

execution_profile
~~~

之后：

- Desktop Agent 角色设置变化不修改已有 Execution；
- Provider 开关变化不修改已有 Execution；
- Role Routing 变化不迁移已有 Execution；
- Workspace UI 选择变化不影响已有 Execution。

建议 Execution 增加：

~~~sql
task_role TEXT NOT NULL
    CHECK(task_role IN (
        'development',
        'testing',
        'review',
        'analysis',
        'general'
    ))
~~~

历史 Execution 可以迁移为 `general`。

Role 不是安全 Evidence，因此历史默认值只表示“旧记录未记录细分任务角色”，不用于重建任何 Runtime 或 Claim 事实。

`task_role` 首版采用数据库 CHECK 是有意选择：Role 是 SerenaDesktop 公共业务枚举而不是动态 Provider identity。未来若新增 Role，需要显式 Schema Migration；这与 Provider ID 不写死进数据库 CHECK 的策略不同。

为了保证 CB-001A 可以独立合入并保持应用可运行，`task_role` 的运行时写入在 CB-001A 就必须闭合：

~~~text
CreateExecutionInput.task_role
    default = general

insert_execution()
    显式写入 input.task_role
~~~

schema_v12 可以使用：

~~~sql
task_role TEXT NOT NULL DEFAULT 'general'
~~~

用于历史迁移和旧构造路径兼容，但生产插入路径不得依赖 SQL 隐式 DEFAULT 来掩盖缺失字段。

CB-001A 阶段 canonical request 仍保持现有 `execution-request-v2`，即 task_role 已持久化但**暂不进入 request hash**；CB-001B 再将 request identity 升级为 v3 并把 task_role 纳入 canonical tuple。

## 9.1 Request Key / Hash

当前基线已经是：

~~~text
execution-request-v2
~~~

并且 v2 tuple 已经包含：

~~~text
provider
~~~

本次真正新增的是 `task_role`，因此新 Execution request identity 升级为：

~~~text
execution-request-v3
~~~

v3 在现有 v2 tuple 的稳定语义上追加冻结后的 `task_role`；Provider 继续参与 hash，不重复增加第二次 Provider identity。

历史 Execution 的 `request_hash` **禁止在 Migration 时重写**。兼容规则必须保持有界：

~~~text
new request
    → only generate v3

retry against persisted row
    → current v3 exact match
       OR approved legacy v2/v1 compatibility path
~~~

已有：

- pre-workspace-generation hash；
- pre-C2 continuation hash；

等历史兼容规则继续保留。新增 task_role 后，Store 增加一条只用于旧持久化行的 v2 compatibility：只有历史行迁移得到 `task_role=general`、workspace generation / provider / mode / parent 等既有身份条件都一致时，才允许按原 v2 hash 认定为同一请求。

禁止：

- 为历史记录重新计算并覆盖 hash；
- 把不同 Provider 的请求视为同一 retry；
- 把不同 taskRole 的新请求视为同一 retry；
- 用 Prompt 文本相同代替 canonical request identity。

因此：

~~~text
same requestKey
+
same prompt
+
different provider
或
different taskRole
~~~

对新请求都必须得到 `EXECUTION_REQUEST_KEY_CONFLICT`。

---

# 10. Continue、Cancel 与 ResumePending

## 10.1 Continue

Continue 不接受：

~~~text
workspaceId
providerId
taskRole
~~~

它只能从 Parent Execution 继承：

~~~text
workspace identity
provider
task_role
~~~

例如：

~~~text
E1
provider = codebuddy
taskRole = testing
status = completed

continue(E1)
    ↓

E2
provider = codebuddy
taskRole = testing
~~~

如果用户希望改由 Codex 继续处理，不应使用 Continue，而应创建同一 Work 下的新 Start。

这样不会把 CodeBuddy Session、Codex Thread、未来 Trae Session 混成一条假 Continuation。

Continue 仍使用：

~~~text
ProviderRegistry
    ↓
provider.validate_continuation(sourceExecutionId)
~~~

并同时重新检查：

~~~text
provider enabled
provider health
provider canContinue
~~~

Provider 被本地用户停用后，不允许创建新的 Continue Execution。

Execution Product View 必须返回该 Execution 创建时冻结的：

~~~text
provider
taskRole
~~~

而 `agent_query providers` 返回的是**当前** Role Routing Policy。二者允许不同。

例如 E1 冻结为：

~~~text
provider=codebuddy
taskRole=testing
~~~

之后用户把当前策略改成：

~~~text
testing → codex
~~~

E1 的 `continue` 仍只能验证并继承 CodeBuddy；Product 不增加另一套 routing authority，也不自动迁移。ChatGPT 可通过 ExecutionView 与 providers query 看出“冻结执行路由”和“当前策略”的差异。

## 10.2 Cancel

Cancel 从 persisted Execution.provider 路由。

即使 Provider 当前 disabled 或 health unavailable，仍按现有 registration-only 语义尝试：

~~~text
get_registered()
    ↓
canCancel
    ↓
provider.cancel()
~~~

不得因为 Provider 停用而使历史任务失去取消入口。

## 10.3 ResumePending

ResumePending 是原 Execution 的首次派发恢复。

它：

- 不重新选择 Provider；
- 不重新选择 Role；
- 不重新解析 Role Routing；
- 必须使用原 Execution 已冻结的 provider；
- 必须重新检查 provider enabled；
- 必须重新检查 provider health；
- 保留现有 not_dispatched / Runtime attempt / Claim 安全条件。

如果 Provider 已被用户停用，ResumePending 返回 `AGENT_PROVIDER_DISABLED`，原 Execution 和 Claim 保持不变。

这是安全上的有意 fail-closed，但产品必须形成闭环：Agent 管理 UI 在停用 Provider 时，如果存在该 Provider 的 pending/resumable Execution，必须提示：

~~~text
该 Provider 有待恢复任务仍占用 Workspace Claim。
可以取消任务释放工作区，或重新启用 Provider 后继续恢复。
~~~

停用操作本身不 Force Unlock，也不把 pending Execution 自动改成 cancelled。

---

# 11. Work 中的多 Provider 模型

Work 继续只是业务容器，不绑定 Provider。

例如：

~~~text
Work W1：实现 CodeBuddy Provider

E1
provider = codex
role = development
status = completed

E2
provider = codebuddy
role = testing
status = completed

E3
provider = codex
role = review
status = completed
~~~

合法流程：

~~~text
ChatGPT
    ↓
Codex 开发
    ↓
Source / Git Review
    ↓
CodeBuddy 测试
    ↓
Source / Git Review
    ↓
Codex Review
    ↓
Work Finish
~~~

不新增 Multi-Agent Scheduler。

现有 `executions_one_unresolved_per_agent` 约束继续保持；当前 Product 以 `agent_id = work_run_id` 创建 Execution，因此一个 Work 内同一时间最多一个 unresolved Execution，首版仍采用顺序编排。

不同 Work 可以拥有不同 Provider 的并发 Execution，但仍受既有 Workspace Claim 约束：若两个 Work 都需要同一个 canonical Workspace 的排他执行，后来的 Execution 仍必须等待或被拒绝，不能因为 Provider 不同而绕过 Workspace 写入所有权。

多 Provider 不改变 ChatGPT 当前“一个 Work 聚合多个顺序 Execution”的语义。

---

# 12. 多 Provider Runtime 持久化前置迁移

这是 CodeBuddy 真正 Dispatch 前的强制 Gate。

目标不是把所有 Provider Runtime 统一成一个复杂框架，而是让当前 Execution / Runtime ownership 契约可以合法表达第二个 Provider，同时保持已有 Codex Evidence 原值和安全语义。

2026-09-24 baseline rebase：当前 Agent StateStore 的 `user_version` 为 11。既有 `schema_v10.sql` 已用于 Windows/macOS Runtime containment/platform evidence，`schema_v11.sql` 已用于 durable CommandRun（`command_runs`、`work_command_links`）。本轮不重用或改写这两项历史 migration；多 Provider persistence 的直接迁移输入为 v11，新增迁移固定为：

~~~text
schema_v12
~~~

由 CB-001A 单独实施和 Review。后续 Request Hash 与 Product/Usage 中立化不再混入该 Schema Migration。

## 12.1 Execution Provider Identity

当前存在三层单 Provider 遗留：

~~~text
execution::Provider
    只有 Codex variant

executions.provider
    CHECK(provider = 'codex')

insert_execution()
    SQL 字面量写入 'codex'
~~~

CB-001A 必须把三者同时闭合。

目标：

1. Execution 创建路径使用经过 `ProviderId` validation 的 Provider identity；
2. `insert_execution()` 写入请求中已冻结的 provider；
3. `executions.provider` 保持非空 TEXT，但不再使用 Provider 枚举 CHECK；
4. Provider 是否注册、是否启用、是否健康由 ProviderRegistry / Admission Policy 在创建边界验证；
5. 已创建 Execution 的 persisted provider 是后续 cancel / continue / recovery 的唯一 Provider Authority。

不得长期并存两套互相独立的 Provider identity authority。

## 12.2 Runtime Identity 增加 Provider

`runtime_instances` 增加：

~~~text
provider TEXT NOT NULL
~~~

历史 Runtime 可以可靠迁移为：

~~~text
provider = codex
~~~

原因是旧 Schema 只能由 Codex Runtime 创建；这是历史类型事实，不是新推导的 Runtime termination evidence。

新 Runtime 必须满足：

~~~text
Execution.provider
==
RuntimeInstance.provider
~~~

该校验进入：

- Runtime first bind；
- provider terminal evidence 写入；
- runtime termination evidence 写入；
- startup reconcile；
- finalization / release evidence 验证。

Provider mismatch 返回稳定 contract / evidence error，并保持 Claim fail-closed。

## 12.3 Provider-neutral Process Identity Header

Windows Job ownership 字段继续复用：

~~~text
owner_host_instance_id
job_name
job_session_id
job_creation_mode
job_handle_inheritable
job_kill_on_close
job_breakaway_allowed
job_policy_verified_at
state
termination_evidence_type
termination_evidence_at
termination_evidence_state
~~~

进程身份字段从 Codex 命名收敛为 Provider-neutral 名称：

| 旧列 | v12 目标列 | 历史值策略 | CodeBuddy |
|---|---|---|---|
| `codex_executable_path` | `executable_path` | 原值逐字复制 | CodeBuddy absolute path |
| `codex_version` | `executable_version` | 原值逐字复制 | CodeBuddy version |
| `codex_pid` | `process_id` | 原值复制 | CodeBuddy PID |
| `codex_process_start_token` | `process_start_token` | 原值复制 | CodeBuddy process start token |
| 无 | `provider` | 固定 `codex` | `codebuddy` |
| `protocol_schema_sha256` | `protocol_contract_sha256` | 原 Codex schema hash 原值复制 | ACP v1 schema/contract hash 可用时保存，否则 NULL |

`protocol_contract_sha256` 是可选的 Runtime Contract provenance，不是 Runtime termination evidence。CodeBuddy 不因为缺少该 hash 而伪造默认值；其 negotiated protocolVersion、Session identity 等继续保存在 Provider-private state。

v12 必须原值保留 v10 已建立的 `runtime_platform`、`containment_type`、`process_identity_scheme`、`containment_process_group_id`、`containment_session_id`、`containment_verified_at`，并保留 `runtime_instances_v10_validate_insert` / `runtime_instances_v10_validate_update` 的跨平台 containment 验证语义。字段重命名或表重建不得降低 Windows Job、macOS process group 或 termination evidence 的现有约束。

## 12.4 Provider-private Identity 归属

当前 `executions.thread_id / turn_id` 是历史 Codex compatibility fields。

本次不删除、不回填、不泛化成“通用 Session ID”。

冻结规则：

~~~text
Codex
    thread_id / turn_id
    继续按现有私有契约使用

CodeBuddy
    executions.thread_id = NULL
    executions.turn_id   = NULL
    ACP session / prompt identity
    → CodeBuddy private store
~~~

公共 Control Plane 只把现有 thread/turn 字段作为 backward-compatible provider-opaque display/diagnostic surface，不用于新 Provider routing 或 Claim release。

## 12.5 schema_v12 迁移方式

由于 `executions.provider` 现有 CHECK 无法通过简单 ADD COLUMN 移除，v12 必须执行受测试的表重建，而不是留一个第二 provider 列绕过旧约束。

v11→v12 采用专用 migration contract，并复用 `migrate()` 已经创建的 `TransactionBehavior::Immediate` transaction：

~~~text
migrate()
    ↓
existing IMMEDIATE transaction
    ↓
v12 migration body
    ↓
rebuild runtime_instances
    ↓
copy old rows exactly
    ↓
rebuild executions without provider='codex' CHECK
    ↓
add task_role with historical default general
    ↓
copy old rows exactly
    ↓
recreate indexes / triggers
    ↓
verify dependent FK references
    ↓
PRAGMA foreign_key_check
    ↓
user_version = 12
    ↓
outer transaction COMMIT
~~~

v12 migration function **不得再次执行 BEGIN / BEGIN IMMEDIATE**，避免嵌套事务。

如果表重建需要调整 FK enforcement，必须在真实 migration test 中先验证 SQLite 行为：事务内不得切换 `PRAGMA foreign_keys=OFF`。若最终证明必须关闭 foreign_keys，只允许在创建 migration transaction 之前于 connection level 设置，并在迁移结束后恢复 `ON`；无论采用哪条路径，提交前/后都必须以 `PRAGMA foreign_key_check` 作为验收 Gate。

如果现有 `apply_migration(sql)` 无法安全完成父表 rebuild，允许在 `migrate()` 中为 v12 增加一个专用 Rust migration function；该函数仍必须使用同一个外层 SQLite transaction，不建立第二套 Store。

`migrate()` 的版本门禁从当前 `1..=11` 升级为接受 `1..=12`，并在 `version < 12` 时按既有顺序应用历史 migration 后追加 v12；大于当前支持版本仍保持 fail-closed。`schema_v9.sql`、`schema_v10.sql`、`schema_v11.sql` 不改写。

必须保留并重新验证至少：

- `prevent_execution_runtime_rebind`；
- `executions_runtime_state`；
- `executions_one_unresolved_per_agent`；
- `workspace_claims` FK；
- Work execution links；
- v10 Runtime platform/containment 字段与 `runtime_instances_v10_validate_insert` / `runtime_instances_v10_validate_update` 触发器；
- v11 `command_runs`、`work_command_links` 及其索引、外键；
- Activity tables；
- public Usage；
- Codex private Usage；
- runtime attempt / quarantine tables；
- 其他引用 `executions` / `runtime_instances` 的现有 FK。

## 12.6 历史 Evidence Migration Rule

迁移只能做字段重命名 / 原值复制 / 已有类型事实转换。

允许：

~~~text
旧 Runtime provider = codex
~~~

禁止：

- 根据当前 Provider 配置回填历史 provider；
- 根据当前进程状态生成 termination evidence；
- 给旧 Runtime 补当前 Session ID；
- 给缺失字段补当前安全默认值并据此释放 Claim；
- 根据 thread/session 文本推断新 Provider identity；
- 重算历史 request hash。

迁移后的历史 Runtime / Execution Evidence 强度不得高于迁移前。

## 12.7 Migration Gate

schema_v12 单独通过：

~~~text
v11 → v12 real fixture migration (primary)
v9 → v10 → v11 → v12 historical fixture migration (transitive compatibility)
empty DB → latest schema
historical Codex rows byte/semantic preserved
all pre-v12 executions task_role = general
historical request_hash values remain byte-identical
v10 platform/containment fields and validation triggers preserved
v11 command_runs/work_command_links rows, indexes and FKs preserved
new execution can be created immediately after CB-001A
provider=codebuddy can persist
runtime provider mismatch rejected
all FK/index/trigger present
foreign_key_check clean
migration failure rolls back schema + user_version
Codex Runtime / Recovery / Claim tests pass
~~~

该 Gate 通过前不开始 CodeBuddy Runtime 产品实现。

---

# 13. CodeBuddy Provider 定位

新增建议：

~~~text
src-tauri/src/agent/codebuddy/
├── mod.rs
├── provider.rs
├── protocol.rs
├── client.rs
├── runtime.rs
├── windows_launcher.rs
├── recovery.rs
├── store.rs
└── tests/
~~~

如果现有 Win32 Job-at-creation launcher 可以在不改变 Codex 行为的前提下抽取公共低层组件，可以复用：

~~~text
CreateJobObjectW
PROC_THREAD_ATTRIBUTE_JOB_LIST
HANDLE_LIST
CreateProcessW
QueryInformationJobObject
TerminateJobObject
~~~

但抽取公共 launcher 不是 CodeBuddy 上线前置条件。

如果重构会扩大 Codex Runtime blast radius，首版允许 CodeBuddy 使用独立薄封装并复用同一组已验证不变量。

---

# 14. CodeBuddy ACP Contract

截至 2026-09-21，CodeBuddy 官方支持：

~~~text
codebuddy --acp
~~~

默认 transport：

~~~text
stdio NDJSON
~~~

官方也支持 --acp-transport streamable-http，以及 --serve 下的 HTTP ACP，但首版不用 HTTP。

ACP 当前稳定协议版本为 v1；协议版本必须通过 initialize.protocolVersion 协商，不能通过 SDK 版本猜测。

## 14.1 启动命令

首版目标命令：

~~~text
<absolute-codebuddy-path>
    --acp
    --permission-mode auto
~~~

具体参数在 Contract Probe 后冻结。

禁止经过 cmd.exe、PowerShell 或 shell command string。

Windows 使用绝对 executable path + argv quoting。

## 14.2 CodeBuddy 安装边界

首版 SerenaDesktop：

- 检测 CodeBuddy；
- 读取版本；
- 记录实际 executable path；
- 可计算 binary SHA-256；
- 不负责安装；
- 不负责升级；
- 不负责登录；
- 不存储 CodeBuddy 账号凭据。

发现顺序建议：

~~~text
1. 本地 Provider 配置的显式 executable path（若后续 UI 提供）
2. where.exe codebuddy
3. unavailable
~~~

第一版可以只实现 `where.exe codebuddy`，自定义 path 作为后续便利功能。

### 14.2.1 Version Contract

CB-005 必须冻结首个受支持的 CodeBuddy 版本契约：

~~~text
detected version
binary SHA-256
ACP negotiated protocolVersion
required capability set
Contract Probe evidence
~~~

运行时 Admission Health 只把**已进入 SerenaDesktop supported-version table** 的版本视为 available。未知版本默认 unavailable，不因为它自称 ACP v1 就自动放行。

binary SHA-256 作为 Probe / Release evidence 保存，用于识别实际测试对象；首版不把“相同 version 但 hash 不同”简单等价为可信。若版本命中但 binary hash 与已验证样本不同，execute 时必须重新通过真实 ACP initialize / capability Contract Gate；失败后标记 Provider unavailable。

首版不增加“忽略版本检查”或“强制放行未知版本”的用户设置。需要支持新版本时，通过新的 Contract Probe 更新 supported-version table。

supported-version table 属于 SerenaDesktop release-owned compatibility data，随应用发布物更新，用户不能在本地把未知版本强制标记为 supported。

Product/UI 对未知版本固定使用“未验证”语义，而不是泛化成“CodeBuddy 损坏”：

~~~text
CodeBuddy 版本 <detectedVersion> 尚未经过当前 SerenaDesktop 的兼容性验证。

当前未启用该版本的 Agent 执行。
请使用受支持版本，或升级 SerenaDesktop 后重新检测。
~~~

底层仍使用稳定诊断 `CODEBUDDY_VERSION_UNSUPPORTED` / `AGENT_PROVIDER_UNAVAILABLE`，UI 文案不得通过解析错误文本判断状态。

## 14.3 ACP Rust SDK

优先使用官方 Rust ACP SDK：

~~~text
agent-client-protocol
~~~

但不能让 SDK 自己无约束 spawn CodeBuddy。

正确关系：

~~~text
SerenaDesktop Windows Runtime
    ↓
CreateProcessW + Job-at-creation
    ↓
获得 stdin / stdout
    ↓
ACP ByteStreams / Lines / 自定义 Transport Adapter
    ↓
ACP Client
~~~

Protocol 和 Process Ownership 必须分离。

CB-005 必须验证官方 Rust SDK 能直接消费 SerenaDesktop 已通过 Win32 Job-at-creation 创建出的 stdin/stdout pipe，而不是要求 SDK 自己 spawn Agent。

首选路径：

~~~text
managed Win32 pipes
    ↓
ACP ByteStreams / Lines
    ↓
official ACP Client
~~~

如果固定 SDK 版本无法支持外部受管 I/O，允许实现一个**仅覆盖首版冻结方法集**的最小 NDJSON ACP client，范围只包含：

~~~text
initialize
session/new
session/prompt
session/update
session/cancel
session/request_permission
以及经 Probe 证明需要的 session recovery method
~~~

该 fallback 不演化为通用 ACP SDK，不支持未冻结 extension。

---

# 15. CodeBuddy Runtime 与 Session

首版每个活动 Execution 使用一个 SerenaDesktop-owned CodeBuddy Runtime。

不共享一个可以动态切 Workspace 的 CodeBuddy 进程。

## 15.1 Fresh Start

~~~text
Execution E1 created
    ↓
Create CodeBuddy Runtime R1
    ↓
Job-at-creation
    ↓
ACP initialize
    ↓
protocol / capability verification
    ↓
session/new(
    cwd = E1.canonical_workspace_root
)
    ↓
persist exact session identity
    ↓
ProviderAcceptanceSink.accepted()
    ↓
persist dispatching
    ↓
session/prompt
    ↓
session/update...
    ↓
prompt terminal response
    ↓
persist result / provider terminal
    ↓
Terminate Runtime Job
    ↓
ActiveProcesses == 0
    ↓
runtime termination evidence
    ↓
finalize_and_release_execution
~~~

首版刻意在每个 Execution terminal 后停止整个 CodeBuddy Runtime。

原因：

- 不依赖 CodeBuddy 私有 background terminal cleanup；
- Runtime Job-level termination 可以证明该受管进程树不再继续写 Workspace；
- Provider 正常路径和 Crash Recovery 使用同一强证据；
- 便于保持与现有 Claim safety 对齐。

## 15.2 Session Identity

CodeBuddy provider-private State 至少持久化：

~~~text
execution_id
runtime_instance_id
acp_protocol_version
session_id
conversation_request_id?
prompt_rpc_id?
prompt_state
terminal_stop_reason?
~~~

session_id 等 Provider private identity 不进入：

- public Provider Port；
- Work；
- generic Product routing；
- Claim release authorization。

## 15.3 conversationRequestId

CodeBuddy 当前支持在 session/prompt._meta 中携带：

~~~text
codebuddy.ai/conversationRequestId
~~~

该字段可帮助把一个 SerenaDesktop Execution 与一个 CodeBuddy Prompt 精确关联。

是否采用由 Contract Probe 决定。

如果采用：

- SerenaDesktop 生成；
- Provider-private 持久化；
- 必须在 prompt send 前 durable；
- 不由 ChatGPT 提供；
- 不作为跨 Provider 公共身份。

---

# 16. Continue Contract

CodeBuddy Continue 是否开放，必须经过真实 Contract Gate。

目标流程：

~~~text
Parent E1
provider = codebuddy
session = S1
terminal

    ↓ Continue

Child E2
provider = codebuddy

    ↓

Create Runtime R2
    ↓
ACP initialize
    ↓
ACP session recovery method(S1)
    ↓
验证 exact session identity / cwd
    ↓
ProviderAcceptanceSink.accepted()
    ↓
session/prompt
~~~

必须证明：

1. R1 结束后 S1 被可靠持久化；
2. 独立 R2 能恢复同一个 S1；
3. 恢复后的 Workspace / cwd 与冻结 Workspace 一致；
4. 历史中可以确认 Parent Execution 的 Session lineage；
5. 不需要重放 Parent Prompt；
6. Continue 不依赖旧 Runtime 继续存活。

上图中的 `ACP session recovery method` 必须以 CB-005 Probe 冻结的真实方法名、参数和返回字段为准。候选可能表现为 `session/load` 或 `session/resume`，但生产代码禁止同时实现两条猜测路径或在失败后自动 fallback 到另一方法。

若当前 CodeBuddy 固定版本无法证明这些条件：

~~~text
canContinue = false
~~~

首版仍可正常支持 fresh Start。

不能为了“看起来支持 Continue”而：

- 拼接旧 Prompt；
- 复制聊天文本模拟 Session；
- 让 old terminal Execution 重新 running；
- 自动换到 Codex。

---

# 17. ACP Client Capability 边界

首版 SerenaDesktop ACP Client 不向 CodeBuddy 宣告：

~~~text
fs.readTextFile
fs.writeTextFile
terminal
~~~

也就是说：

> CodeBuddy 使用自己的本地工具访问 Workspace，不把文件或终端操作代理给 SerenaDesktop ACP Client。

这样可以避免第一版同时承担：

- ACP Client 文件系统权限系统；
- ACP Client Terminal ownership；
- 第二套 Source Write；
- 第二套 command process tree。

CodeBuddy Process 本身仍受到 SerenaDesktop Runtime Job ownership 约束。

---

# 18. Permission Contract

ACP v1 的 Client baseline method 包含：

~~~text
session/request_permission
~~~

CodeBuddy 自身也有独立 Permission Mode。

首版启动建议：

~~~text
--permission-mode auto
~~~

理由：

- 比 bypassPermissions 更适合作为普通桌面环境默认值；
- 尽量减少人工审批；
- 仍保留 CodeBuddy 自身安全判定。

## 18.1 未解决 Permission 的默认行为

SerenaDesktop 首版不建立远程实时权限审批工作流。

如果 CodeBuddy 仍通过 ACP 发出 session/request_permission：

1. 校验 exact Runtime / Session / Execution identity；
2. 不把 command、prompt、源码内容直接暴露到 Remote MCP；
3. 按 ACP Contract 返回拒绝 / cancelled 的 fail-closed 结果；
4. 通过安全 Activity 投影记录“Provider 权限未获批准”；
5. 允许 CodeBuddy 自己调整路径或最终失败。

具体 PermissionOption 到拒绝结果的映射，在 Contract Probe 中针对固定 CodeBuddy 版本冻结。

Permission deny 只是 Client 权限决策，不是 Provider terminal evidence。CodeBuddy 可能在权限请求之前已经产生文件或命令副作用，因此收敛规则固定为：

~~~text
permission denied
+
收到 exact Provider terminal
    → 按真实 terminal 保存
    → Runtime termination
    → normal finalization

permission denied
+
没有可靠 Provider terminal
    → terminate Runtime Job
    → obtain Job-level evidence
    → reconciling / interrupted
~~~

禁止把 `permission denied` 本身直接映射成 `cancelled`、`failed` 或 `completed` 并释放 Claim。

首版不自动选择“永久允许”。

## 18.2 不使用 bypassPermissions 作为默认

SerenaDesktop 不默认：

~~~text
-y
--dangerously-skip-permissions
CODEBUDDY_IS_SANDBOX=1
~~~

这些能力只适合隔离沙箱，不适合作为普通用户桌面默认安全模型。

如果后续真实使用证明 auto 打断过多，再单独设计 CodeBuddy Provider 权限设置。

---

# 19. ACP Activity Mapping

CodeBuddy 的 session/update 只在 Adapter 内解释。

目标：

~~~text
ACP session/update
      ↓
validate runtime/session/request identity
      ↓
CodeBuddy Adapter
      ↓
AgentTelemetryEvent::Activity
      ↓
TelemetryProjector
~~~

公共层不得直接理解 ACP event 类型。

## 19.1 Tool Mapping

只映射安全分类：

~~~text
文件读取类 → Read
文件修改类 → Edit
命令执行类 → Command
测试明确可识别 → Test
构建明确可识别 → Build
其他 → Tool
~~~

禁止通过 command string、stdout、agent message 文本猜测安全或业务事实。

无法可靠识别时使用 ToolCategory::Tool。

## 19.2 Message 内容

普通 agent_message_chunk 不作为 Activity summary 原文暴露。

它可以用于 Provider private result assembly 和 terminal Final Result，但 Activity 公共投影继续保持安全分类。

---

# 20. Usage Contract

CodeBuddy ACP 存在 usage_update 能力，但首版不能因为字段名存在就直接映射成 SerenaDesktop Usage。

必须通过固定版本 Contract Test 确认：

- token 字段含义；
- cumulative 还是 per-turn；
- Session restart 后是否 reset；
- Continue 后是否累计；
- terminal 前后事件顺序；
- context window 字段语义；
- late usage 行为；
- exact Session / Prompt identity。

只有验证完成后：

~~~text
ProviderCapabilities.token_usage = true
~~~

否则：

~~~text
token_usage = false
~~~

且 Product Usage 返回 `unknown`，不伪造 0。

## 20.1 Provider-private Usage Gate

现有 `codex_execution_usage_state`、Codex Thread epoch、baseline、terminal grace 等全部继续属于 Codex Provider private contract。

冻结规则：

~~~text
Execution.provider = codex
    → 允许进入 Codex private Usage path

Execution.provider != codex
    → 禁止创建 / 读取 / 更新 codex_execution_usage_state
~~~

CodeBuddy 在 Usage Contract Gate 通过前：

- 不调用 `prepare_codex_usage_baseline`；
- 不调用 `enter_codex_usage_terminal_grace`；
- 不调用 `freeze_codex_usage`；
- 不把 CodeBuddy UsageEvent 写成 `provider_id='codex'`；
- Product observe/list/detail 稳定返回公共 Usage `unknown`，不得因为缺少 Codex private state 而失败。

未来 CodeBuddy Usage 上线时，通过 Provider-owned Usage projector 写公共 `execution_usage.provider_id=codebuddy`；仍不得复用 Codex Thread epoch 语义。

---

# 21. Provider Terminal 与 Result

ACP session/prompt 的 terminal response 和 stop reason 是 CodeBuddy Provider Terminal 的主要来源。

Adapter 必须验证：

~~~text
runtime identity
session identity
target prompt identity
~~~

再写 Provider terminal evidence。

ProviderRunResult 仍只能返回：

~~~text
executionId
outcome
result
resultCompleteness
diagnosticCode
~~~

不能返回：

~~~text
safeToReleaseWorkspace
jobEmpty
releaseEvidence
runtimeTerminated
~~~

Safe Release 仍由 StateStore / Runtime Evidence 决定。

Claim Release 的授权条件必须保持 Provider-neutral：

~~~text
persisted release evidence kind/state
        ↓
finalize_and_release_execution()
~~~

允许 Codex 形成 `same_runtime_cleanup`，允许 CodeBuddy 主要形成 `runtime_terminated`；但 StateStore 最终判断不得写成：

~~~text
if provider == "codebuddy" { release }
~~~

Provider ID 只用于验证 Evidence ownership 与 Runtime identity，永远不是 ReleaseBasis 本身。

## 21.1 Normal Success

首版正常终态建议：

~~~text
ACP terminal
    ↓
Result complete / partial
    ↓
terminate CodeBuddy Job
    ↓
ActiveProcesses == 0
    ↓
Runtime termination evidence complete
    ↓
atomic terminal + Claim release
~~~

因此 CodeBuddy 不需要复制 Codex 的 background terminal cleanup 机制。

---

# 22. Cancellation

流程：

~~~text
persist user cancel intent
    ↓
provider.cancel()
    ↓
session/cancel
    ↓
等待 bounded terminal response
~~~

如果 Provider 正常返回取消终态：

~~~text
persist provider terminal
    ↓
terminate Runtime Job
    ↓
Job empty
    ↓
finalize cancelled
~~~

如果 session/cancel 后没有可靠 terminal：

~~~text
bounded timeout
    ↓
terminate Runtime Job
    ↓
Job empty
    ↓
reconciliation
~~~

Runtime termination 可以证明旧 CodeBuddy Runtime 不再继续写 Workspace，但不能伪造一个从未收到的 ACP terminal。

这种情况沿用现有 Runtime termination recovery 语义收敛，必要时进入 interrupted，而不是仅因为用户曾点击 Cancel 就伪造 Provider 已确认 Cancelled。

---

# 23. Crash Recovery

目标安全等级与 Codex 一致：

> Host Crash 后只有取得可靠 Runtime / Job-level evidence，才允许认为旧 Provider 不再继续修改 Workspace。

## 23.1 Windows Runtime

CodeBuddy Runtime 必须保持：

~~~text
CreateProcessW success
    ⇒ process already belongs to Job
~~~

继续使用：

~~~text
PROC_THREAD_ATTRIBUTE_JOB_LIST
JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
no breakaway
non-inheritable Job Handle
explicit stdio HANDLE_LIST
~~~

不得使用 spawn 后再 AssignProcessToJobObject 的两阶段模式。

## 23.2 Startup Recovery

SerenaDesktop 重启：

~~~text
ProviderRegistry
    ↓
get_registered(codebuddy)
    ↓
startup_reconcile
    ↓
CodeBuddy private State
    ↓
Runtime Job evidence
~~~

只有 job_active_processes_zero 或 managed_job_destroyed 等经过原 Session namespace / Policy 验证的证据，才能证明旧 Runtime 已停止。

PID 不足。

## 23.3 Result Recovery

如果 Runtime R1 已安全终止，但 Execution terminal result 尚未持久化：

- R2 可以通过 CodeBuddy Session recovery 尝试恢复结果；
- 恢复必须绑定 exact Session / Prompt identity；
- 恢复结果不能替代 R1 Runtime termination evidence。

如果 CodeBuddy 历史 API 无法精确恢复 target Prompt：

~~~text
resultCompleteness = unknown / partial
~~~

但在已取得 R1 Runtime termination evidence 后，可以按照既有 Runtime termination recovery 收敛为 interrupted 并安全释放 Claim。

这允许第一版在“结果无法完整恢复”时仍保持 Workspace Safety，而不会让整个 SerenaDesktop 不可用。

## 23.4 无法取得 Job Evidence

如果旧 Runtime 的 Job ownership / Session namespace 无法证明：

~~~text
Execution = unknown
Workspace Claim = retained
~~~

不得因为 CodeBuddy Session 能重新打开就释放旧 Claim。

---

# 24. Multitask 与 Agent Teams

CodeBuddy 当前 ACP 已支持 Multitask 和 Agent Teams 扩展，但第一版固定不启用。

要求：

- 不传 --multitask；
- 不调用 session/set_config_option(multitask=true)；
- 如果 configOptions 出现 multitask，保持 false；
- 未知 CodeBuddy _meta 扩展忽略；
- Agent Teams 事件不进入 SerenaDesktop Multi-Agent Provider 模型。

CodeBuddy 内部 Multitask 与 SerenaDesktop 的“多个 Provider / 多个 Execution / Work 编排”属于不同层次。

首版多 Agent 是：

~~~text
ChatGPT
    ↓
SerenaDesktop
    ↓
多个独立 Provider Execution
~~~

不是 CodeBuddy 内部自动生成多个 worker。

---

# 25. Agent 管理 UI

左侧导航：

~~~text
Agent
~~~

改为：

~~~text
Agent 管理
~~~

页面保持当前 Serena Desktop Design System，不新增独立视觉语言。

建议页面结构：

~~~text
Agent 管理

Agent 接入
────────────────────────

[ Codex ]
已启用 · 可用
版本
协议：Codex App Server
Runtime：运行中 / 已停止
角色：开发、分析
[启用开关] [重新检测]

[ CodeBuddy ]
已启用 · 可用
版本
协议：ACP
Transport：stdio
Runtime：运行中 / 已停止
角色：测试、评审
[启用开关] [重新检测]

角色分工
────────────────────────

开发      [ Codex ▼ ]
测试      [ CodeBuddy ▼ ]
评审      [ CodeBuddy ▼ ]
分析      [ Codex ▼ ]
通用      [ Codex ▼ ]

任务
────────────────────────

现有 Agent Composer
最近任务
历史任务
~~~

## 25.1 Provider Card 状态

必须分开显示：

~~~text
enabled
health
runtime
activeExecutions
~~~

例如：

~~~text
CodeBuddy

接入：已启用
可用性：可用
Runtime：已停止
活动任务：0
~~~

这是正常 Idle 状态。

## 25.2 Draining

如果 Provider 有运行任务时用户关闭：

~~~text
CodeBuddy

接入：正在停用
活动任务：1
Runtime：运行中
~~~

最后一个 Execution 收敛后：

~~~text
接入：已停用
Runtime：已停止
~~~

## 25.3 Role UI

Role 下拉允许选择已注册 Provider，包括当前 disabled Provider。

如果配置 `testing → codebuddy`，但 CodeBuddy disabled，UI 保留该绑定并提示：

~~~text
测试 → CodeBuddy · 已停用
~~~

不要自动改成 Codex。

Role 允许清空；未配置时显示：

~~~text
测试 → 未指定 Agent
~~~

Remote Start 对该 Role 返回 `AGENT_ROLE_NOT_CONFIGURED`，本地 UI 提供明确的“设置 Agent”入口，不自动选择默认 Provider。

如果 Provider 被停用且存在 pending/resumable Execution 持有 Workspace Claim，Provider 卡片必须额外显示阻塞提示，并提供：

~~~text
查看任务
取消任务
重新启用 Provider
~~~

不能提供 Force Unlock。

---

# 26. Local IPC

建议新增或扩展本地管理命令：

~~~text
agent_provider_settings_get

agent_provider_set_enabled(
    providerId,
    enabled
)

agent_provider_set_role_route(
    taskRole,
    providerId?
)

agent_provider_refresh_health(
    providerId
)
~~~

这些命令：

- 只在本地 Tauri UI 暴露；
- 不通过 Remote MCP 修改；
- 复用现有 Supervisor configuration management lock；
- 复用 config atomic persist；
- 不启动 Agent Runtime，除非明确的 health probe 需要短生命周期检测；
- Role Routing 更新不影响 running Execution。

Remote MCP 只新增只读 agent_query(action=providers)。

---

# 27. Product Projection

现有 ProviderProduct 不能继续通过字符串 match 只认识 codex。

必须改为从：

~~~text
ProviderRegistry descriptor
+
persisted Execution.provider
~~~

投影。

目标：

~~~text
Execution.provider = codebuddy
    ↓
ProviderRegistry.get_registered(codebuddy)
    ↓
descriptor
    ↓
Product provider
~~~

历史 Execution 如果 Provider 当前未注册，不得伪造为 Codex。

公共投影固定为：

~~~text
persisted provider id
+
registered descriptor when available
+
safe historical fallback when unavailable
~~~

如果 persisted Provider 当前未注册或 descriptor 无法读取：

~~~text
id          = persisted provider id
displayName = persisted provider id
version     = null
~~~

Execution Product 本身不新增第二套 Provider Health 字段；当前 Provider 是否已注册、enabled、available 由 `agent_query providers` 的 Provider Catalog 表达。

Product query/observe/list 仍必须成功返回 Execution snapshot，不能因为 ProviderProduct 不认识该字符串而使整个任务详情失败。

Execution Product View 同时返回冻结的 taskRole；它描述创建时的 routing identity。当前 Role Policy 仅由 agent_query providers 返回，两者不互相覆盖。

Usage 投影也必须 Provider-neutral：非 Codex Execution 在 CodeBuddy Usage 未实现时返回 unknown/null，禁止进入 Codex private Usage state。

Product 层不得出现新的 provider == "codebuddy" 业务分支；Provider-specific display metadata 来自 Registry descriptor，Provider-specific Usage/Runtime 逻辑留在 Adapter。

---

# 28. CodeBuddy Contract Probe

在写 Provider 正常执行路径前，先建立：

~~~text
CB-ACP-000 Contract Probe
~~~

Probe 证据必须进入仓库，例如：

~~~text
docs/tasks/evidence/CB-005/codebuddy-<version>/
    verification.md
    initialize.jsonl
    fresh-session.jsonl
    continuation.jsonl
    cancellation.jsonl
    permission.jsonl
    usage.jsonl
    crash-recovery.md
    binary.sha256
~~~

证据文件必须脱敏，不保存 Prompt、凭据、源码正文或完整环境。

记录：

~~~text
CodeBuddy CLI version
absolute executable path
binary SHA-256
ACP SDK version
ACP initialize request / response
negotiated protocolVersion
agentCapabilities
auth behavior
session/new
session/prompt
session/update
session/cancel
session recovery method（由 Probe 钉死真实方法名，不预设 load/resume 二选一）
session/request_permission
usage_update
process exit behavior
session persistence behavior
~~~

至少覆盖：

## 28.1 Fresh Session

~~~text
initialize
session/new(cwd)
session/prompt
terminal
~~~

验证 Session ID、cwd、update identity、prompt terminal、result 和 exit。

## 28.2 Continue Across Runtime

~~~text
R1
session/new S1
prompt P1
terminal
stop R1

R2
initialize
load/resume S1
prompt P2
terminal
~~~

验证 exact S1、history retained、cwd、P2 不重放 P1。

Contract Probe 必须把真实恢复方法名、参数、返回字段和 cwd 恢复/校验语义冻结下来。设计文档中的 session/load / session/resume 只是候选表达，生产代码不得同时猜测两条路径。

## 28.3 Cancel

~~~text
prompt running
session/cancel
~~~

验证 terminal 是否必达、stop reason、update 顺序、permission pending 如何收敛。

## 28.4 Crash

在不同窗口强杀 Host / Runtime：

~~~text
before prompt send
during prompt send
after side effect
before prompt terminal
after terminal before persist
~~~

验证 Job-level cleanup 和 Session recovery。

## 28.5 Permission

验证：

- --permission-mode auto；
- ACP permission request wire；
- deny / cancelled option；
- non-interactive behavior；
- 拒绝后 Provider 是否继续或终止。

## 28.6 Usage

验证：

- fresh；
- multi-turn；
- restart；
- resume；
- terminal ordering；
- cumulative reset。

Contract Probe 没证明的能力必须保持 unsupported 或 unknown，不能按文档字段名推断。

---

# 29. Stable Errors

在现有 Provider error 外增加 CodeBuddy Adapter 私有诊断码，Product 仍映射为稳定公共错误。

候选私有码：

~~~text
CODEBUDDY_BINARY_NOT_FOUND
CODEBUDDY_VERSION_UNSUPPORTED
CODEBUDDY_ACP_INIT_FAILED
CODEBUDDY_ACP_INCOMPATIBLE
CODEBUDDY_ACP_STDIO_EOF
CODEBUDDY_ACP_INVALID_MESSAGE
CODEBUDDY_AUTH_REQUIRED
CODEBUDDY_SESSION_CREATE_FAILED
CODEBUDDY_SESSION_RECOVERY_FAILED
CODEBUDDY_PERMISSION_DENIED
CODEBUDDY_PROMPT_FAILED
CODEBUDDY_RUNTIME_TERMINATION_TIMEOUT
CODEBUDDY_RUNTIME_EVIDENCE_INCOMPLETE
~~~

公共 Provider Port 继续只暴露现有五类：

~~~text
AGENT_PROVIDER_NOT_FOUND
AGENT_PROVIDER_UNAVAILABLE
AGENT_PROVIDER_CAPABILITY_UNSUPPORTED
AGENT_PROVIDER_CONTRACT_ERROR
AGENT_PROVIDER_OPERATION_FAILED
~~~

CodeBuddy 私有 code 可进入安全 diagnosticCode / log，但不能成为通用 Control Plane 的 Provider 特判来源。

---

# 30. 日志与隐私

Agent Provider 日志允许记录：

~~~text
providerId
executionId
runtimeInstanceId
ACP method name
request correlation id
duration
success/failure
safe diagnostic code
CodeBuddy version
negotiated ACP protocol version
~~~

公共 Agent / MCP / Product 日志不记录：

~~~text
prompt
source content
full result
Authorization
API key
credential
environment
raw command
stdout
stderr
permission request sensitive payload
~~~

Provider-private Runtime diagnostics 可以保存有界、脱敏的 stderr tail，仅用于本地诊断和测试证据；它不得进入：

- ChatGPT Product Result；
- MCP tool output；
- Activity summary；
- Remote logs；
- Provider public error payload。

因此“公共日志不记录 stderr”和“Provider-private bounded stderr diagnostic”属于不同暴露面，不冲突。

---

# 31. 安全不变量

CodeBuddy Provider 上线后必须继续满足：

~~~text
1. Execution 从创建后只属于一个 Provider。

2. Continue 不改变 Provider。

3. Role Routing 变化不迁移已有 Execution。

4. Provider disabled 不取消已有 Execution。

5. Provider disabled 不阻止 Cancel / Startup Reconcile。

6. Provider unavailable 不触发自动 fallback。

7. Provider Runtime 从进程创建的第一个可运行时刻进入受管 Job。

8. Main PID disappearance 不是 Runtime termination evidence。

9. Runtime termination evidence 属于产生它的 Provider Runtime。

10. ACP Session recovery 不能替代旧 Runtime termination evidence。

11. Provider terminal 不能直接授权 Workspace Claim release。

12. terminal + Claim release 仍原子提交。

13. unknown side effects 不 replay。

14. 无可靠 Runtime evidence 时保持 unknown + Claim。

15. Remote MCP 不能修改 Provider enabled / Role Routing。

16. Agent Role 是 Routing Policy，不是安全 Capability。

17. CodeBuddy ACP 不获得 Workspace Authority；cwd 来自冻结 Execution Workspace。

18. ACP 客户端文件 / terminal 代理首版不启用。

19. Workspace Claim release 只依赖 persisted evidence；Provider ID 永远不能单独授权 Release。
~~~

---

# 32. 实施顺序

建议按以下顺序拆分，每个阶段独立 Review。

## CB-001A — Multi-Provider Persistence Migration

内容：

- schema_v12；
- Execution provider CHECK removal；
- execution Provider identity 与 ProviderId 收敛；
- Store insert 写真实 provider；
- Runtime provider identity；
- provider-neutral process identity header；
- task_role + `AgentTaskRole::General` 兼容默认；
- `insert_execution()` 从 CB-001A 起显式写 task_role；
- CB-001A 期间 request canonicalization 仍保持 v2；
- FK / Index / Trigger rebuild。

Gate：

~~~text
v11 → v12 fixture PASS
v9 → v10 → v11 → v12 transitive fixture PASS
historical Codex rows preserved
historical Runtime evidence preserved
all pre-v12 executions task_role=general
v10 containment triggers and v11 CommandRun/WorkCommandLinks preserved
historical request_hash byte-identical after migration
new execution creation works before CB-001B
CodeBuddy provider value can persist
Runtime provider mismatch rejected
foreign_key_check clean
migration rollback verified
Codex Runtime / Recovery / Claim tests all pass
~~~

## CB-001B — Request Identity v3 / Legacy Compatibility

内容：

- execution-request-v3；
- task_role 进入 canonical request；
- provider 继续沿用 v2 identity；
- v2 / v1 historical retry compatibility；
- historical hash 不重写。

Gate：

~~~text
fresh v3 deterministic
provider change conflicts
taskRole change conflicts
historical v2 general retry succeeds
historical generation/C2 compatibility remains bounded
no migration rewrites request_hash
~~~

## CB-001C — Product / Usage Provider Neutralization

内容：

- ProviderProduct 从 Registry descriptor 投影；
- historical/unregistered Provider safe fallback；
- ExecutionView 输出 frozen taskRole；
- CodeBuddy Execution 不进入 Codex private Usage；
- public Usage unknown/null fallback；
- fake second Provider 端到端 query/observe/list fixture。

Gate：

~~~text
provider=codebuddy Product snapshot succeeds
unknown historical provider does not become Codex
non-codex usage does not create codex private state
Codex Usage regressions pass
no provider-specific branch in generic Product
~~~

## CB-002 — Agent Provider Local Policy

内容：

- ManagerConfig agentProviders；
- per-provider enabled；
- roleRouting；
- migration default；
- Provider admission gate；
- disabled / draining semantics；
- Local IPC；
- 前端 ManagerConfig 类型与初始配置同步更新，包括 `src/types.ts`、`src/app/useAppController.ts`、`src/api.ts` 及相关测试 fixture。

Gate：

~~~text
toggle does not spawn runtime
disable does not cancel running task
cancel/reconcile still works when disabled
restart preserves provider policy
role mapping persists
~~~

## CB-003 — Provider Discovery / Routing Contract

内容：

- agent_query providers；
- agent_execute start.taskRole；
- agent_execute start.providerId；
- Role Policy validation；
- no fallback；
- Continue inheritance；
- Product/MCP Schema。

Gate：

~~~text
development → codex
testing → codebuddy
explicit taskRole + providerId route correctly
legacy Start with both fields absent → general routing
legacy Start with general route absent → AGENT_ROLE_NOT_CONFIGURED
only taskRole → INVALID_PARAMS
only providerId → INVALID_PARAMS
role/provider mismatch → AGENT_ROLE_PROVIDER_MISMATCH
disabled rejected
unavailable rejected
routing error can recover by agent_query(providers) snapshot
continue inherits frozen provider/role
no provider reroute
advertised capabilities equal implemented capability ∩ passed Evidence Gate
unproven CodeBuddy continue/recover/usage capabilities remain false
~~~

## CB-004 — Agent 管理 UI

内容：

- 菜单改为“Agent 管理”；
- Provider cards；
- enable toggle；
- health / version / protocol / runtime；
- Role Routing；
- draining；
- unsupported-version user-facing message；
- ManagerConfig 前端类型 / controller / API / fixture 同步；
- existing task composer / task list integration。

Gate：

~~~text
UI displays registered providers dynamically
role mapping changes persist
role route can be cleared and shows “未指定 Agent”
disabled provider binding remains visible as “已停用” and is not auto-rebound
running task survives provider disable
pending/resumable Claim shows blocked-workspace warning
pending warning provides 查看任务 / 取消任务 / 重新启用 Provider
no Force Unlock action exists
unsupported CodeBuddy version is explained as “尚未验证” rather than generic failure
existing task UI has no regression
~~~

## CB-005 — CodeBuddy ACP Contract Probe

内容：

- 固定测试版本；
- binary hash；
- supported-version table entry；
- ACP v1；
- stdio NDJSON；
- official Rust SDK external-managed-I/O compatibility；
- 如果 SDK 不兼容，冻结最小 NDJSON fallback method set；
- fresh / continue / cancel / permission / usage / crash evidence；
- 真实 Session recovery method name / params；
- 脱敏 evidence files 落入 docs/tasks/evidence/CB-005/。

Gate：

~~~text
required ACP wire contract recorded
exact version/hash evidence recorded
managed Win32 pipes → ACP transport proven
session recovery method frozen
unsupported features explicit
no guessed fields
evidence files reproducible and sanitized
~~~

## CB-006 — CodeBuddy Runtime Foundation

内容：

- CodeBuddy Job-at-creation；
- pipes；
- ACP managed transport；
- initialize；
- Job termination；
- startup reconcile；
- Runtime evidence。

Gate：

~~~text
host crash kills CodeBuddy process tree
no Job escape window
stdio ACP works
runtime identity persisted
job-level recovery works
unknown remains fail-closed
canRecover remains false before this Gate
canRecover may become true only after startup_reconcile + Job Recovery Gate PASS
~~~

## CB-007 — CodeBuddy Provider Vertical Slice

内容：

~~~text
Start
ACP session/new
Prompt
Activity
Terminal
Result
Runtime termination
Atomic release
~~~

Gate：

~~~text
real CodeBuddy modifies isolated test workspace
result persists
claim releases only after evidence
Codex path unaffected
canExecute/canCancel/activity reflect only implemented + passed evidence
canContinue remains false until CB-008 continuation evidence passes
tokenUsage remains false until CB-009 Usage Gate passes
~~~

## CB-008 — Continue / Cancel / Recovery

内容：

- Probe-frozen ACP session recovery method；
- Continue；
- cancel；
- crash recovery；
- result recovery；
- permission fail-closed。

Gate：

~~~text
cross-runtime continuation proven or explicitly recorded unsupported
canContinue becomes true only when continuation implementation + evidence both PASS
cancel race covered
old runtime termination precedes safe release
missing result can converge interrupted when runtime safety is proven
unsupported continuation does not block fresh Start / Cancel / Runtime Safety
~~~

## CB-009 — Activity / Usage

内容：

- ACP update → Activity；
- optional CodeBuddy Usage；
- Product projection；
- UI。

Usage Contract 未通过时，本任务只上线 Activity，不阻塞 Provider 基础可用性。

`tokenUsage` 只有在 Usage Contract 与 CodeBuddy Usage projector 都通过后才能 advertise 为 true；否则保持 false，公共 Usage 稳定为 unknown/null。

## CB-010 — Manual E2E

真实路径：

~~~text
Local:
    enable Codex
    enable CodeBuddy
    development → Codex
    testing → CodeBuddy

ChatGPT:
    agent_query providers

    Work begin

    agent_execute:
        role=development
        provider=codex

    observe
    source/git review

    agent_execute:
        role=testing
        provider=codebuddy

    observe
    review result

    Work finish
~~~

同时验证：

~~~text
CodeBuddy disabled
CodeBuddy unavailable
Role mismatch
CodeBuddy Host crash
CodeBuddy cancel
Desktop restart
Provider policy persistence
legacy Start without taskRole/providerId → general routing
legacy Start creates a real Execution through configured general Provider
Codex regression
~~~

---

# 33. 第一版接受的能力边界

CodeBuddy 第一版最低可交付能力：

~~~text
enabled / disabled
health
version
ACP stdio
fresh start
workspace_write
cancel
activity
runtime ownership
job-level recovery
safe claim release
~~~

以下能力只有对应 Evidence Gate 与实现同时完成后才打开：

~~~text
continue
    → CB-005 continuation contract
    + CB-008 implementation / cross-runtime verification

canRecover
    → CB-006 startup_reconcile + Job Recovery Gate
    （不等价于 Session continuation）

token usage
    → CB-005 Usage contract
    + CB-009 Usage implementation

cross-runtime result recovery
    → CB-005 exact history/session contract
    + CB-008 recovery implementation
~~~

如果某项 CodeBuddy 能力无法达到 Codex 同等功能，但能够保证：

~~~text
不串 Workspace
不错误释放 Claim
不自动重放未知副作用
不影响 Codex
不影响 Desktop Core
~~~

则允许该能力保持 unsupported、partial 或 unknown，而不是阻塞整个 Provider 接入。

---

# 34. 外部事实依据

核实日期：2026-09-21。

CodeBuddy：

- ACP 官方文档：https://www.codebuddy.ai/docs/zh/cli/acp
- CLI 参数：https://www.codebuddy.ai/docs/zh/cli/cli-reference
- Permission Mode：https://www.codebuddy.ai/docs/zh/cli/permission-modes
- Permission Rules：https://www.codebuddy.ai/docs/zh/cli/permissions
- HTTP API Beta：https://www.codebuddy.ai/docs/cli/http-api
- CodeBuddy Code v2.153.0：https://www.codebuddy.ai/docs/zh/cli/release-notes/v2.153.0

ACP：

- Agent Client Protocol：https://github.com/agentclientprotocol/agent-client-protocol
- ACP v1 Schema：https://github.com/agentclientprotocol/agent-client-protocol/blob/main/schema/v1/schema.json
- Rust SDK：https://github.com/agentclientprotocol/rust-sdk
- Rust Transport Architecture：https://github.com/agentclientprotocol/rust-sdk/blob/main/md/transport-architecture.md

当前 ACP stable wire protocol 为 v1。CodeBuddy 官方支持 codebuddy --acp，默认 stdio NDJSON；HTTP ACP 虽已提供，但首版 SerenaDesktop 不使用。CodeBuddy v2.153.0 已增加 ACP 等协议的一致性测试，但 SerenaDesktop 仍需以实际固定 binary 的 Contract Probe 作为发布兼容证据。
