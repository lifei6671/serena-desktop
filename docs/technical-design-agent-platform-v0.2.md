# SerenaDesktop Agent 平台化改造技术方案 V0.2

状态：Design Freeze Candidate  
适用范围：SerenaDesktop v1.1.0 之后的下一阶段架构改造  
目标：完成 Serena 解耦、Workspace Registry/项目管理、Rust Source、Provider-Agnostic Agent Control Plane、Agent Activity Observe、Provider-Agnostic Usage、Windows NSIS Release  
实施原则：**先冻结公共契约与安全边界，再分阶段迁移；禁止在同一阶段同时进行大规模目录搬迁与业务语义修改。**

---

# 1. 背景

SerenaDesktop 最初围绕 Serena MCP Server 构建，负责 Serena 的安装、启动、项目激活和远程接入。随着项目演进，SerenaDesktop 已经逐步承担：

- Desktop Runtime；
- MCP Broker；
- Workspace 管理；
- Git；
- CodeGraph；
- Remote Access；
- OAuth；
- Work Orchestration；
- Codex Agent Runtime；
- Crash Recovery；
- Agent Activity；
- bounded Observe；
- Agent Task UI。

当前 SerenaDesktop 已经不再只是 Serena 的启动器，而正在演化为：

> **连接 ChatGPT、本地 Workspace 和本地 Agent Runtime 的 Provider-Agnostic Agent Host。**

当前架构仍存在六组核心问题：

1. Serena 仍然是 Workspace 和部分 Source 能力的硬依赖；
2. ChatGPT 无法通过当前公开 Work Adapter 真正观察 Agent Activity；
3. Token Usage 缺少 Provider 无关的数据模型和准确的 Continuation 统计契约；
4. Agent Runtime 公共层仍然存在 Codex Thread/Turn/Runtime 私有语义泄漏；
5. Windows Release 仍然发布裸 `serena-desktop.exe`，尚未形成安装版生命周期。
6. Broker 全局 `ActiveWorkspace` 同时承担 UI 选择与远程工具执行上下文；多个 ChatGPT 会话、浏览器窗口或 Remote 连接并发时，最后一次 Activate 会改变其他会话后续 Source/Git/CodeGraph 请求的目标目录。

此外，本次 Serena 解耦会直接带来一个新的产品需求：

> **Workspace 不再由 Serena Registry 提供，因此 SerenaDesktop 必须自己承担项目注册、目录选择、项目管理和迁移。**

---

# 2. 本版本目标

V0.2 revision003 完成后，应满足：

1. Serena 未安装、未启动、启动失败或配置损坏时，SerenaDesktop Core 仍能启动；
2. Workspace、Source、Git、Remote、Agent 不依赖 Serena；
3. 原 Serena 同步得到的项目在升级后自动保留；
4. 用户可以通过系统目录选择器手动添加本地目录作为 Workspace；
5. 支持项目 Rename、Remove、Reorder 和 Desktop UI 选择；
6. Workspace 不要求必须是 Git Repository；
7. 四个基础 Source Read 工具迁移到 Rust；
8. Source Write 建立稳定、安全、有界、支持 OCC 的公共契约；
9. Remote Direct Source Write 默认关闭；
10. Serena 降为 Optional Semantic Provider；
11. Agent Runtime 抽象为 Provider-Agnostic Agent Control Plane；
12. Codex 是首个 Agent Provider；
13. ChatGPT 可以通过 `agent_query observe` 感知 Agent 的语义 Activity；
14. Activity Telemetry 与 Execution lifecycle CAS 解耦；
15. Token Usage 形成 Provider 无关模型；
16. Continue 的 Token delta 不重复统计；
17. terminal 后允许有界的 late Usage，Runtime teardown/grace 后冻结；
18. 任务详情展示 Token Usage；
19. 左侧任务 Hover 展示 Total Token；
20. Windows Release 使用 NSIS x64 current-user Installer；
21. GitHub Actions 负责正式安装包构建与 Release；
22. Installed 与旧 portable 版本共享同一 Single Instance Identity，禁止双 Host 同时写 StateStore；
23. Workspace 执行上下文改为 Request/Execution Scoped；每个新 Workspace-scoped 请求以显式 `workspaceId` 解析独立 `WorkspaceLease`，并发请求互不切换；
24. Source、Git、CodeGraph、Serena Semantic、未来 Workspace-scoped Capability Tool 和新建 Agent Execution 的公共契约显式携带 `workspaceId`，不再读取 Broker 全局 `ActiveWorkspace`；
25. Serena 与 CodeGraph 使用 Workspace-scoped Runtime Slot，同一 Workspace 复用、不同 Workspace 隔离；
26. Provider Runtime 按需启动、single-flight 合并并发启动、受容量上限约束，并在空闲后回收；
27. Provider Runtime 的启动、调用、崩溃和停止按 Workspace 隔离，不影响 Desktop Core 或其他 Workspace；
28. Workspace-scoped 工具通过 `WorkspaceCapabilityProvider` 接口和 Registry 接入，Serena/CodeGraph 只是首批内置 Adapter，新增 Provider 不修改 Workspace Core；
29. Workspace 注册不等待任何 Capability 准备；Serena 的 Project Creation、Runtime Activation、Onboarding、Indexing 分层处理，CodeGraph Indexing 保持显式 Local Human Authority。

---

# 3. 非目标

本版本不包含：

- 第二个真实 Agent Provider；
- Claude / Claude Code Provider；
- Gemini Provider；
- Provider Plugin Marketplace；
- 动态 DLL/脚本 Provider 加载；
- 通用 Workflow DAG；
- 新 Scheduler；
- Chain-of-Thought 展示；
- Raw Reasoning 展示；
- stdout/stderr 实时终端；
- Token 美元费用统计；
- Usage 日/周/月报；
- Tauri Updater；
- Windows MSI；
- ARM64 Installer；
- Authenticode 代码签名；
- 自动删除旧 portable exe；
- Provider 通用 Runtime Evidence Schema；
- Serena / CodeGraph / LSP 的统一 Semantic Provider 抽象；
- Remote MCP 远程注册/删除本机 Workspace；
- 默认开放 Remote Direct Source Write；
- 把 MCP Transport Session 当作长期稳定的业务身份，或为 Workspace Routing 新增 Session Map、Expiry、Persistence、Reconnect Binding；
- 运行中的 Execution 随 Desktop UI 选择变化而迁移 Workspace；
- 在 V0.2 支持修改已注册 Workspace 的 Root；需要变更 Root 时使用 Remove + Register。
- 为每个已注册 Workspace 启动常驻 Serena/CodeGraph 进程；
- 动态加载第三方 DLL/脚本或稳定跨版本 Plugin ABI；V0.2 的 `WorkspaceCapabilityProvider` 是进程内 Rust 接口，首版 Provider 编译期注册；
- 提前实现 Serena/CodeGraph/LSP 统一 Semantic Domain abstraction；本版只统一 Workspace 探测、生命周期、路由和安全契约；
- 把 Runtime 容量和 Idle Timeout 暴露为首版用户设置；初始常量在基准测试后冻结。
- 在 Workspace 注册关键路径中创建 Serena Project、启动 Provider Runtime、执行 Onboarding 或构建 Serena/CodeGraph 索引；
- 把 Serena Index 或 Onboarding 完成作为 Semantic Tool 可调用的统一前置条件；
- 允许 Remote 客户端调用通用 Capability Prepare API 或隐式触发 CodeGraph init/reindex。

---

# 4. 当前已核对基线

本节只描述当前实现，不描述目标状态。

## 4.1 Workspace / Serena

当前 Workspace 激活仍依赖 Serena：

```text
Serena installation
    ↓
Serena config
    ↓
Serena process
    ↓
Serena MCP Client
    ↓
activate_project
    ↓
Active Workspace
```

当前 Active Workspace 对象仍包含 Serena Client/PID 语义。

当前 Broker 只有一个全局 Active Workspace。Source/Git/CodeGraph 等工具通过该共享状态决定工作目录，因此会出现确定性的跨会话竞态：会话 B 激活 Workspace B 后，会话 A 未显式指定 Workspace 的下一次调用也会落到 B。这是当前单工作区模型的已知架构限制，不是 V0.2 Target Contract。

当前 `sync_workspaces()` 仍从 Serena 的 `serena_config.yml` 读取项目，并更新 `ManagerConfig.workspaces`。

因此 Serena 不可用时，即使 Broker、Git、Agent Runtime 和 Remote Access 正常，Workspace 仍可能无法进入 Active。

---

## 4.2 Workspace Registry

当前已经存在：

```rust
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
}
```

以及：

```rust
ManagerConfig {
    workspaces: Vec<Workspace>,
    ...
}
```

当前配置校验已经要求：

- Workspace ID 非空且唯一；
- Workspace Name 非空；
- Workspace Root 唯一。

因此 V0.2 不需要重建新的 Workspace 数据库。

> `ManagerConfig.workspaces` 可以直接升级为 Workspace Registry Authority。

---

## 4.3 Project Management

当前缺少正式的 Workspace Registry CRUD。

现有主要流程仍是：

```text
Serena Registry
    ↓
sync_workspaces()
    ↓
ManagerConfig.workspaces
```

UI 仍包含：

```text
同步项目
如何初始化并同步项目
```

当前已经安装并初始化 `tauri-plugin-dialog`，可以直接用于本地目录选择。

---

## 4.4 Source Read

当前可验证的四个公开 Source Read 工具为：

```text
source_read_file
source_list_dir
source_find_file
source_search_pattern
```

这些工具仍然依赖 Serena MCP。

`source_read_file` 已增加 Rust 侧 SHA256 / before-after consistency 校验，但正文读取仍经 Serena。

---

## 4.5 Source Write

**本仓库当前公开 MCP Registry 中未暴露 Source Write。**

V0.2 第 13 节定义 Source Write Target Contract。

若其他分支、实验 build 或未来实现已经暴露同义工具，在 Phase 2C 做 compatibility alias，不以当前设计文档中的旧命令名假定“已经实现”。

---

## 4.6 Agent Activity

当前存在：

```rust
ActivityPhase {
    Provider,
    Tool
}

ToolCategory {
    Build,
    Test,
    Command,
    Read,
    Edit,
    Tool
}
```

当前 `activityRevision` 算法域仍为：

```text
agent-activity-v1
```

且包含：

```text
lastActivityAt
```

因此同一语义 Activity 只要时间戳变化就会产生新 Activity Revision。

---

## 4.7 Activity Store

当前 Activity 更新会修改：

```text
executions.revision
```

即高频 Activity Telemetry 会参与 Execution Lifecycle CAS。

V0.2 必须将二者彻底解耦。

---

## 4.8 Observe

内部 Product Observe 已经存在：

```text
WakeOn::Control
WakeOn::Activity
```

但真正暴露给 ChatGPT 的 Work Adapter `agent_query observe` 当前主要只有：

```text
knownRevision
waitMs
includeResult
```

且 Work Adapter 调用内部 Observe 时固定为：

```text
WakeOn::Control
```

因此当前 ChatGPT 无法真正请求 Activity Wake。

---

## 4.9 Provider

当前 Runtime 仍围绕 Codex：

```text
AgentTaskManager
    ↓
CodexProvider
    ↓
Codex Runtime Pool
    ↓
Codex App Server
```

现有 Runtime Foundation 已具备较强可靠性，本次不重写。

---

## 4.10 Usage

当前 Product DTO / StateStore / UI 没有稳定的 Provider-Agnostic Token Usage Domain Model。

Codex Usage Wire Contract 在实施前必须针对固定的 Codex App Server binary/version/hash 做真实 Contract Test。

---

## 4.11 Release

当前：

```text
tauri.conf.json version = 1.1.0
bundle.active = false
```

当前 Release Workflow 实际执行：

```text
npm ci
npm run lint
cargo test --locked
tauri build --no-bundle
```

最终上传：

```text
serena-desktop.exe
```

当前 CI 并未把 frontend tests、fmt、check、clippy 全部作为 Release Gate。

---

## 4.12 Serena / CodeGraph Runtime

当前 `Broker` 只有一个 `RwLock<Option<Active>>`。`Active` 同时持有一个 Workspace、一个 Serena Client/PID、一个 CodeGraph Binding 和全局切换 generation。

当前 Activate 流程会连接共享 Serena Server、调用 `activate_project`，同时为同一个 Active Workspace 创建 CodeGraph Binding；`codegraph_explore` 再从该全局 Active slot 取 Binding 并在持有 Workspace read lock 时查询。

当前 Serena 子进程统一继承 Supervisor 的 `SERENA_HOME=<app data>/serena-home`，因此共享同一个 `serena_config.yml`。多进程是否会并发改写 project/global config 仍需 Phase 0 实测，当前文档不把推测写成既定故障。

因此当前实现只能保证“单 Active Workspace 的绑定切换”，不能满足两个会话同时对不同 Workspace 使用 Serena/CodeGraph 的隔离要求。V0.2 必须把 Serena Client/Process 与 CodeGraph Binding 从全局 Active slot 拆到 Workspace-scoped Runtime Manager。

2026-09-15 当前设备实测 Serena `1.7.0`：`serena start-mcp-server --project <path|name>` 支持在启动时激活项目；`serena project create` 只有显式传入 `--index` 才在创建后索引；`serena project index [project]` 与 Project Creation 独立，并可在缺少 `project.yml` 时自动创建。Serena 官方 [Project Workflow](https://github.com/oraios/serena/blob/main/docs/02-usage/040_workflow.md) 同样把 Project Creation、Project Activation、Onboarding 和 Indexing 分开，并声明首次按目录激活可以使用默认配置隐式创建项目。因此 Target Contract 不再依赖“枚举所有已初始化项目”的全局命令。

当前设备实测 CodeGraph `1.6.0`：`codegraph init [path]` 会初始化并建立首个索引，`-i/--index` 已只是兼容参数；`codegraph status --json [path]` 返回 `initialized`、`projectPath`、`pendingChanges` 和 `index.state/reindexRecommended`，可作为 Adapter 的机器可读 readiness evidence。当前 Broker Binding 仍以 `<root>/.codegraph/codegraph.db` 检查索引可用性；Target 必须改为通过 Provider Adapter 校验 Root identity 与 status schema。

---

# 5. Runtime Foundation 不变量

以下现有安全契约必须保持。

## 5.1 Runtime Ownership

Codex Process 在 CreateProcess 成功的第一个可运行时刻即属于受管 Runtime。

## 5.2 Job-level Evidence

Windows Job-level termination evidence 高于主 PID existence。

## 5.3 Immutable Runtime Ownership

Execution 一旦绑定 Runtime，不得重绑定。

## 5.4 Provider Terminal Evidence

Provider Terminal Evidence 一旦形成，running-stage transition 永久关闭。

## 5.5 Background Cleanup

Background Cleanup Evidence 必须属于原 Runtime。

## 5.6 Recovery Identity

Recovery 必须保持 exact Runtime / Thread / Turn identity。

不得用 latest Turn 猜测。

## 5.7 Unknown Fail-Closed

`unknown` 状态不得自动释放 Workspace Claim。

## 5.8 Atomic Claim Release

Execution terminal + Workspace Claim release 必须在同一个 SQLite Transaction 提交。

## 5.9 requestKey Idempotency

同一 requestKey 必须保持幂等。

## 5.10 Continue

Continue 创建新的 Execution；旧 terminal Execution 保持 absorbing state。

---

# 6. 目标总体架构

```text
                         ChatGPT
                            │
                            ▼
                   SerenaDesktop MCP
                            │
                            ▼
                    SerenaDesktop Broker
                            │
                            ▼
                    Workspace Resolver
                  workspaceId → WorkspaceLease
                            │
       ┌────────────────────┴──────────────────────┐
       │                                           │
       ▼                                           ▼
Workspace Registry                       Agent Control Plane
  shared catalog                          execution scoped
       │                                           │
       ▼                                           ├── Work
WorkspaceCapabilityRegistry                     ├── Execution
       │                                         ├── Provider Registry
       ├── Source Adapter                        ├── Activity
       ├── Git Adapter                           ├── Usage
       ├── Serena Adapter                        └── Product DTO
       └── CodeGraph Adapter
               │
               ▼
WorkspaceCapabilityManager
       └── (providerId, workspaceId, generation)
              → lazy bounded runtime slot
```

Desktop UI 另有 `DesktopSelectedWorkspace`，只表示用户正在查看或新建任务时默认选择的项目；它不进入 MCP Source/Git/CodeGraph 的 Workspace 解析，也不改变已创建 Execution。

`WorkspaceCapabilityRegistry` 保存编译期注册的 `Arc<dyn WorkspaceCapabilityProvider>`；`WorkspaceCapabilityManager` 统一 Workspace 探测、能力路由、Runtime Slot 和 Health 投影。它们不取代 Agent Control Plane。Source/Git Adapter 声明无常驻 Runtime；Agent 继续由 Execution-scoped Runtime 管理；Serena/CodeGraph 是首批声明 Workspace-scoped process 的 Provider。

Agent Control Plane：

```text
Agent Control Plane
        │
        ▼
ProviderRegistry
        │
        ▼
Arc<dyn AgentProvider>
        │
        ▼
Codex Adapter
        │
        ├── App Server
        ├── Thread / Turn
        ├── Runtime Pool
        ├── Windows Job
        ├── Recovery
        └── Evidence
```

---

# 7. Workspace Registry 与项目管理

## 7.1 Workspace Authority

V0.2 起唯一项目注册 Authority：

```text
ManagerConfig.workspaces
```

它是全局共享的 **Workspace Registry**，只回答“哪些本地 Root 已获 Local Human 注册授权”，不回答“所有客户端当前正在使用哪个 Workspace”。

以下内容不再决定 Workspace 是否存在：

```text
Serena registry
.serena/project.yml
Serena Server
Serena MCP
.git
CodeGraph index
```

---

## 7.2 Workspace 定义

Workspace 是：

> 用户显式注册的一个本地目录。

为避免把产品对象、执行上下文和工具进程混为一谈，V0.2 固定三个概念：

```text
Project
    用户在 SerenaDesktop 中持久化管理的代码目录；对应 Workspace Registry Entry

Workspace Context
    某个 Desktop selection、MCP Request 或 Agent Execution 对 Project 的使用上下文

Capability Runtime
    Serena / CodeGraph / Future Adapter 针对 WorkspaceLease 按需持有的能力实例
```

现有公共字段 `workspaceId` 和 Rust `Workspace` 类型为兼容性继续保留，但语义必须满足：

```text
Project 添加 ≠ Provider 准备
Workspace Context 选择/绑定 ≠ Provider Runtime 启动
Tool Call / Local Prepare Action → 按 Descriptor 获取 Capability Runtime
```

Workspace 不要求必须是 Git Repository。

非 Git 目录：

```text
Workspace  ready
Source     ready
Agent      ready
Git        unavailable
CodeGraph  capability-specific
Semantic   capability-specific
```

Git 不是 Workspace 可用前置条件。

---

## 7.3 Workspace Model

V0.2 在现有模型上只增加 Path Authority 所需的最小版本字段：

```rust
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    pub generation: u64,
}
```

新注册 Workspace 的 `generation=1`；旧配置缺失该字段时迁移为 `1`。`ManagerConfig` 同时保存 `workspace_registry_revision`，旧配置缺失时迁移为 `1`。不在 V0.2 一次增加其他 metadata 字段。

---

## 7.4 Workspace ID

Workspace ID：

- 一次生成；
- 稳定；
- Rename 不改变；
- 不由 Name 或 Folder Basename Hash 派生。

推荐复用现有 ID Generator / UUID / ULID。

---

## 7.5 Workspace Name

Name：

```text
trimmed non-empty
```

允许同名 Workspace。

例如：

```text
backend
D:\company-a\backend

backend
E:\company-b\backend
```

UI 使用 Root 消歧。

---

## 7.6 Workspace Root

注册时：

```text
Directory Picker
    ↓
canonicalize
    ↓
validate directory
    ↓
duplicate detection
    ↓
persist canonical root
```

Windows Root Identity 比较使用 canonicalized case-insensitive identity。

以下只能注册一次：

```text
E:\Repo
e:\repo
E:\foo\..\Repo
```

重复返回：

```text
WORKSPACE_ALREADY_EXISTS
```

---

# 8. Project Management API

项目管理是 **Local Human Authority**。

不得把以下能力暴露给 Remote MCP：

```text
workspace_register
workspace_remove
workspace_rename
```

Remote MCP 继续只允许：

```text
workspace_list
workspace_get
```

V0.2 Remote MCP 不再通过 `workspace_current`、`workspace_activate`、`workspace_deactivate` 建立执行上下文，见第 10.5 节。Workspace-scoped Tool 必须在每次请求显式提供 `workspaceId`；Desktop UI 选择也不创建远程隐式 Context。

### Workspace Discovery

ChatGPT 尚不知道 `workspaceId` 时，先调用只读 `workspace_list`；也可以使用最终冻结的等价只读 Workspace Query API。Discovery 只返回已授权 Workspace catalog，包括 `workspaceId`、`name` 和必要的展示/能力状态，不 Activate Workspace，也不建立任何 Binding：

```text
workspace_list
    ↓
project-4 = serena-desktop
    ↓
source_read_file(
    workspaceId = project-4,
    relative_path = src-tauri/src/mcp/mod.rs
)
```

同一 ChatGPT 任务链一旦获得 `workspaceId = project-4`，后续 Source/Git/CodeGraph/Serena Semantic 请求直接重复显式传递该 ID；`workspace_list` 不是每次调用的前置步骤。`workspace_get(workspaceId)` 只查询指定 Entry，也不把该 ID 绑定到后续请求。

禁止把 Query 解释为 Activate：

```text
workspace_list / workspace_get
    ↓
ChatGPT obtains workspaceId
    ↓
each Workspace-scoped request explicitly carries workspaceId
```

Discovery 与 Execution Authority 分离；查询结果、查询顺序、最近一次查询的 ID 均不得进入 Workspace Routing。

否则远程客户端可以扩大本机文件访问边界。

---

## 8.1 Directory Picker

Tauri IPC：

```text
workspace_pick_directory()
```

返回：

```text
Option<Path>
```

用户取消 Picker：

```text
None
```

属于正常 no-op，不是 error。

Picker 与 Register 分开，避免 UI 选择动作本身直接变更持久化 Registry。

---

## 8.2 Registration 与 Capability Readiness 边界

Directory Picker 返回 Root 后，Workspace 注册关键路径只处理 SerenaDesktop 自己的 Authority：

```text
workspace_pick_directory()
    ↓
workspace_inspect_directory(root)  // exists / directory / canonical root / basic metadata only
    ↓
user confirms
    ↓
workspace_register(root, name?)
    ↓
Project immediately visible in Registry/UI
    ↓
async workspace_capability_observe(workspaceId)
```

`workspace_inspect_directory` 不调用 Serena/CodeGraph，不创建 Provider 文件，不等待 Language Server 或索引。任何 Provider 缺失、未准备、状态未知或探测失败都不能阻止 `workspace_register` 成功。

注册完成、打开项目详情、显式刷新或 Tool acquire 时，`WorkspaceCapabilityRegistry` 才根据 Descriptor 并行执行有界只读观察。每个 Provider 返回统一外壳和动态阶段：

```json
{
  "providerId": "serena",
  "installation": "installed",
  "readiness": "not_prepared",
  "runtimeState": "stopped",
  "checkedAt": 0,
  "stages": [
    {
      "id": "project_configuration",
      "displayName": "项目配置",
      "state": "absent",
      "requirement": "auto_preparable",
      "messageCode": "CAPABILITY_STAGE_NOT_PREPARED"
    },
    {
      "id": "index",
      "displayName": "符号索引",
      "state": "unknown",
      "requirement": "optional",
      "messageCode": null
    }
  ],
  "actions": [
    { "id": "prepare", "displayName": "准备", "authority": "local_human", "execution": "manager_ensure_runtime" },
    { "id": "build_index", "displayName": "建立索引", "authority": "local_human", "execution": "provider_prepare" }
  ]
}
```

公共枚举：

```text
installation = installed | not_installed | check_failed
readiness = not_prepared | preparing | ready | degraded | error | unknown
runtimeState = stopped | starting | ready | error | stopping
stage.state = absent | pending | running | ready | stale | error | unknown
stage.requirement = required | auto_preparable | optional
```

`readiness` 表示“该 Provider 对当前 Workspace 的总体准备情况”，不再使用含义过载的 `initialized: bool`。Provider 可以声明自己的阶段和动作；Core/UI 只渲染 Descriptor/DTO，不按 `providerId` 分支。只读观察必须有独立 timeout/cancellation；失败只更新该 Provider 为 `unknown/error`，不影响 Workspace、Source/Git 或其他 Provider。

观察结果是带 `checkedAt` 的 freshness snapshot，不写成 Workspace Authority。持久化 Provider 工件、Runtime handle 和探测缓存都不能反向证明 Workspace 合法；每次 Tool acquire 仍先解析新的 `WorkspaceLease`。

### Serena Readiness

Serena Adapter 把状态拆成：

```text
project_configuration = absent | ready | error
runtime              = stopped | starting | ready | error | stopping
index                = absent | building | ready | stale | error | unknown
onboarding           = not_started | running | ready | error | unknown
```

`<canonicalRoot>/.serena/project.yml` 缺失表示 `project_configuration=absent`，但不是 Workspace 不可用，也不是永久的 Semantic unavailable；它表示第一次 Serena Tool acquire 可以按第 11.2 节使用官方默认配置路径自动准备。Project Configuration 就绪也不代表 Runtime、Index 或 Onboarding 已就绪。

Index 是可选性能准备：`absent/unknown/stale` 不阻止 Serena Runtime 启动或普通 Semantic Tool 调用。只有 Provider 能从冻结的机器可读 command/API 或本次成功的 index operation 获得证据时才能报告 `ready`；不得从 `project.yml` 的存在推断索引已完成。Onboarding 属于 Agent 的项目认知/Memory 工作流，不在注册或 Runtime acquire 中自动执行，也不作为 Semantic Tool ready 的前置条件。

### CodeGraph Readiness

CodeGraph Adapter 在已安装时调用：

```text
codegraph status --json <canonicalRoot>
```

只有 machine-readable response 同时满足以下条件才报告 index `ready`：

```text
initialized == true
canonical(projectPath) == canonicalRoot
index.state == complete
```

`pendingChanges` 非零或 `reindexRecommended=true` 时报告 `degraded/stale`；目录不存在、`initialized=false` 或 status 明确未初始化时报告 `not_prepared/absent`；命令/Schema/Root identity 校验失败报告 `unknown/error`。不得只因 `.codegraph/` 目录存在就宣称索引可用。

CodeGraph binary 未安装但 `.codegraph/` 已存在时报告 `installation=not_installed, readiness=unknown`，保留磁盘工件且不猜测其有效性；安装恢复后再用 `status --json` 重新观察。

CodeGraph `init/index/sync` 是可能耗时并修改 Workspace 内 `.codegraph/` 的显式准备动作，不能进入注册关键路径，也不能由普通 Remote Tool acquire 隐式执行。用户可从项目卡片执行 `[建立索引]` 或 `[更新索引]`，见第 8.9 节。

---

## 8.3 Register

```text
workspace_register(root, name?)
```

行为：

1. root 必须存在；
2. 必须是目录；
3. canonicalize；
4. canonical root 不得重复；
5. 生成稳定 Workspace ID；
6. name 缺失时使用 Folder Basename；
7. 写入 ManagerConfig；
8. 持久化成功后立即进入项目列表，Capability 状态先显示 `checking/unknown` 并异步刷新；
9. 不自动选择；
10. 不启动任何 Workspace Capability Runtime；
11. 不创建 Provider Project、不执行 Onboarding、不建立索引；
12. 不创建任何 Remote Session Workspace binding。

Register/Rename/Remove/Reorder 必须复用现有 `SupervisorState.operation: Mutex<()>` 的配置变更入口，并继续通过 `config::save` atomic persist；不得另建一套与 `replace_workspaces`、`replace_config`、`replace_remote_access` 竞争的 ManagerConfig 写路径。

---

## 8.4 Rename

```text
workspace_rename(id, name)
```

只修改：

```text
Workspace.name
```

不得修改：

```text
id
root
Execution frozen workspace identity
```

---

## 8.5 Remove

```text
workspace_remove(id)
```

只从 Workspace Registry 移除。

绝不：

```text
delete directory
delete git repo
delete .serena
delete .codegraph
delete historical Work
delete historical Execution
```

---

## 8.6 Remove Safety

以下状态禁止 Remove：

```text
存在 active WorkspaceWriteGuard
OR
存在 nonterminal Execution
OR
存在 Agent Workspace Claim
OR
任一 Capability Prepare/Index operation 正在 running
OR
任一 workspace_scoped_process Provider Runtime 正在 starting 或 in-flight
```

返回：

```text
WORKSPACE_IN_USE
```

已捕获 `WorkspaceLease` 的纯 Source Read/Search 允许完成；Remove 只阻止未来解析。Workspace-scoped Provider 请求不是纯 Read Lease：调用期间持有 Runtime in-flight guard，因此阻止 Remove。Prepare/Index operation 可能写入 Provider 工件或占用 Language Server，同样持有 Capability Operation Claim。Desktop UI 正在选中该 Workspace 不构成文件系统占用：Remove 成功后清除本地 UI 选择，后续显式引用旧 ID 的请求返回 `WORKSPACE_NOT_FOUND`。

Idle Provider Slot 不阻止 Remove，但必须先按第 10.9 节安全 stop/dispose；停止失败时保留 Registry Entry 并返回 `WORKSPACE_CAPABILITY_STOP_FAILED`。

存在 WriteGuard/Execution/Agent Claim 时，用户需要先：

```text
finish or cancel write/execution
```

再 Remove。

---

## 8.7 Reorder

```text
workspace_reorder(ids[])
```

只改变 Display Order。

如果当前 `ManagerConfig.workspaces` 顺序已经是 UI 顺序，则可直接使用 Vector 顺序，不必增加 order 字段。

要求：

- IDs 必须是当前 Registry 的完整排列；
- 不得丢失/重复 Workspace；
- 不影响 DesktopSelectedWorkspace；
- 不影响其他请求的显式 Workspace Context；
- 不影响 Running Execution。

---

## 8.8 Open Folder / Copy Path

属于便利操作：

```text
workspace_open_folder
workspace_copy_path
```

不是 Architecture Gate。

---

## 8.9 Desktop Select / Explicit Capability Prepare

Local Desktop 用户显式选择 Workspace：

```text
workspace_select(id)
```

`workspace_select` 只更新 `DesktopSelectedWorkspace` 和当前详情页，不启动、停止、创建或索引任何 Capability。Project 添加、Desktop 选择、Remote Request Workspace Context 和 Provider Runtime Activation 是四个独立生命周期。

项目卡片根据 Provider Descriptor/Readiness DTO 动态展示动作，例如：

```text
Serena    未准备       [准备]
Serena 索引 未建立     [建立索引]
CodeGraph 未建立索引   [建立索引]
```

Local Desktop 显式动作统一调用：

```text
workspace_capability_prepare(workspaceId, providerId, actionId)
```

该 API 是 Local Human Authority，不暴露给 Remote MCP。Core 只验证 WorkspaceLease、Provider/Action Descriptor、operation single-flight 与权限，再把动作交给 Adapter；不能按 Serena/CodeGraph ID 决定命令。

- Serena `prepare`：`manager_ensure_runtime, warmRuntime=true`；启动或复用该 Workspace 的独立 Runtime，如果缺少 Project Configuration，Adapter 通过官方支持的首次按路径激活/`--project <canonicalRoot>` 默认配置路径完成创建，不执行 index 或 Onboarding；
- Serena `build_index`：`provider_prepare, warmRuntime=false`；确保 Project Configuration 已存在后，运行受管的 `serena project index <canonicalRoot>`，索引完成后不要求 Runtime 常驻；
- CodeGraph `build_index`：`provider_prepare, warmRuntime=false`；首次执行受管的 `codegraph init --yes <canonicalRoot>`；
- CodeGraph `update_index`：`provider_prepare, warmRuntime=false`；对现有 index 执行受管的 `codegraph sync <canonicalRoot>`；只有用户明确选择 rebuild 时才执行 `codegraph index <canonicalRoot>`。

所有动作都必须异步、可取消、输出安全进度并有 Provider-specific bounded timeout；失败只更新对应 Stage/Provider，不回滚 Workspace 注册或 Desktop selection。从 A 选择 B 不得 retarget 或 kill A 的 Provider Process；只有 Tool acquire、用户点击 `[准备]`，或者显式索引动作自身需要短生命周期 Runtime 时才创建资源。

---

# 9. 旧 Serena 项目迁移

当前 Serena 同步结果已经持久化在：

```text
ManagerConfig.workspaces
```

因此升级时：

> 现有 Serena-synced projects 无需重新导入。

V0.2 启动直接保留旧 `config.workspaces`。

---

## 9.1 停止启动时 Serena Sync

Phase 2A.1 完成后：

```text
startup
```

不得再调用 Serena Registry 去覆盖 Workspace Registry。

否则会产生：

```text
用户手动管理项目
    ↓
下次启动
    ↓
Serena Registry 覆盖本地 Registry
```

---

## 9.2 从 Serena 导入

可以保留一个显式操作：

```text
从 Serena 导入
```

语义是 Import，不是 Sync。

流程：

```text
read Serena registry
    ↓
canonicalize roots
    ↓
compare Workspace Registry
    ↓
add missing roots
```

Import：

- 不删除本地 Workspace；
- 不 Rename；
- 不 Reorder；
- 不切换 DesktopSelectedWorkspace；
- 不改变任何 Remote Request 的显式 Workspace Context；
- 重复执行幂等。

Serena 不存在：

```text
Import unavailable
```

但 Project Management 正常。

---

## 9.3 Missing Root

如果注册后用户在磁盘删除/移动目录：

Registry 不自动删除。

状态：

```text
registered
rootStatus = missing
```

后续 `WorkspaceResolver::resolve(workspaceId)` 或任意 Workspace-scoped Tool 请求：

```text
workspaceId
    ↓
WorkspaceResolver
    ↓
WORKSPACE_ROOT_NOT_FOUND
```

用户可手动 Remove。

---

# 10. Multi-Workspace Context 与 WorkspaceLease

V0.2 删除“Broker 全局 `ActiveWorkspace` 是工具执行上下文”的目标模型，并明确拆分三种状态：

```text
Workspace Registry
    全局共享；定义已注册、获授权的 Workspace 集合

DesktopSelectedWorkspace
    Desktop UI 状态；用于展示和新建任务默认值

Request Workspace
    本次 Workspace-scoped 调用的 Authority；只由 request.workspaceId 显式解析

Execution Workspace Snapshot
    Execution 创建时冻结；后续只由 executionId / parentExecutionId 继承定位
```

三者不得互相隐式覆盖。Desktop 选择 A、MCP 请求访问 B、运行中 Execution 使用 C 可以同时成立。

---

## 10.1 DesktopSelectedWorkspace

`DesktopSelectedWorkspace` 只用于：

- Desktop 项目页和详情页展示；
- 用户点击切换正在查看的项目；
- Agent 页面创建新 Task 时的默认 Workspace。

它不得决定：

```text
source_* target root
git_* target root
codegraph_* target project
Serena Semantic target project
future Workspace-scoped Capability target
existing execution workspace
```

选择变化不增加任何 Workspace generation，也不使进行中的 Read/Write/Execution 失效。

---

## 10.2 Registry Revision 与 Workspace Generation

全局 `workspaceRegistryRevision` 只在 Registry 结构变化时递增：

```text
register
rename
remove
reorder
path-authority metadata change
```

每个 Workspace Entry 独立维护 `generation`。只有该 Entry 自身发生影响 Path Authority 的变化时才递增；其他 Workspace 的注册、删除、重命名、排序和 Desktop 选择均不得改变它。

V0.2 不支持原地修改 Root，因此正常的 Rename/Reorder 不改变 Workspace generation。未来如引入 Root 修改，必须递增该 Entry generation。Remove 后旧 ID 不得复用。

因此 V0.2 正常生命周期内多数 Entry 的 generation 会保持 `1`；它是未来 Path Authority 变化的版本位，不是 Desktop selection、请求次数或 Provider Runtime restart 计数器。

`workspaceRegistryRevision` 用于 Registry 列表缓存和 UI freshness；Write Commit 不得用全局 revision 做并发门禁，否则无关 Workspace 的变更会错误中止当前操作。

`workspace_list` 返回顶层 `registryRevision`，每个 Entry 返回自己的 `generation`，客户端据此刷新目录列表但不得把二者混为一个版本。

---

## 10.3 WorkspaceResolver 与 WorkspaceLease

所有新 Workspace-scoped 请求统一通过同一个内部入口解析；请求中的 `workspaceId` 是唯一公共路由输入：

```rust
pub struct WorkspaceLease {
    pub workspace_id: WorkspaceId,
    pub canonical_root: PathBuf,
    pub generation: u64,
}

WorkspaceResolver::resolve(workspace_id) -> Result<WorkspaceLease, WorkspaceError>
```

Resolver 必须：

1. 在 `ManagerConfig.workspaces` 中按稳定 ID 查找 Entry；
2. 检查 Root 存在、是目录并完成 canonicalization；
3. 捕获该 Entry 的 `workspace_id`、`canonical_root`、`generation`；
4. 释放 Registry read lock；
5. 由 Source/Git/CodeGraph/Agent 在 Lease 或冻结快照上完成后续操作。

```text
request.workspaceId
    ↓
WorkspaceResolver
    ↓
WorkspaceLease {
    workspaceId,
    canonicalRoot,
    generation
}
    ↓
capability operation
```

长时间 Read/Search、Git subprocess、CodeGraph/Serena Semantic query 和 Agent Runtime 不得一直持有 global Workspace Registry lock，也不得二次读取 DesktopSelectedWorkspace 决定 Root。

---

## 10.4 显式 MCP Workspace Contract

最终公共契约要求所有新 Workspace-scoped Tool 显式携带顶层 `workspaceId`：

```json
{
  "workspaceId": "project-4",
  "relative_path": "src-tauri/src/mcp/mod.rs"
}
```

采用顶层字符串而不是 `{ "workspace": { "id": ... } }`，以减少 Schema 和 Token 开销。`workspaceId` 只允许引用 Local Human 已注册的 Registry Entry，Remote MCP 不可传入任意 Root。

| Tool family | Workspace contract |
|---|---|
| `workspace_list` | 无 `workspaceId`，返回 Registry catalog |
| `workspace_get` | 必填 `workspaceId`；只读 Query，不建立 Binding |
| `source_*` | 必填 `workspaceId`；已有 `relative_path` 字段保持不变，语义为 Workspace-relative path |
| `git_*` | 必填 `workspaceId`；已有可选 `path` 字段保持不变，语义为 Workspace-relative path |
| `codegraph_*` | 必填 `workspaceId` |
| Serena Semantic Tool | 必填 `workspaceId` |
| 未来 Workspace-scoped Capability Tool | 必填 `workspaceId` |
| `agent_execute start` / 新建 Work | 必填 `workspaceId`，创建时冻结 |
| `agent_query` / cancel / continue | 不接收新 `workspaceId`，按既有 execution/work identity 定位 |

响应中的 Workspace provenance 统一为：

```json
{
  "workspace": {
    "id": "project-4",
    "generation": 18
  }
}
```

不得接受 caller-provided absolute root。需要 `workspaceId` 的请求缺少该字段时直接返回稳定错误 `WORKSPACE_CONTEXT_REQUIRED`；字段存在但类型错误、空串或仅空白时返回 `INVALID_PARAMS`；语法有效但未注册时返回 `WORKSPACE_NOT_FOUND`。

Workspace Routing 不得使用任何 fallback：

```text
DesktopSelectedWorkspace
MCP Transport Session
last request Workspace
Broker global ActiveWorkspace
workspace_activate implicit state
```

“Workspace-relative path”描述参数语义，不要求把各 Tool 的公共字段统一重命名。已有 Tool 保持稳定字段名：Source 使用 `relative_path`，Git 使用 `path`；未来 Tool 也保留各自冻结的公开 Schema。不是所有 Workspace-scoped Tool 都包含路径参数；但任何 Tool 一旦包含文件或目录路径参数，就只能接受 Workspace-relative path。禁止 caller 传 Windows absolute path、Unix absolute path、UNC path、Workspace root 或可逃逸 Root 的 `..` 路径。绝对路径只由服务端内部派生：

```text
request.workspaceId
    ↓
WorkspaceResolver
    ↓
WorkspaceLease.canonicalRoot

public tool-specific path field
    ↓
WorkspacePathResolver
    ↓
Workspace-relative semantic path
    ↓
canonical absolute target
    ↓
boundary / junction / reparse validation
```

同一规则适用于 Local Tauri IPC：页面可以用 DesktopSelectedWorkspace 填充表单默认值，但发起 Source/Git/Capability/Agent 操作时必须把最终 `workspaceId` 显式传给 Command；Rust Command 不读取 Desktop selection 作为执行兜底。

---

## 10.5 Session ID 不属于 Workspace Authority / No Session Workspace Binding

当前 HTTP MCP 已配置：

```text
legacy_session_mode = false
json_response = true
```

实际 Transport 不提供可作为业务 Authority 的稳定 `Mcp-Session-Id`。即使未来引入 MCP Session / Transport Session，它也只能用于 Transport 生命周期、日志关联、诊断、限流等连接层能力，不能决定 Source/Git/CodeGraph/Serena Semantic Tool 实际访问哪个 Workspace。

V0.2 不新增 `SessionWorkspaceBindings`，也不为 Workspace Routing 设计 Session Map、Session Expiry、Session Persistence、Reconnect Binding，且不为兼容旧 `workspace_activate` 改回有状态 Transport。

`workspace_activate`、`workspace_current`、`workspace_deactivate` 从 V0.2 新 `tools/list` 移除。若滚动升级期间必须短暂保留旧 handler，它们只能作为 deprecated compatibility surface，不能写入 Workspace Context、改变 Desktop selection 或影响后续请求路由；后续 major contract revision 可以删除。

Workspace Context 解析只有一条路径：

```text
request.workspaceId
    ↓
WorkspaceResolver
    ↓
WorkspaceLease
```

缺少 required `workspaceId` 返回 `WORKSPACE_CONTEXT_REQUIRED`。类型错误、空串或仅空白字符串在 Tool Schema/参数校验阶段返回 `INVALID_PARAMS`。只有通过 WorkspaceId 语法校验、但 Registry 中不存在的非空 ID 返回 `WORKSPACE_NOT_FOUND`。不得按 OAuth subject、连接来源、Transport Session、最近请求、`workspace_activate` 状态、Broker 全局 ActiveWorkspace 或 DesktopSelectedWorkspace 猜测 Workspace。

---

## 10.6 Agent Execution Workspace

`agent_execute start(workspaceId=A)` / 新建 Work 的入口必须显式接收 `workspaceId`。Execution 创建时原子冻结：

```text
execution.workspace_id
execution.canonical_workspace_root
execution.workspace_generation
```

之后：

- DesktopSelectedWorkspace 变化不影响 Execution；
- 其他 MCP 请求的显式 Workspace Context 不影响 Execution；
- Workspace Rename 不影响 Execution；
- nonterminal Execution / Agent Workspace Claim 阻止 Remove；
- `agent_query`、cancel 和 `agent_continue(parentExecutionId)` 不接收新的 `workspaceId`；
- Continue 仍创建新的 Execution，但只能继承原 Execution 的 `workspace_id`、`canonical_workspace_root`、`workspace_generation`；
- 若恢复时冻结的 identity 与 Registry Entry 不再一致，按现有 Unknown/Fail-Closed 原则停止派发并保留 Claim，不猜测新 Root。

这样避免 `Execution identity → Workspace A` 与 `caller workspaceId → Workspace B` 的双 Authority，也可以同时运行 `Execution E1 → Workspace A` 与 `Execution E2 → Workspace B`，不会因 UI 或其他请求而迁移目录。

---

## 10.7 Workspace Capability Routing、Git 与 CodeGraph

Tool Handler 先从请求的 `workspaceId` 解析 WorkspaceLease，再交给明确的 capability owner：

```text
Request
    ↓
request.workspaceId
    ↓
WorkspaceResolver
    ↓
WorkspaceLease
    ↓
WorkspaceCapabilityManager
    ↓
Provider.call(lease, runtime, tool)
```

Capability owner：

```text
WorkspaceCapabilityManager
    ├── SourceProvider            in-process / no daemon
    ├── GitProvider               stateless command / no daemon
    ├── SerenaCapabilityProvider  workspace-scoped process
    └── CodeGraphCapabilityProvider
                                 workspace-scoped process + index
```

Registry 首版编译期注册：

```text
source
git
serena
codegraph
```

核心接口必须 object-safe，且不包含 Serena/CodeGraph 专有字段：

```rust
pub trait WorkspaceCapabilityProvider: Send + Sync {
    fn descriptor(&self) -> &WorkspaceCapabilityDescriptor;
    fn probe_installation(
        &self,
    ) -> BoxFuture<'_, Result<CapabilityInstallation, CapabilityProviderError>>;
    fn observe_readiness(
        &self,
        lease: WorkspaceLease,
    ) -> BoxFuture<'_, Result<CapabilityReadiness, CapabilityProviderError>>;
    fn prepare<'a>(
        &'a self,
        lease: WorkspaceLease,
        action: CapabilityPrepareAction,
        activity: &'a dyn CapabilityActivitySink,
    ) -> BoxFuture<'a, Result<CapabilityPrepareResult, CapabilityProviderError>>;
    fn start(
        &self,
        lease: WorkspaceLease,
    ) -> BoxFuture<'_, Result<CapabilityRuntimeHandle, CapabilityProviderError>>;
    fn call<'a>(
        &'a self,
        lease: &'a WorkspaceLease,
        runtime: Option<&'a CapabilityRuntimeHandle>,
        tool: WorkspaceToolCall,
    ) -> BoxFuture<'a, Result<WorkspaceToolResult, CapabilityProviderError>>;
    fn stop(
        &self,
        runtime: CapabilityRuntimeHandle,
    ) -> CapabilityFuture<'_, Result<StopEvidence, CapabilityStopFailure>>;
}
```

Descriptor 至少声明：

```text
providerId
displayName
toolNames
runtimeModel = in_process | stateless_command | workspace_scoped_process
readinessProbe = required | none
preparationPolicy = none | auto_on_first_tool_call | explicit_only
stageDescriptors[]
actionDescriptors[] = actionId / authority / execution / warmRuntime
runtimePolicy = maxInstances / idleTimeout / perSlotConcurrency
```

`action.execution` 只允许 `manager_ensure_runtime | provider_prepare`：前者由 Manager 走与 Tool acquire 相同的 readiness/auto-prepare/start 路径，后者才调用 Adapter `prepare` 执行 Index/Sync 等持久化动作。`warmRuntime` 只决定动作成功后是否保留一个 idle Runtime Slot，不得由 UI 自行拼接调用顺序。

每次 `call` 都显式接收由服务端 Resolver 产生的 `WorkspaceLease`。Source 只以 `lease.canonical_root + tool.relative_path` 解析目标，Git 执行 `git -C lease.canonical_root` 并把可选 `tool.path` 按 Workspace-relative 语义解析，Serena/CodeGraph 使用 WorkspaceLease identity 取得 Workspace-scoped Runtime Slot，并验证 Lease 与 Runtime Handle 的 `(providerId, workspaceId, generation)`；`WorkspaceToolCall` 不包含 caller-provided root，也不能替代 Lease。Source/Git Adapter 不得读取全局状态、Desktop selection 或 Tool payload 中的隐式 Root。

Source/Git Adapter 对 `prepare/start/stop` 使用无准备、无 Runtime 语义；只有声明准备动作的 Provider 才能写入自己的受管工件，只有 `workspace_scoped_process` Provider 创建 Runtime Slot。`WorkspaceCapabilityManager` 和 Workspace UI 只能按 Descriptor/trait 调用，不得出现 `if providerId == "serena"` 或 `if providerId == "codegraph"` 的核心分支。Provider-specific readiness、准备命令、启动参数、阶段状态和上游协议全部封装在 Adapter 内。

`CapabilityRuntimeHandle` 是带 `providerId` 的 Manager-owned opaque handle，只能交回创建它的 Provider；普通 Tool call 在 in-flight guard 内借用 handle，只有安全 stop 才转移所有权。`stop` 成功时 Provider 完成安全停止并消费 handle；失败时必须通过非 wire 的 `CapabilityStopFailure { runtime, error }` 返还同一 handle 和安全 `CapabilityProviderError`，Manager/Slot 继续持有清理责任和容量。该 carrier 不 Serialize，不含 PID、port、root 或 raw error；Manager 不 downcast、重建或 Clone handle，也不解释内部 Client/Process/Protocol。Registry 启动时验证 Provider ID、Tool name、Runtime policy 和 compatibility error mapper 唯一且完整，失败则 fail-fast，不部分发布 Tool surface。

外部 binary Provider 必须声明 Version Contract：`detectedVersion`、支持范围和 machine-readable contract probe。版本缺失或不兼容时只把该 Capability 标为 unavailable/error；Serena/CodeGraph 不参与 Agent Claim/Recovery Safety，因此 V0.2 不要求像 Codex 一样 pin binary hash。

Serena 首版声明 `readinessProbe=required`、`preparationPolicy=auto_on_first_tool_call`，但自动准备的 allowlist 只包含默认 Project Configuration + Runtime Activation，不包含 Index/Onboarding。CodeGraph 声明 `readinessProbe=required`、`preparationPolicy=explicit_only`；Source/Git 为 `none`。所有 Provider 都不得因 Desktop selection 自动准备或预热，未来 Provider 通过 Descriptor 选择策略而不是修改 Core。

Tool 名称和 Input/Output Schema 仍由已有 MCP Tool Registry/Dispatcher allowlist 冻结。Provider 不能在运行时注入任意 Tool；新增 Provider 需要显式注册 Adapter、Schema、安全边界和测试，但不修改 Workspace Core 或 Runtime Manager。

`WorkspaceCapabilityProvider` 与第 17 节 `AgentProvider` 是两个不同端口：前者管理 Workspace-scoped 工具探测/调用/Runtime，后者管理 Execution lifecycle、Recovery 和 Runtime Evidence。不得为追求统一而让 Capability Provider 获得 Agent Claim/terminal 权限。

Git Tool 必须显式 Workspace Scoped：

```text
git_status(workspaceId)
git_diff(workspaceId, path?, scope?)
git_log(workspaceId, ...)
```

执行路径固定为：

```text
workspaceId → WorkspaceResolver → canonicalRoot → git -C canonicalRoot ...
```

CodeGraph 不得保留单一 Global Active Project。目标模型：

```text
WorkspaceCapabilityManager[provider=codegraph]
    ├── project-1 → RuntimeSlot(index-1, process-1)
    ├── project-2 → RuntimeSlot(index-2, process-2)
    └── project-4 → stopped / no live process
```

所有 `codegraph_*` Tool 显式接收 `workspaceId`，再调用 `WorkspaceCapabilityManager.acquire("codegraph", WorkspaceLease)`。每个 live process 从启动到停止只绑定一个 `(providerId, workspaceId, generation, canonicalRoot)`，不得执行“全局切 Project 后复用进程”。

同一 Workspace 的多个显式请求复用同一个 Slot；不同 Workspace 必须使用不同 Slot。Workspace 隔离是硬保证，跨 Workspace 真正并发只在 Provider 的 `runtimePolicy.maxInstances` 容量允许时成立。现有 `.codegraph` index 可以复用，但首次 acquire 必须通过 `codegraph status --json` 校验它属于当前 canonicalRoot 且 index state 可查询；index 存在、Runtime Slot 存在或进程 ready 都不构成 Workspace Authority。

Lazy start 只启动已存在且通过校验的 CodeGraph index Runtime，不得隐式执行 init/index/sync 或创建 `.codegraph`。index absent 时返回 `CODEGRAPH_NOT_INITIALIZED`/统一 `WORKSPACE_CAPABILITY_PREPARATION_REQUIRED`，由 Local Desktop 提供 `[建立索引]`；stale 时可按 Descriptor 标为 degraded 并返回 freshness provenance，但不能静默更新。

容量满时先按第 10.9 节驱逐无 in-flight 的 LRU Slot；只有所有候选均不可驱逐时才返回 `CODEGRAPH_BUSY`。不得为了腾出容量终止正在执行的其他 Workspace 查询，也不得把其他 Workspace 的 live process retarget 到当前 Workspace。

---

## 10.8 并发与生命周期语义

```text
Request A: source_read_file(workspaceId=A)
    → Lease(A, root=A, generation=7)

Request B: source_read_file(workspaceId=B)
    → Lease(B, root=B, generation=3)
```

两者可完全并发。

- Read/Search 捕获 Lease 后允许完成；期间 Entry 被 Remove 只影响未来请求。
- Source Write 的 Resolve + `WorkspaceWriteGuard` acquire 必须与 Remove 检查互斥，消除“先 resolve、后 Guard”窗口；per-Workspace refcount 持有到 Commit/abort 完成，但不阻止不同 target path 并发。
- Agent Execution 的冻结 Workspace snapshot + Claim acquire 必须与创建 Execution 原子提交；之后使用既有 Workspace Claim / nonterminal Execution 作为 Remove 防护。
- Remove 不等待任意长 Read；存在 WorkspaceWriteGuard、Agent Claim 或 nonterminal Execution 时返回 `WORKSPACE_IN_USE`。
- 任何新请求都必须重新通过 Resolver；不得复用另一个请求的“last active”状态。

---

## 10.9 Workspace-scoped Provider Runtime Contract

所有声明 `workspace_scoped_process` 的 Provider 共享同一组生命周期不变量，由通用 `WorkspaceCapabilityManager` 按 Provider 隔离持有；Serena/CodeGraph Adapter 不能共享一个可切项目的进程。

```rust
enum RuntimeState {
    Stopped,
    Starting,
    Ready,
    Error,
    Stopping,
}

struct CapabilityRuntimeSlot<R> {
    provider_id: WorkspaceCapabilityProviderId,
    workspace_id: WorkspaceId,
    workspace_generation: u64,
    canonical_root: PathBuf,
    state: RuntimeState,
    runtime: Option<R>,
    last_used_at: Instant,
    in_flight: usize,
    last_error: Option<SafeCapabilityError>,
}
```

Slot 可以完全不存在；注册 Workspace 不创建 Slot，Slot 存在也不代表 live process 存在。`runtime` handle、启动 Future 和 raw provider error 只属于 Manager 内部，不进入公共 DTO。

### Acquire / Single-flight

第一次调用流程：

```text
resolve WorkspaceLease
    ↓
resolve Provider Descriptor
    ↓
lookup (providerId, workspaceId, generation)
    ↓
ready Slot → acquire in-flight guard

absent/stopped/retryable error Slot
    ↓
observe readiness
    ├── ready/degraded → publish Runtime starting → start exactly one process
    ├── auto-preparable + auto_on_first_tool_call
    │       → publish readiness preparing
    │       → run allowlisted prepare
    │       → publish Runtime starting → start exactly one process
    └── explicit_only + not_prepared → PREPARATION_REQUIRED
    ↓
all concurrent callers await the same startup result
    ↓
revalidate Workspace identity
    ↓
publish ready + acquire in-flight guard
```

同一 Workspace 的并发首次调用只能启动一个进程。`in_flight` 必须在返回 runtime handle 前增加，并通过 RAII guard 在 success/error/cancel/panic 路径释放，避免 Slot 永久不可驱逐。

`prepare` 按 `(providerId, workspaceId, generation, actionId)` single-flight，Runtime startup 按 `(providerId, workspaceId, generation)` single-flight。自动准备只能执行 Descriptor 中编译期声明的 action，并受 Adapter 后置条件约束；例如 Serena 默认 Project Creation 成功后必须重新确认 canonical Root 对应的 `.serena/project.yml`，CodeGraph 因 `explicit_only` 永远不能从普通 query acquire 进入 `init/index/sync`。

如果 Provider 是否支持同一 Runtime 的并行 Tool Call 尚无真实 Contract Evidence，首版在每个 Slot 内串行化调用。不同 Workspace Slot 在 `maxInstances` 容量允许时可并行；容量不允许时走既有 LRU eviction / capability busy 契约。只有验证 Provider 并发安全后才能放宽 per-Slot concurrency，不得用一个跨 Workspace global Mutex 替代隔离。

### Preparation Activity

可能超过瞬时响应时间的 prepare/start/index 操作必须发布 Provider-Agnostic Activity：

```json
{
  "operationId": "cap-op-...",
  "workspaceId": "project-a",
  "providerId": "serena",
  "actionId": "prepare",
  "stageCode": "starting_runtime",
  "state": "running",
  "revision": 4,
  "messageCode": "CAPABILITY_STARTING_RUNTIME"
}
```

Stage/Action 的显示名来自 Descriptor，公共事件不包含命令行、PID、port、绝对 Root、Language Server 原始输出或 Provider raw error。Local UI 订阅该事件更新项目卡片；Remote MCP 请求携带标准 progress token 时可接收同一安全投影，否则调用在 bounded timeout 内等待。若准备仍未结束，则返回 `WORKSPACE_CAPABILITY_PREPARING + operationId`，后续相同 Tool Call 复用同一 operation，不重复启动。

Capability Activity 可以复用 Agent Activity 的 EventSink/序列化基础设施，但必须使用独立 scope/revision，不写入 Execution Activity History，不改变 Work/Execution control revision，也不伪装成 Agent reasoning。

`provider_prepare` 默认取得该 `(providerId, workspaceId, generation)` 的 exclusive Capability Operation Claim。首版不得让 Index/Sync/Rebuild 与同 Slot Tool Call 并发；`in_flight > 0` 时返回 busy，idle Runtime 若会占用相同 index/cache，则先 bounded stop，动作完成后只有 `warmRuntime=true` 才重新启动。Adapter 有明确并发安全证据后才能放宽。

### Capacity / Idle Eviction

每个 `workspace_scoped_process` Provider 都必须通过 Descriptor 的静态 `runtimePolicy` 接入：

```text
lazy start
max running instances
LRU eviction
idle timeout
startup deduplication
per-workspace failure isolation
```

容量按 `providerId` 独立计算；一个 Provider 的 Slot 不占用另一个 Provider 的实例上限。`starting + ready + stopping` 的 live/allocated Slot 计入容量。请求到达且容量已满时，只能在同一 Provider 内选择 `in_flight == 0` 且不处于 `starting/stopping` 的最久未使用 Slot，先完成 stop，再为新 Workspace 启动。没有可驱逐 Slot 时返回统一 capability busy error，再由 compatibility mapper 映射旧工具错误码。

Capacity 只能限制同时 live 的 Slot 数量，不能削弱 Workspace 隔离：

```text
Workspace A → Slot A
Workspace B → Slot B

禁止：A/B 共用一个可 retarget 的 Runtime/Process

capacity allows
    → A/B 可以真正并发运行

capacity full + safe idle LRU exists
    → stop idle Slot，随后启动目标 Workspace Slot

capacity full + no safely evictable Slot
    → capability busy error
```

例如 Serena `maxInstances = 1` 时，如果 A Runtime 正在 in-flight，B 请求必须返回对应 capability busy error；不得复用 A Process 并执行 `activate B`。

Idle sweeper 只停止超过 timeout 且 `in_flight == 0` 的 Slot。驱逐/空闲停止不得删除 Workspace Registry Entry 或 Provider 持久化数据。Serena/CodeGraph 的具体 `maxInstances`、`idleTimeout` 数值由 Phase 0/2A.3/2D 的 Windows 资源与负载测试冻结在各自 Descriptor；首版不新增用户配置。

### Failure / Shutdown

一个 Provider process 的启动失败、连接丢失、超时或退出只把对应 Slot 置为 `error`，并向该请求返回稳定 safe error；不得清空 Workspace、切换其他 Slot、使 Broker 退出或污染其他 Workspace Health。后续显式请求可以重新触发该 Slot 的启动，Manager 不做无界后台自动重试。

Process 一旦创建即由 Slot 持有清理责任。进入稳定 `error` 前必须确认进程已经退出或仍由该 Slot 明确持有；stop 失败必须通过 `CapabilityStopFailure` 返还原 handle，且不得丢弃 handle、释放容量后启动替代进程或留下无人管理的 orphan。该 Slot 继续计入容量，直到清理证据成立。

Host shutdown 必须停止全部 live Slot 并复用现有受管进程清理语义。Idle eviction、Workspace Remove 与 Host shutdown 都必须对同一 Slot 的 stop 操作 single-flight，禁止重复 kill 或遗留 orphan process。

### Workspace Remove Coordination

`workspace_remove(id)` 与 Runtime acquire 必须通过同一 management exclusion 线性化：

1. 临时标记该 Entry 为 removing，拒绝新的 capability acquire；
2. 若任一 Prepare/Index operation 正在 running，或 Provider Slot 正在 `starting` / `in_flight > 0`，清除 removing 并返回 `WORKSPACE_IN_USE`；
3. 对 idle `ready/error` Slot 执行 stop/dispose；若已经 `stopping`，等待同一个 bounded stop result；
4. stop 失败则保留 Registry Entry，清除 removing，返回 `WORKSPACE_CAPABILITY_STOP_FAILED`；
5. 全部 capability runtime 已释放后，才从 Registry 删除 Entry。

`removing` 是进程内短生命周期状态，不写入 `ManagerConfig`。进程异常退出后的启动恢复默认没有 removing 标记；Registry Entry 仍保留。

---

# 11. Serena 解耦

WorkspaceResolver 成功发布 Lease 只要求：

```text
Workspace registered
root exists
root is directory
canonicalization succeeds
```

不要求：

```text
Git repo
Serena installed
Serena running
.serena exists
Serena MCP connect
activate_project
```

---

## 11.1 Optional Semantic Bind

Workspace Lease 发布后，可按该 `workspaceId` 独立绑定 Semantic Capability：

```text
Workspace Lease ready
    ↓
WorkspaceCapabilityManager.acquire("serena", lease)
    ↓
stopped → starting → ready
```

Semantic failure 不回滚 Workspace。

---

## 11.2 SerenaCapabilityProvider

如果 Serena Process 内部只有一个 Current Project，则每个 live Slot 必须拥有独立的 Serena Process、Transport endpoint、Client 和受管进程 handle：

```text
WorkspaceCapabilityManager[provider=serena]
    ├── Workspace A → Serena Process A → project A only
    ├── Workspace B → Serena Process B → project B only
    └── Workspace C → stopped / no live process
```

图中的 A/B Process 只有在 Serena `maxInstances` 容量允许时才可同时 live；容量较小时，Workspace Slot identity 仍然独立。容量满则按第 10.9 节驱逐安全的 idle Slot，或在没有安全候选时返回 capability busy error，绝不复用 A Process 去 activate B。

启动时使用固定的 `serena start-mcp-server --project <canonicalRoot>`，或在该独立 Slot 中仅执行一次等价 `activate_project(canonicalRoot)`；这里的 `activate_project` 只是 Provider 内部、绑定新 Slot 的一次启动步骤，不是公共 `workspace_activate`，也不建立请求间 Workspace Context。缺少 `.serena/project.yml` 时允许使用 Serena 官方的首次按路径激活默认配置路径隐式创建。Runtime 发布 `ready` 前必须重新验证激活 Root 与 WorkspaceLease 一致；之后整个生命周期内不得再激活其他 Workspace。`find_symbol(workspaceId=A)` 只能取得 A Slot，不能把共享 Serena Process 从 B 切回 A。

不采用“单 Serena Process + 跨 Workspace global Mutex + 每次 activate/call”的目标方案。即使 Phase 0 DCR 将 Serena `maxInstances` 冻结为 `1`，也仍是 Workspace-scoped Slot + LRU/BUSY，而不是可 retarget 的全局 Process。后者虽然可以串行避免直接串线，但会造成全 Workspace 队头阻塞，并依赖尚未验证的 Serena project-specific cache/state 切换完整性。

SerenaDesktop 启动不遍历 Workspace 创建 Serena Project，也不拉起进程。第一次 Semantic Tool Call 才 lazy acquire：

```text
resolve WorkspaceLease
    ↓
Serena installed?
    ↓
.serena/project.yml exists?
    ├── yes → start/reuse Workspace Slot
    └── no  → start Slot with --project <canonicalRoot>
              → Serena default auto-creation
              → verify project configuration/root
    ↓
run original Semantic Tool
```

这个自动准备契约只覆盖 Project Configuration 与 Runtime Activation，不执行 `serena project index`，不等待完整预缓存，也不执行 Onboarding。Language Server 初始化是 Runtime startup 的组成部分，因此第一次 Tool Call 可能较慢，必须通过第 10.9 节的 single-flight、bounded timeout 和 Preparation Activity 反馈进度。

自动创建只能在 `.serena/project.yml` 不存在时发生，不得覆盖现有 Project Configuration。若配置已创建但 Runtime/LSP 启动失败，保留配置并分别报告 `project_configuration=ready`、`runtime=error`，不得回滚删除用户目录中的 Provider 工件。

Serena Index 是独立的可选优化。用户可以在 Local Desktop 点击 `[建立索引]`；Adapter 先确保 Project Configuration 已存在，再执行受管的 `serena project index <canonicalRoot>`。`index=absent/unknown/stale` 不把 Serena capability 标为 unavailable，工具仍可由 Language Server 按需获取符号；成功的完整索引用于减少首次符号调用延迟，后续由 Serena 自身维护变化。

Onboarding/Memory 构建是另一个独立阶段，只能由 Agent 工作流按 Serena contract 运行；它不属于 `prepare`、Runtime ready 或 Index ready 的后置条件。Onboarding 失败不得停止已有 Semantic Runtime。

桌面安装/版本/配置管理可以继续由 Serena supervisor 提供，但 supervisor 的“全局 running/active project”不得再作为 Semantic Tool Runtime 或 Workspace Authority。Capability Health 必须区分：

```text
Serena installation availability
Workspace A project configuration / runtime / optional index / onboarding
Workspace B project configuration / runtime / optional index / onboarding
```

---

## 11.3 Core Startup Contract

以下能力不得依赖 Serena：

```text
Desktop
Broker
Workspace Registry
Workspace Registry / Resolver
Source
Git Capability
Remote
OAuth
Agent Runtime
Work Orchestration
```

Serena Runtime Slot 全部为 stopped/absent 也属于 Core startup success。

---

# 12. Rust Source Read

公开 MCP Tool 名保持：

```text
source_read_file
source_list_dir
source_find_file
source_search_pattern
```

工具名保持兼容；V0.2 新 Schema 为每个工具增加必填 `workspaceId`。省略 ID 返回 `WORKSPACE_CONTEXT_REQUIRED`；malformed ID 返回 `INVALID_PARAMS`。不得回退到 Broker 全局状态、DesktopSelectedWorkspace、Transport Session、最近请求或 `workspace_activate` 状态。

```text
source_read_file(workspaceId, relative_path, ...)
source_list_dir(workspaceId, relative_path?, ...)
source_find_file(workspaceId, pattern, relative_path?, ...)
source_search_pattern(workspaceId, pattern, relative_path?, ...)
```

`relative_path` 是现有 Source 公共 Schema 字段名，V0.2 保持不变；本章中的 Workspace-relative 只定义其语义，不执行 API rename。

---

## 12.1 Read Snapshot

Read/Search 根据请求 `workspaceId` 捕获 WorkspaceLease 后运行。

期间如果 DesktopSelectedWorkspace 变化、其他请求访问不同 Workspace，或当前 Entry 被 Remove：

```text
T0 read starts on A generation=12
T1 UI selects B / another request uses B / A is removed
T2 read A completes
```

T2 可以返回。

结果必须携带 provenance：

```json
{
  "workspace": {
    "id": "A",
    "generation": 12
  }
}
```

Read 不因为上述并发状态变化而失败；未来对已 Remove Workspace A 的新解析返回 `WORKSPACE_NOT_FOUND`。

Write 不同，见第 13 节。

---

## 12.2 WorkspacePathResolver

Source Tool 的任何 `relative_path` 文件或目录参数都交给统一 `WorkspacePathResolver`，且只接受 Workspace-relative path。Tool 没有 `relative_path` 参数时不要求虚构一个参数。

拒绝：

```text
absolute Windows path
UNC path
Unix absolute path
Workspace root
.. escape
```

Windows 必须处理：

```text
symlink
junction
reparse point
case normalization
```

Resolver 只能从 `WorkspaceLease.canonical_root` 派生绝对路径；调用方不能提供 Root。不存在目标使用最近存在父目录进行 boundary validation。

---

## 12.3 source_read_file

返回：

```json
{
  "workspaceId": "project-x",
  "path": "src/main.rs",
  "content": "...",
  "sha256": "...",
  "sizeBytes": 12000,
  "truncated": false,
  "workspace": {
    "id": "project-x",
    "generation": 12
  }
}
```

`sha256` 基于完整原始文件 bytes，不基于截断内容。

继承当前已验证的读取预算：

```text
max_bytes default = 32 KiB
max_bytes hard max = 128 KiB
valid range        = 1..=131072
```

`content`/`text` 只返回预算内结果并设置 `truncated`；`sha256`、`sizeBytes` 始终基于完整原始文件，不因行范围或输出截断改变。客户端不能通过 `max_bytes` 放大 Server Hard Limit。

---

## 12.4 source_list_dir

要求：

- max entries；
- max depth；
- max result bytes；
- timeout；
- cancellation；
- symlink/junction 默认不递归跟随。

---

## 12.5 source_find_file

候选依赖：

```text
ignore
globset
```

语义：

- respect `.gitignore`；
- hidden 默认不搜索；
- symlink/junction 默认不跟随；
- bounded traversal；
- filename matching 可命中任何文件类型。

---

## 12.6 source_search_pattern

候选：

```text
grep-regex
grep-searcher
ignore
```

要求：

- regex compile once；
- bounded file size；
- bounded match count；
- bounded result bytes；
- timeout；
- cancel；
- stream traversal；
- 不允许先全量读取再裁剪。

Binary：

```text
NUL
OR invalid UTF-8
```

内容搜索跳过正文。

Path search 不受影响。

---

# 13. Rust Source Write Target Contract

公开工具：

```text
source_create_text_file
source_write_text_file
source_insert_lines
source_delete_lines
source_replace_lines
source_replace_content
```

若现有分支已经有其他同义命令，通过 compatibility alias 对齐。

所有 Source Write 请求必须显式携带 `workspaceId`；缺少时返回 `WORKSPACE_CONTEXT_REQUIRED`。现有 Source Write 公共 Schema 继续使用 `relative_path`，不重命名为 `path` 或其他统一字段；其文件或目录语义必须是 Workspace-relative，并经第 12.2 节统一 `WorkspacePathResolver` 以 `WorkspaceLease.canonical_root` 派生目标，不得接收 absolute/UNC/Root 参数。开始操作时，Resolver 在与 Remove 互斥的管理边界内同时返回 `WorkspaceLease + WorkspaceWriteGuard`；Guard 持有到 Commit/abort 完成，只通过 per-Workspace refcount 阻止 Remove，不互斥同 Workspace 的不同文件写入，也不复用 Agent 的持久化 `workspace_claims`。

---

## 13.1 全局写入限制

所有 Source Write 属于 bounded operation。

冻结默认 hard limits：

```text
INLINE_MUTATION_CONTENT_MAX = 1 MiB
WHOLE_FILE_WRITE_MAX        = 8 MiB
TARGET_TEXT_FILE_MAX        = 8 MiB
RESULT_TEXT_FILE_MAX        = 8 MiB
```

适用：

```text
insert_lines.content
replace_lines.content
replace_content.oldContent
replace_content.newContent
write_text_file.content
target file
result file
```

错误：

```text
SOURCE_INPUT_LIMIT_EXCEEDED
SOURCE_FILE_TOO_LARGE
```

这些是 Server Hard Limit，客户端不能通过 maxBytes 放大。

---

## 13.2 Optimistic Concurrency Control

修改已有文件必须提供：

```text
expectedSha256
```

流程：

```text
read file @ SHA=A
    ↓
prepare candidate change
    ↓
acquire TargetCommitMutex(canonical target path)
    ↓
lock 内重新验证 Workspace / path / current SHA
    ↓
current SHA == A ?
   yes → temp + atomic replace → unlock
   no  → SOURCE_VERSION_CONFLICT → unlock
```

`TargetCommitMutex` 是进程内、按 canonical target path keyed 的 Commit Mutex。由于 SerenaDesktop Host 受 Single Instance Contract 约束，所有 SerenaDesktop Source Write 都经过同一 keyed mutex：同一文件的 Commit 串行，不同文件仍可并发。Create 也必须在该锁内重新检查“不存在”，避免两个并发 create 都成功。Key entry 只在存在 holder/waiter 时保留，最后一个 guard 释放后从 map 移除，不能随历史文件路径无界增长。

现有目标的 key 使用 boundary validation 后的 canonical file identity；目标尚不存在时使用 canonical parent identity + normalized final filename。不得用未经校验的 caller path string 作为 mutex key。

该契约对 SerenaDesktop 自身管理的并发写提供严格 OCC。外部编辑器或其他进程不一定参与此锁协议，只通过锁内、atomic replace 前的最后一次 SHA/path revalidation 尽力检测；V0.2 不声明跨进程事务隔离，也不为此引入 `LockFileEx`。

---

## 13.3 Text/Binary

Source Write 只接受 UTF-8 Text。

拒绝：

```text
invalid UTF-8
contains NUL
```

返回：

```text
SOURCE_BINARY_REJECTED
```

不按扩展名猜 Binary。

---

## 13.4 Line Number

全部 1-based。

---

## 13.5 insert_lines

```text
1 <= beforeLine <= N + 1
```

`beforeLine=1`：文件首部。

`beforeLine=N+1`：append。

空 `content`：

```text
SOURCE_INVALID_ARGUMENT
```

---

## 13.6 delete_lines

```text
1 <= startLine <= endLine <= N
```

inclusive closed range。

```text
delete_lines(3,5)
```

删除：

```text
3
4
5
```

`delete_lines(1,N)` 合法，结果是空文件。

---

## 13.7 replace_lines

同样采用 inclusive closed range。

`content=""` 合法，等价于删除目标 Range。

---

## 13.8 create_text_file

目标存在：

```text
SOURCE_ALREADY_EXISTS
```

不覆盖。

空 `content` 合法，可创建空文件。

新文件默认 LF。

---

## 13.9 write_text_file

参数：

```text
ifExists = fail | overwrite
```

目标不存在：创建。

目标存在且 `fail`：

```text
SOURCE_ALREADY_EXISTS
```

目标存在且 `overwrite`：

必须提供 `expectedSha256`。

缺失：

```text
SOURCE_VERSION_REQUIRED
```

---

## 13.10 replace_content

literal match，不是 regex。

`oldContent=""`：

```text
SOURCE_INVALID_ARGUMENT
```

参数：

```text
mode = first | all
expectedMatches?
maxReplacements?
```

规则：

### mode=first

最多替换一处。

如果提供 `expectedMatches`，它表示修改前全文总匹配数。

典型：

```text
mode=first
expectedMatches=1
```

表示要求 oldContent 唯一。

### mode=all

`maxReplacements` 必填。

如果提供 `expectedMatches`：

```text
actualMatches == expectedMatches
```

必须成立。

未提供 `expectedMatches`：

只受 `maxReplacements` 限制。

找不到：

```text
SOURCE_CONTENT_NOT_FOUND
```

匹配数不符合预期：

```text
SOURCE_CONTENT_AMBIGUOUS
```

---

## 13.11 Newline

已有文件：

保留 dominant newline style。

如果 CRLF/LF 数量相同：

> 使用文件中第一个实际出现的换行符风格。

文件无换行：

```text
LF
```

插入/替换内容转换为该文件风格。

---

## 13.12 Write Commit Revalidation

Write 与 Read 的 Registry 并发语义不同。Desktop UI 选择或其他请求访问别的 Workspace 不得使当前 Write 失败。

Write Commit 必须先取得 `TargetCommitMutex(canonical target path)`，再在锁内重新验证：

```text
workspace entry still registered
same workspace id
same workspace generation
same canonical root
parent path
reparse boundary
target identity
expectedSha256
```

如果该 Workspace Entry 在操作期间被移除，或发生影响 Path Authority 的变化：

```text
WORKSPACE_CHANGED
```

不得 Commit。无关 Workspace 的注册、删除、Rename、Reorder、DesktopSelectedWorkspace 变化和其他请求的 Workspace Context 不触发 `WORKSPACE_CHANGED`。

锁只覆盖 Commit Revalidation + temp/atomic replace，不覆盖前置读取、内容变换或响应序列化。这样 `A.rs` 与 `B.rs` 可并发，`A.rs` 的两个写者则在锁内由 SHA/OCC 决定第二个是否冲突。

---

## 13.13 Crash-safe Replace

修改已有文件：

```text
same-directory temp file
    ↓
write
    ↓
flush
    ↓
sync where supported
    ↓
safe/atomic replace
```

失败清理 temp。

禁止：

```text
truncate original
    ↓
incremental write
```

---

## 13.14 Metadata

首版：

- 不主动修改 ACL；
- 不主动修改 read-only 等属性；
- 覆盖写不得无意重置基本权限；
- ADS 完整迁移留后续；
- Windows replacement 行为通过真实测试确认。

---

## 13.15 Success Result

修改已有文件成功返回：

```text
path
workspaceId
generation
beforeSha256
afterSha256
changedRange / changedCount
```

---

# 14. Remote Source Write Policy

Remote Direct Source Write：

```text
default = disabled
```

即 Remote MCP 默认不 advertise：

```text
source_create_text_file
source_write_text_file
source_insert_lines
source_delete_lines
source_replace_lines
source_replace_content
```

Remote 仍允许：

```text
Source Read(workspaceId)
Git Read(workspaceId)
CodeGraph Query(workspaceId)
Agent Execute(workspaceId)
```

ChatGPT 不知道 ID 时先通过 `workspace_list` Discovery；获得 ID 后，每个允许的 Workspace-scoped 请求都显式重复传递该 `workspaceId`，无需再次 Query。Discovery 不 Activate，也不建立 Binding。

Remote `workspaceId` 只能解析到 Local Human 已注册的 Workspace；任何 Tool-specific path 字段都必须保持其既有公共名称和 Workspace-relative 语义：Source 使用 `relative_path`，Git 使用 `path`。请求不能通过 Windows/Unix/UNC absolute path、Root 参数、`..` escape、Session 状态或 Activate 扩大 Registry 授权集合。

这符合 SerenaDesktop 的职责边界：

```text
ChatGPT → Analyze / Review
Agent   → Write / Build / Test
```

---

## 14.1 Explicit Enable

未来如果用户本机显式开启：

```text
Remote Source Write
```

必须满足：

- Local UI 明确设置；
- 风险说明；
- 完整 OCC；
- 完整 Workspace Boundary；
- 写操作 Audit；
- Remote Principal / Session 可用于连接层审计，但不参与 Workspace Routing；
- 不允许绕过 expectedSha256。

未开启：

```text
SOURCE_WRITE_REMOTE_DISABLED
```

---

# 15. Provider-Agnostic Agent Control Plane

公共 Control Plane 只理解：

```text
Work
Execution
Workspace Identity
Provider Identity
Provider Capability
Activity
Usage
Execute
Continue
Cancel
Startup Reconcile
```

不理解：

```text
Codex Thread
Codex Turn
App Server RPC
Windows Job
historyMode
Background Terminal
Codex Runtime Evidence
Codex Token Notification
```

---

# 16. ProviderDescriptor

> Human-approved binding clarification：以下 `ProviderId`、Context、Error、Outcome、Result Completeness 与 `ProviderRunResult` 定义是 P1-001 的冻结契约；它们只补全既有 Provider domain type，不引入插件 ABI。

```rust
#[serde(transparent)]
pub struct ProviderId(String);

pub struct ProviderDescriptor {
    pub id: ProviderId,
    pub display_name: String,
    pub version: Option<String>,
}

pub struct ProviderCapabilities {
    pub can_execute: bool,
    pub can_continue: bool,
    pub can_cancel: bool,
    pub can_recover: bool,
    pub activity: bool,
    pub token_usage: bool,
}
```

`ProviderId` 是 opaque identifier。首版唯一值是：

```text
codex
```

Core 不解析 Provider-specific 前缀或格式。`ProviderId` 的所有构造与反序列化入口必须执行同一 validation：

- 值非空；
- 拒绝任何 whitespace character；
- 拒绝任何 control character；
- 除此之外不增加正则、长度、字符集或其他格式约束。

`ProviderId` 的 serde wire form 是 JSON string，不是 object。

---

# 17. Object-safe AgentProvider Port

> Human-approved binding clarification / DCR（P1-005）：以下 `ProviderAcceptanceSink` 与 `execute` 参数是对 revision003 Design Freeze 的最小补充。它只补足 Provider-specific pre-turn acceptance boundary，不建立新的设计框架，也不改变 P1-001 已冻结的 `ProviderExecutionContext` 字段。

> Human-approved resolver clarification / DCR（P1-005，第二个最小 DCR）：execute / continue 继续使用 §22 的 health-gated `get()`；cancel 按 persisted `Execution.provider` 使用 registration-only `get_registered()`。这只区分“注册存在性”和“执行可用性”，其余 P1-005 冻结语义不变。

> Human-approved execution-failure clarification / DCR-3（P1-005，第三个最小 DCR）：`AgentProvider::execute` 使用 Provider 层内部 `ProviderExecutionFailure` 返回执行失败，解除 AgentTaskManager 对 Codex 私有 `ExecutionFailure` 的依赖。该 DCR 只解决前一轮 DESIGN_BLOCKER，不扩展 `ProviderError` wire contract，不建立通用错误框架，也不进入 P1-006 / P1-007 / P1-008。

> Human-approved startup-reconcile clarification / DCR-4（P1-006）：`ProviderStartupContext` 继续保持空 `{}`，不增加 result sink / callback；`ProviderReconcileSummary` 改为 Provider 层内部 typed startup report，并冻结 registration-only startup routing、失败隔离与 health 更新边界。该 DCR 不改变 P1-005 的 `ProviderError`、`ProviderExecutionFailure`、resolver、cancel 或 acceptance 契约，也不进入 P1-007 / P1-008。

> Bounded continuation-validation clarification / DCR-5（P1-008B）：`AgentProvider::validate_continuation` 使用内部、非 serde/wire 的 `ProviderContinuationContext { source_execution_id }` 和 `ProviderContinuationDecision::{Eligible, Ineligible}`。Context 不含 provider-private provenance，Adapter 自己从 StateStore 解释；decision 不向 Product / MCP 返回 provenance。该方法尚未接入 Product、Store 或 TaskManager Continue routing，不改变 `ProviderRunResult`、Claim authority 或状态机；P1-008C 才作 route cutover。

> Bounded continuation-routing clarification / P1-008C1：Store 的 core eligibility 只解释通用 lifecycle、release evidence、mode 与 execution profile；Provider provenance 只由 Adapter 验证。Continue 的 exact same-key retry（含既有 Work retry context）必须早于 Provider validation，并直接返回既有 Execution、不得再 Dispatch。无 prior 时，TaskManager 仅以 `get_registered` 读取 Provider 的 `can_continue` 与 source validation；execute/dispatch 仍使用 health-gated `get`。Provider validation 在 List/Observe 的布尔投影失败时为 `false`，在新建 Continue 时 unknown Provider/capability/Ineligible 统一为 `AGENT_CONTINUE_NOT_ALLOWED`，Provider read failure 保持稳定 Provider error code。creation re-check 的 source revision 仅防 validation-to-create TOCTOU。`thread_id` 仍参加 `execution-request-v1` canonical hash，child 仍复制 source thread；这是 P1-008C2 的剩余边界，本说明不宣告 P1-008 Gate PASS。

> Bounded continuation-identity clarification / P1-008C2A：Execution 持久化 nullable `parent_execution_id` 作为 generic Control Plane lineage identity；new continuation 写 source Execution ID，fresh 为 null。`execution-request-v1` 保持 tag 与 tuple 形状，末位语义改为 `parent_execution_id`，所以 fresh fixed bytes/hash 保持不变；`thread_id` 不参与 current canonicalization。为 pre-v6 row 保留唯一 Store-only compatibility：仅 persisted `parent_execution_id IS NULL` 且 request hash 精确匹配旧 source-thread tuple 时可 retry；不 backfill、不猜测 parent，也不影响新建、Product candidate 或 Provider validation。child `thread_id` copy 继续仅供 Codex runtime compatibility，C2B 才能改变它；P1-008 Gate 尚未闭合。

> Bounded provider-owned runtime continuation clarification / P1-008C2B：current child 不再复制 Provider thread；generic `parent_execution_id` 的 source 是 runtime authority。Codex Adapter 私有地复用 managed provenance 解析 parent source 的 thread，验证 paginated history 后 bind actual child identity，再开始 turn。parent 为 null 且 child 已有 thread 的路径仅兼容 pre-C2 persisted / previously-bound row；不回填 parent、不将 thread/turn/runtime/historyMode 放入 Provider Port，P1-008 Gate 尚未闭合。

Registry 目标是：

```rust
Arc<dyn AgentProvider>
```

因此 V0.2 不使用可能非 object-safe 的裸 `async fn` trait 作为冻结契约。

定义：

```rust
pub type ProviderFuture<'a, T> =
    Pin<Box<dyn Future<Output = T> + Send + 'a>>;
```

P1-001 冻结 Port 使用的输入与 `ProviderError` domain type；P1-005 DCR-3 在同一 Provider 层增加内部 `ProviderExecutionFailure`：

```rust
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderExecutionContext {
    pub execution_id: String,
}

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderCancelContext {
    pub execution_id: String,
}

#[serde(deny_unknown_fields)]
pub struct ProviderStartupContext {}

pub struct ProviderReconcileSummary {
    pub items: Vec<ProviderReconcileItem>,
}

pub struct ProviderReconcileItem {
    pub subject_id: String,
    pub kind: ProviderReconcileKind,
}

pub struct ProviderContinuationContext {
    pub source_execution_id: String,
}

pub enum ProviderContinuationDecision {
    Eligible,
    Ineligible,
}

pub enum ProviderReconcileKind {
    OrphanResourceRecovered,
    OrphanResourceUnknown,
    ExecutionReleased,
    ExecutionInconsistent,
    ExecutionPendingExplicitResume,
    ExecutionUnknown,
    ExecutionProviderFailure,
    ExecutionInterrupted,
}

#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderErrorCode {
    AgentProviderNotFound,
    AgentProviderUnavailable,
    AgentProviderCapabilityUnsupported,
    AgentProviderContractError,
    AgentProviderOperationFailed,
}

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderError {
    pub code: ProviderErrorCode,
}

pub enum ProviderExecutionFailure {
    State(String),
    Runtime { code: String, message: String },
}
```

`ProviderExecutionContext` 与 `ProviderCancelContext` 只包含 `executionId`。它们不携带 `workspaceId`、`canonicalRoot`、thread、turn、job、`historyMode` 或 Runtime identity；Provider 必须通过权威 StateStore / Execution identity 读取已经冻结的执行上下文，禁止建立第二套 Authority。

`ProviderStartupContext` 首版继续是序列化为 `{}` 的空 struct。Provider 实例持有自己的 Store / Runtime 依赖，不新增 Host Context、result sink 或 callback。

`ProviderReconcileSummary`、`ProviderReconcileItem` 与 `ProviderReconcileKind` 是 Provider 层内部、provider-agnostic 的 typed startup report；不实现 serde，不是 Product / MCP / wire DTO。`subject_id` 是只供 Host startup report / logging 使用的 opaque subject identity，Core 不解析其格式或前缀。上述类型不得携带 Runtime handle / id、thread、turn、job、`historyMode`、Claim、release / termination evidence、raw provider error、`ExecutionRecord` 或 Provider private object。

`ProviderErrorCode` 的 serde wire form 精确为 §50 Provider 已冻结的五个 string：

```text
AGENT_PROVIDER_NOT_FOUND
AGENT_PROVIDER_UNAVAILABLE
AGENT_PROVIDER_CAPABILITY_UNSUPPORTED
AGENT_PROVIDER_CONTRACT_ERROR
AGENT_PROVIDER_OPERATION_FAILED
```

`ProviderError` 继续精确且仅包含 `code`，五个稳定码及其 wire form 不变；禁止扩展 payload，禁止加入 raw provider message、command、stdout、stderr、Runtime、thread、turn 或 job 字段。

`ProviderExecutionFailure` 仅是 Provider 层内部执行失败类型，不实现 serde，不是 wire / Product DTO，也不进入 MCP schema。`State(String)` 保留现有执行状态与安全诊断字符串语义；`Runtime { code, message }` 只携带安全诊断 code 与 message，禁止携带 Runtime handle / id、thread、turn、job、`historyMode`、Claim 或 evidence。

```rust
pub trait ProviderAcceptanceSink: Send + Sync {
    fn accepted(&self);
}
```

`ProviderAcceptanceSink` 是 provider-agnostic、one-shot 控制握手，只表达“本次已创建的 Execution 已通过 Provider-specific pre-turn acceptance boundary”。Host 实现负责 one-shot / at-most-once；重复调用不得产生第二次 acceptance，sink 内部 receiver 被丢弃或 Provider 丢弃 sink 均不得改变 Provider lifecycle。它不是 Activity、Usage、terminal、recovery 或 evidence，也没有 `rejected` / `error` 方法；acceptance 前的失败继续通过 `execute` 的现有 `Result` 返回。`accepted()` 不携带 payload，尤其不携带 workspace、thread、turn、job、`historyMode`、Runtime 或 evidence。

```rust
pub trait AgentProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;

    fn capabilities(&self) -> ProviderCapabilities;

    fn execute<'a>(
        &'a self,
        context: ProviderExecutionContext,
        acceptance: Arc<dyn ProviderAcceptanceSink>,
        telemetry: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>>;

    fn cancel<'a>(
        &'a self,
        context: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>>;

    fn validate_continuation<'a>(
        &'a self,
        context: ProviderContinuationContext,
    ) -> ProviderFuture<'a, Result<ProviderContinuationDecision, ProviderError>>;

    fn startup_reconcile<'a>(
        &'a self,
        context: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>>;
}
```

该扩展保持 object-safe、boxed future，且不强制引入 `async-trait` crate。

只有 `execute` 使用 `ProviderExecutionFailure`；`cancel`、`validate_continuation`、`startup_reconcile` 与 `ProviderRegistry` resolver 继续使用 `ProviderError`。`validate_continuation` 是 Provider-owned source Execution provenance check：普通不满足返回 `Ineligible`，source 缺失或 Store read 失败投影为既有五码之一，且不得携带 raw provider detail。它不是 Product / MCP wire，不改变 `ProviderRunResult`、Claim authority 或状态机，也不在 P1-008B 接入 Continue routing。Codex Adapter 负责把既有 `codex::provider::ExecutionFailure::State(value)` 映射为 `ProviderExecutionFailure::State(value)`，把 `ExecutionFailure::Runtime(failure)` 映射为 `ProviderExecutionFailure::Runtime { code: failure.code, message: failure.message }`。`failure` 中的真实 Runtime owner / identity 与 quarantine ownership 必须继续留在 `CodexRuntimePool` / Codex Adapter 内，跨 Provider Port 只传安全诊断 code / message。

AgentTaskManager 只消费 `ProviderExecutionFailure`，不再依赖或匹配 `codex::provider::ExecutionFailure`，也不得解释 Codex Runtime / thread / turn / `historyMode`。该边界调整不得改变任何既有 observable semantics：`AGENT_RUNTIME_QUARANTINED` 的 Product 投影不变；Recovery 的 Runtime / State failure 分类语义不变；Execution / Claim 的 pending、Unknown 与 finalize authority 不变。

Acceptance 顺序与失败语义冻结如下：

- Host 先完成 durable Execution create、现有 guard / admission 与 `ProviderRegistry` resolve；这些既有 authority 不变。
- Codex Start 在 Codex availability / quarantine / pre-dispatch checks 成功后、实际 `turn/start` 之前调用 `accepted()`，保持当前可观察 acceptance 时点，不得因 Registry routing 延迟到 terminal。Codex Adapter 可以在其现有 pre-execute 边界触发 sink，公共层无需读取 Codex 私有状态。
- Codex Continue 仅在 managed Thread resume、返回 Thread identity 精确校验、`historyMode=Paginated` 校验与 Execution bind 全部成功后，并且在 `turn/start` 之前调用 `accepted()`。
- `execute` 在 acceptance 前返回 `Err` 时，Host 将其投影为 rejection。若 `execute` 在没有 acceptance 的情况下异常结束或返回 terminal success，Host 不得虚构 acceptance，必须走现有稳定错误 / contract error 路径。
- `accepted()` 不写 DB，不改变 `dispatch_state` 或 Execution status；Store `dispatch_state` 也不能等价替代 acceptance。它不授权 Claim release，不代表 `providerInvoked`、dispatched 或 terminal，也不进入 Activity Revision。

公共 Control Plane 仍不得解释 Codex Thread / Turn / `historyMode` / Runtime；Provider 内部决定何时触发 sink。§18 与 §20 的 safety authority 不变：Provider 返回 completed 仍不能 release Claim，finalize 仍须重读权威 evidence。

DCR-3 回归要求冻结如下：

- provider / control tests 继续验证 `ProviderError` 的 `{ code }` wire 不变，并验证 `ProviderExecutionFailure` 不可 serde / wire；
- Recovery `explicit_resume_binary_failure_keeps_pending_and_claim` 必须恢复通过并保持 Runtime failure 分类；
- Runtime quarantine Product regression 必须恢复 `AGENT_RUNTIME_QUARANTINED`；
- 现有 P1-005 routing / acceptance / cancel tests 必须全部继续通过。

---

# 18. ProviderRunResult

```rust
#[serde(rename_all = "snake_case")]
pub enum ProviderOutcome {
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

#[serde(rename_all = "snake_case")]
pub enum ProviderResultCompleteness {
    Unknown,
    Partial,
    Complete,
}

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderRunResult {
    pub execution_id: String,
    pub outcome: ProviderOutcome,
    pub result: Option<serde_json::Value>,
    pub result_completeness: ProviderResultCompleteness,
    pub diagnostic_code: Option<String>,
}
```

`ProviderOutcome` 的 serde wire form 是 `completed` / `failed` / `cancelled` / `interrupted`。这是 Provider outcome，不是 Workspace Claim release authorization。

`ProviderResultCompleteness` 的 serde wire form 是 `unknown` / `partial` / `complete`，与现有 Execution result completeness 语义一致；P1-001 不修改现有 Execution 类型。

`ProviderRunResult` 精确且仅允许：

```text
executionId
outcome
result
resultCompleteness
diagnosticCode
```

禁止：

```text
safe=true
safeToReleaseWorkspace
releaseEvidence
jobEmpty
runtimeTerminated
cleanupComplete
thread / turn / job
historyMode
Runtime identity
```

`deny_unknown_fields` 必须使上述伪造字段反序列化失败。ProviderRunResult 不能授权 Claim Release；finalization 仍必须重新读取权威 StateStore / Runtime evidence。

---

# 19. Recovery Ownership

启动恢复：

```text
Desktop
   ↓
ProviderRegistry
   ↓
startupReconcile(providerId)
```

Codex：

```text
Codex Adapter
    ↓
Codex Store Adapter
    ↓
runtime / claim scan
    ↓
thread / turn / job
    ↓
historyMode
    ↓
unknown / inconsistent
    ↓
evidence
    ↓
StateStore lifecycle transition
```

公共 Control Plane 不解释 Codex 私有状态。

DCR-4 冻结 startup routing：Registry 枚举全部已注册 Provider，并使用 registration-only `get_registered()` 取得 Provider；不得使用 health-gated `get()` 跳过历史安全恢复。只有 `capabilities().can_recover == true` 的 Provider 执行 `startup_reconcile(ProviderStartupContext {})`；Codex 在 P1-006 完成后必须声明 `can_recover = true`。这不改变 P1-005：execute / continue 仍使用 `get()`，cancel 仍按 §22 的既有 `get_registered()` + capability 语义执行。

Codex 可以继续在内部产生现有 `Vec<RecoveryOutcome>`，但必须在 Codex Adapter 边界按原 recovery report 的逐项顺序投影为：

| Codex private `RecoveryOutcome` | `subject_id` 来源 | Provider report `kind` |
|---|---|---|
| `OrphanRuntime { failure: None, .. }` | `runtime_id` | `OrphanResourceRecovered` |
| `OrphanRuntime { failure: Some(_), .. }` | `runtime_id` | `OrphanResourceUnknown` |
| `Released` | `execution_id` | `ExecutionReleased` |
| `Inconsistent` | `execution_id` | `ExecutionInconsistent` |
| `PendingExplicitResume` | `execution_id` | `ExecutionPendingExplicitResume` |
| `Unknown` | `execution_id` | `ExecutionUnknown` |
| `RuntimeFailure` | `execution_id` | `ExecutionProviderFailure` |
| `Interrupted` | `execution.id` | `ExecutionInterrupted` |

`RecoveryOutcome` 的私有 evidence、raw failure、`ExecutionRecord` 与 Runtime owner 继续留在 Codex recovery / `CodexRuntimePool`，不得跨 Provider Port。Host 复用现有 startup reporting / logging，按 summary 原顺序仅匹配 `ProviderReconcileKind` 与 `subject_id`；公共层不得解释 Codex private runtime / thread / turn / job / `historyMode` / evidence。

单个 Provider reconcile 返回 `ProviderError` 时，该 Provider 的 durable Claim / Unknown 必须保持 fail-closed，不得释放任何未证明安全的资源。Registry 将该已注册 Provider 的 health 更新为 `Unavailable`，使后续 execute / continue 继续被 health-gated `get()` 拒绝；其他已注册且 `can_recover` 的 Provider 继续 reconcile，不回滚已完成结果。首版不增加插件框架、自动重试、health reason 或 time metadata。

---

# 20. Finalize

`finalize_and_release_execution()` 必须重新读取：

```text
Execution
Claim
Runtime ownership
Provider terminal evidence
cleanup evidence
termination evidence
```

满足既有 ReleaseBasis 后，才在同一个 SQLite Transaction 提交：

```text
terminal
+
Claim release
```

Provider 返回 completed 不得绕过上述校验。

---

# 21. Provider-Opaque Compatibility Fields

V0.2 暂保留：

```text
threadId
threadName
turnId
```

定义：

> backward-compatible provider-opaque fields

只允许：

```text
serialize
display
diagnostic
```

禁止用于：

```text
control decisions
capability decisions
controlRevision hash
public recovery decisions
```

后续 API v2 才移除。

---

## 21.1 providerSessionLabel

新增：

```text
providerSessionLabel?
```

Codex 可以把安全的 Thread Title 映射到该字段。

新 UI 优先使用它。

---

# 22. ProviderRegistry

首版：

```text
ProviderRegistry
    └── codex
```

提供：

```text
get
get_registered
set_health (internal only)
listDescriptors
capabilities
health
startupReconcile
```

Resolver 契约冻结为：

```rust
pub fn get(&self, id: &ProviderId) -> Result<Arc<dyn AgentProvider>, ProviderError>;
pub fn get_registered(&self, id: &ProviderId) -> Result<Arc<dyn AgentProvider>, ProviderError>;
```

- `get()` 是 execute / continue 等需要执行可用性的 health-gated resolver：unknown 返回 `AGENT_PROVIDER_NOT_FOUND`；`ProviderHealth::Unavailable` 返回 `AGENT_PROVIDER_UNAVAILABLE`。
- `get_registered()` 只判断 Provider 是否已注册，不检查 `ProviderHealth`：unknown 返回 `AGENT_PROVIDER_NOT_FOUND`；已注册时即使 `Unavailable` 也返回 Provider handle，且不得改变或伪造 health。
- P1-005 cancel 必须从 persisted `Execution.provider` 构造 `ProviderId`，经 `get_registered()` 取得 Provider，随后检查 `capabilities().can_cancel`；`false` 返回 `AGENT_PROVIDER_CAPABILITY_UNSUPPORTED`，`true` 调用 `provider.cancel()`。Provider / CLI health 不作为 cancel 的前置门禁。
- execute / continue 仍经 `get()` 路由；`Unavailable` 时不得启动或继续 Provider work。
- P1-006 startup reconcile 是 registration-only resolver 的另一个明确授权调用方：即使 Provider 当前为 `Unavailable`，仍须经 `get_registered()` 执行其历史安全恢复；只有 `can_recover == true` 时调用 `startup_reconcile`。
- P1-006 只允许增加一个 Registry 内部 health update API（`set_health` 或等价命名）。它必须先校验 Provider 已注册；unknown 继续返回 `AGENT_PROVIDER_NOT_FOUND`。reconcile failure 仅把对应 Provider 更新为 `Unavailable`，不增加错误码、状态机、health reason 或 time metadata。

`get_registered()` 仅供已冻结的 P1-005 cancel 与 P1-006 startup reconcile 边界使用，不是通用 bypass；不新增状态、错误码、Store 字段、fallback、Codex 特判或 Plugin ABI。

Codex CLI 缺失：

```text
codex.health = unavailable
Desktop still starts
```

execute / continue 调用返回：

```text
AGENT_PROVIDER_UNAVAILABLE
```

---

# 23. AgentEventSink 与 Identity Binding

正确链路：

```text
Codex notification
      ↓
Codex Adapter
      ↓
validate Runtime identity
validate Thread identity
validate Turn identity
validate Execution binding
      ↓
map to Provider-Agnostic Event
      ↓
AgentEventSink
      ↓
TelemetryProjector
```

> Thread/Turn Identity Binding 必须在 Codex Adapter publish 之前完成。

TelemetryProjector 永远不理解 Thread / Turn。

---

## 23.1 Event Sink

```rust
pub enum AgentTelemetryEvent {
    Activity(AgentActivityEvent),
    Usage(UsageEvent),
}
```

闭集只有：

```text
Activity
Usage
```

禁止：

```text
TerminalEvidence
RuntimeEvidence
ClaimRelease
CleanupEvidence
JobEvidence
RecoveryEvidence
```

---

# 24. Activity summaryCode 确定性派生

定义唯一纯函数：

```text
derive_summary_code(
    progressPhase,
    activityPhase,
    toolCategory
)
```

优先级固定：

```text
1. ProgressPhase::Finalizing
      → execution.finalizing

2. ProgressPhase::Reconciling
      → execution.reconciling

3. ActivityPhase::Provider + ToolCategory=None
      → provider.processing

4. ActivityPhase::Tool + ToolCategory::Read
      → tool.read

5. ActivityPhase::Tool + ToolCategory::Edit
      → tool.edit

6. ActivityPhase::Tool + ToolCategory::Command
      → tool.command

7. ActivityPhase::Tool + ToolCategory::Build
      → tool.build

8. ActivityPhase::Tool + ToolCategory::Test
      → tool.test

9. ActivityPhase::Tool + ToolCategory::Tool
      → tool.other

10. 无 Activity
      → null
```

非法组合：

```text
ActivityPhase=Provider + ToolCategory!=None
ActivityPhase=Tool + ToolCategory=None
```

返回：

```text
AGENT_ACTIVITY_CONTRACT_ERROR
```

不猜测。

---

## 24.1 Finalizing / Reconciling

ProgressPhase 覆盖 Activity Mapping。

因此：

```text
running/tool.test
    ↓
finalizing
```

必须得到：

```text
execution.finalizing
```

并改变 Activity Revision。

同一 Finalizing heartbeat 不改变 Activity Revision。

---

# 25. Activity Revision v2

算法域：

```text
agent-activity-v2
```

输入：

```text
executionId
activityPhase
toolCategory
summaryCode
```

明确排除：

```text
lastActivityAt
activityAgeMs
silenceLevel
heartbeat
updatedAt
Usage
```

---

# 26. Activity 与 Lifecycle CAS 解耦

`executions.revision`：

```text
Execution Lifecycle CAS
```

Activity-only update：

> **不得 increment `executions.revision`。**

新增：

```sql
ALTER TABLE executions
ADD COLUMN activity_summary_code TEXT;

ALTER TABLE executions
ADD COLUMN activity_sequence INTEGER NOT NULL DEFAULT 0
CHECK(activity_sequence >= 0);
```

语义：

```text
semantic activity change
    → activity_sequence += 1

same semantic heartbeat
    → only last_activity_at refresh
```

---

# 27. Activity Current Snapshot / History

Current Activity Authority：

```text
executions
```

Current fields：

```text
activity_phase
tool_category
activity_summary_code
activity_sequence
last_activity_at
```

---

## 27.1 Activity History DDL

```sql
CREATE TABLE execution_activity_events (
    execution_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,

    activity_phase TEXT,
    tool_category TEXT,
    summary_code TEXT NOT NULL,

    activity_revision TEXT NOT NULL,
    observed_at INTEGER NOT NULL,

    PRIMARY KEY(execution_id, sequence),

    FOREIGN KEY(execution_id)
        REFERENCES executions(id)
        ON DELETE RESTRICT
);
```

只有 semantic Activity change append history。

Heartbeat 不 insert。

History 只用于：

```text
UI
diagnosis
```

不参与：

```text
Runtime Evidence
Claim Evidence
Lifecycle CAS
```

V0.2 不执行 Activity History 自动 prune，也不新增 retention job、滚动删除或 CASCADE。Activity History 查询必须使用 Server-enforced page limit + cursor，禁止无界读取；Execution delete/retention 与相应从表清理留到真正引入删除生命周期的后续版本。

---

# 28. Agent Observe V0.2

公开 `agent_query observe`：

```json
{
  "action": "observe",
  "executionId": "...",
  "knownRevision": "...",
  "knownControlRevision": "...",
  "knownActivityRevision": "...",
  "wakeOn": "activity",
  "waitMs": 15000,
  "includeResult": false
}
```

---

## 28.1 Revision

`knownRevision`：

```text
legacy alias of knownControlRevision
```

两者同时提供：

```text
knownControlRevision wins
```

---

## 28.2 Validation

公共 Work Adapter 直接继承当前已发布范围：

```text
waitMs default = 15000
waitMs range   = 0..=20000
```

`waitMs=0` 是合法的即时 Snapshot。内部 Product Observe 可以支持更大的实现上限，但不得扩大公共 MCP Schema；公共值超过 `20000` 返回 `AGENT_OBSERVE_INVALID_ARGUMENT`。

以下属于 invalid：

```text
knownRevision=""
knownControlRevision=""
knownActivityRevision=""
unknown wakeOn
waitMs out of range
```

返回：

```text
AGENT_OBSERVE_INVALID_ARGUMENT
```

---

## 28.3 wakeOn=control

唤醒：

```text
control change
terminal
requested result
timeout
```

---

## 28.4 wakeOn=activity

唤醒：

```text
control change
activity change
terminal
requested result
timeout
```

Activity Mode 不忽略 Control Change。

---

## 28.5 Initial Mismatch

客户端：

```text
knownActivityRevision=A1
```

服务端：

```text
A2
```

第一次读取立即返回：

```text
wakeReason=initial_mismatch
mismatchKind=activity
```

不得等待 A3。

---

## 28.6 wakeReason

固定：

```text
initial_mismatch
control
activity
terminal
result
timeout
```

---

## 28.7 unchanged

兼容字段 `unchanged` 只表示：

> known control revision 是否等于当前 controlRevision。

它不描述 Activity。

MCP Tool Description 必须明确：

> `wakeOn=activity` 时客户端必须使用 `wakeReason` 和 `activityRevision`，不得只看 `unchanged`。

---

## 28.8 v1 → v2

Activity Revision 是 opaque token。

旧 v1 token 到 V0.2：

```text
mismatch
    ↓
immediate v2 snapshot
```

不迁移旧 hash。

---

## 28.9 Snapshot Semantics

Observe 返回唤醒或读取时刻的最新 Execution Snapshot，不是逐事件消息流。多个 control/activity change 可能在一次轮询窗口内被合并，客户端允许从 revision N 直接看到 N+K，不能假设每个中间 revision 都会单独返回。

`known*Revision` 只用于 freshness/mismatch 判断；需要诊断历史时使用第 27 节的 bounded Activity History 查询，不得通过重复 `waitMs=0` 推导完整事件序列。

---

# 29. Activity 隐私边界

允许：

```text
正在处理任务
正在读取代码
正在修改文件
正在执行命令
正在构建项目
正在运行测试
正在整理结果
```

禁止：

```text
Chain-of-Thought
raw reasoning
完整 command line
argv
stdout
stderr
environment
token/credential
Prompt
diff
source content
```

---

# 30. Provider-Agnostic Usage Model

Usage 是 Telemetry，不是 Lifecycle Authority。

---

## 30.1 Usage DTO

```rust
pub struct UsageSnapshot {
    pub provider_id: ProviderId,
    pub execution_id: ExecutionId,

    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub cache_write_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub total_tokens: Option<i64>,

    pub model_context_window: Option<i64>,

    pub completeness: UsageCompleteness,
    pub revision: u64,
    pub updated_at: i64,
}
```

---

## 30.2 null 与 0

```text
null = unknown / unsupported / not yet observed
0    = known real zero
```

不得互换。

`total_tokens` 只能使用 Provider 明确提供、且语义已在 Provider Contract 中冻结的总量；不得在公共层通过 `input_tokens + output_tokens + cached_input_tokens + reasoning_tokens + ...` 自行相加推导。Breakdown 字段可能重叠、属于父子计数或采用 Provider-specific 口径，全部为 null 时也不能伪造 total=0。

---

## 30.3 UsageCompleteness

```text
unknown
partial
complete
```

### unknown

没有足够信息形成可信 Execution Usage。

### partial

有可信数字，但无法证明覆盖当前 Turn terminal boundary。

### complete

Provider Contract 能证明 snapshot 覆盖 terminal boundary。

---

# 31. Codex Usage Contract

Codex Usage Wire Schema 属于：

> Pinned Codex App Server Contract

Phase 0 必须记录：

```text
binary version
binary hash
schema
event fields
integer semantics
ordering
terminal ordering
sync checkpoint capability
```

不能假设所有未来 Codex 都一样。

---

## 31.1 Token JSON

只接受：

```text
integer
>= 0
<= i64::MAX
```

拒绝：

```text
float
negative
string
overflow
silent cast
clamp
round
```

错误：

```text
USAGE_EVENT_INVALID
```

---

# 32. Nullable Baseline Delta

Provider Contract 必须声明：

- 哪些 cumulative fields 属于 Execution Delta Snapshot 的 required participating fields；
- 哪些是 optional unsupported fields。

对于 participating fields：

> baseline snapshot 必须原子有效。

例如：

```text
baseline.input = 40
baseline.output = null
current.input = 100
current.output = 50
```

如果 output 是 participating field：

```text
整个 baseline = unknown
```

不得产生：

```text
input=60
output=null
```

这种半截 delta。

---

## 32.1 Delta Rule

只有：

```text
all participating baseline fields known
AND
all participating current fields known
```

才计算 delta。

否则：

```text
all execution delta participating counters = null
completeness = unknown
```

Provider 不支持的 optional breakdown 可以永久为 null，不影响其他 participating fields。

---

# 33. Continue Baseline

新 Turn Side-Effect Boundary 前：

优先：

```text
query provider cumulative checkpoint
    ↓
persist baseline
    ↓
start new turn
```

若没有同步 checkpoint：

只有同时满足：

```text
checkpoint.lastExecutionId == sourceExecutionId
source Usage completeness == complete
checkpoint covers source terminal boundary
same provider lineage
```

才能复用。

否则：

```text
baseline unknown
```

禁止：

```text
old checkpoint hard subtract
current - 0
```

---

# 34. Usage Terminal Coverage

Execution terminal != Usage complete。

只有：

### A. Terminal 后同步 checkpoint

或：

### B. Pinned Contract 明确证明 final Usage notification 覆盖 TurnCompleted

才能：

```text
complete
```

否则：

```text
Execution completed
Usage partial
```

合法。

---

# 35. Usage Telemetry Lifecycle

Usage Scope 状态：

```text
accepting
    ↓
terminal_grace
    ↓
frozen
```

冻结：

```text
USAGE_TERMINAL_GRACE_MS = 2000
```

---

## 35.1 Terminal

Execution terminal + Claim release 按现有 Runtime Foundation 立即提交。

Usage Scope：

```text
accepting
→ terminal_grace
```

二者不耦合。

---

## 35.2 Late Usage

terminal grace 内，满足：

```text
exact Execution
exact Provider
exact Runtime
exact Provider-private Thread/Turn
```

可以更新 Usage。

只能更新 Telemetry，不能修改：

```text
Execution terminal
Provider terminal
Claim
Release Evidence
```

---

## 35.3 Freeze Trigger

以下任一发生：

```text
Runtime teardown completed
OR
terminal grace expired
OR
authoritative final checkpoint persisted
```

则：

```text
telemetry_state = frozen
```

之后 late event：

```text
drop
USAGE_TELEMETRY_FROZEN
```

---

# 36. Usage Revision

`usageRevision` 只表示 Telemetry Freshness。

不属于：

```text
Execution CAS
Runtime CAS
Control Revision
Activity Revision
```

---

## 36.1 Duplicate

相同 cumulative snapshot 且 completeness/coverage 没增强：

```text
no-op
```

---

## 36.2 Regression

incoming cumulative < latest：

```text
USAGE_COUNTER_REGRESSION
```

整 event 不写入。

---

# 37. Usage 数据库

公共表：

```sql
CREATE TABLE execution_usage (
    execution_id TEXT PRIMARY KEY NOT NULL,
    provider_id TEXT NOT NULL,

    input_tokens INTEGER
        CHECK(input_tokens IS NULL OR input_tokens >= 0),

    cached_input_tokens INTEGER
        CHECK(cached_input_tokens IS NULL OR cached_input_tokens >= 0),

    cache_write_input_tokens INTEGER
        CHECK(cache_write_input_tokens IS NULL OR cache_write_input_tokens >= 0),

    output_tokens INTEGER
        CHECK(output_tokens IS NULL OR output_tokens >= 0),

    reasoning_tokens INTEGER
        CHECK(reasoning_tokens IS NULL OR reasoning_tokens >= 0),

    total_tokens INTEGER
        CHECK(total_tokens IS NULL OR total_tokens >= 0),

    model_context_window INTEGER
        CHECK(model_context_window IS NULL OR model_context_window >= 0),

    completeness TEXT NOT NULL
        CHECK(completeness IN ('unknown','partial','complete')),

    usage_revision INTEGER NOT NULL DEFAULT 0
        CHECK(usage_revision >= 0),

    updated_at INTEGER NOT NULL,

    FOREIGN KEY(execution_id)
        REFERENCES executions(id)
        ON DELETE RESTRICT
);
```

无 Usage 行：

```text
all counters=null
completeness=unknown
```

---

# 38. Codex Private Usage Schema

```sql
CREATE TABLE codex_thread_usage_checkpoints (
    runtime_instance_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,

    last_execution_id TEXT NOT NULL,
    cumulative_json TEXT NOT NULL,

    covers_terminal_boundary INTEGER NOT NULL
        CHECK(covers_terminal_boundary IN (0,1)),

    captured_at INTEGER NOT NULL,

    PRIMARY KEY(runtime_instance_id, thread_id)
);
```

```sql
CREATE TABLE codex_execution_usage_state (
    execution_id TEXT PRIMARY KEY NOT NULL,

    runtime_instance_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    turn_id TEXT,

    baseline_json TEXT,
    latest_cumulative_json TEXT,

    telemetry_state TEXT NOT NULL
        CHECK(telemetry_state IN (
            'accepting',
            'terminal_grace',
            'frozen'
        )),

    terminal_at INTEGER,
    freeze_at INTEGER,
    last_event_at INTEGER,

    FOREIGN KEY(execution_id)
        REFERENCES executions(id)
        ON DELETE RESTRICT
);
```

公共 Control Plane 不读取这些 Provider-private identity 字段。

---

# 39. Product DTO

Execution Product 增加：

```text
provider: {
  id,
  displayName,
  version?
}

providerSessionLabel?

progress: {
  phase,
  activityPhase?,
  toolCategory?,
  summaryCode?,
  summary?,
  activityRevision,
  lastActivityAt?,
  activityAgeMs?,
  silenceLevel?
}

usage: {
  inputTokens?,
  cachedInputTokens?,
  cacheWriteInputTokens?,
  outputTokens?,
  reasoningTokens?,
  totalTokens?,
  modelContextWindow?,
  completeness,
  usageRevision,
  updatedAt?
}
```

兼容：

```text
threadId?
threadName?
turnId?
```

---

# 40. Agent UI

## 40.1 Task Detail

新增 Token Usage：

```text
Total Tokens

Input
Cached Input
Cache Write
Output
Reasoning

Completeness
```

null：

```text
—
```

真实 0：

```text
0
```

partial：

```text
12,531 · 统计不完整
```

UI 不把 Breakdown 相加得到 Total。

---

## 40.2 Left Task Hover

```text
任务标题
执行中 · Codex
总 Token：12,531
```

partial：

```text
总 Token：12,531 · 统计不完整
```

unknown：

```text
总 Token：—
```

列表 Summary 直接返回：

```text
usageTotalTokens
usageCompleteness
providerId
```

禁止 Hover N+1 请求详情。

---

# 41. Capability Health

Capability availability 保持：

```text
ready
unavailable
error
```

其中 `ready/degraded + runtimeState=stopped` 表示持久化准备已满足、首次调用可以 lazy start；`not_prepared` 表示 Provider 需要自动或显式准备，不表示 Workspace 不可用。Workspace Runtime 状态统一为：

```text
stopped
starting
ready
error
stopping
```

`unavailable` 是能力可用性，不是 Runtime lifecycle state。例如 Serena/CodeGraph binary 缺失时 capability unavailable；Serena Project Configuration 缺失但允许自动准备时是 `not_prepared`，CodeGraph index 缺失时是 `not_prepared + explicit action`，已准备但尚未使用的 Runtime 是 stopped。

三个维度正交：`status` 表示 Provider 能力是否可用，`readiness` 表示该 Workspace 的持久化准备情况，`runtimeState` 表示当前进程生命周期；UI 不得把三者压回一个红/绿布尔值。

Health 必须按 Workspace 投影，并根据 `WorkspaceCapabilityRegistry` 动态生成 Provider Map，不能只返回一个全局 Serena/CodeGraph 状态或在 UI 写死两个 Provider。示例：

```json
{
  "workspaceId": "project-b",
  "coreCapabilities": {
    "source": { "status": "ready" },
    "git": { "status": "unavailable" },
    "agent": { "status": "ready" }
  },
  "providers": {
    "codegraph": {
      "displayName": "CodeGraph",
      "installation": "installed",
      "readiness": "ready",
      "status": "ready",
      "runtimeState": "ready",
      "stages": [
        { "id": "index", "state": "ready", "requirement": "required" }
      ],
      "actions": [
        { "id": "update_index", "displayName": "更新索引", "authority": "local_human", "execution": "provider_prepare" }
      ]
    },
    "serena": {
      "displayName": "Serena",
      "installation": "installed",
      "readiness": "not_prepared",
      "status": "ready",
      "runtimeState": "stopped",
      "stages": [
        { "id": "project_configuration", "state": "absent", "requirement": "auto_preparable" },
        { "id": "index", "state": "unknown", "requirement": "optional" },
        { "id": "onboarding", "state": "unknown", "requirement": "optional" }
      ],
      "actions": [
        { "id": "prepare", "displayName": "准备", "authority": "local_human", "execution": "manager_ensure_runtime" },
        { "id": "build_index", "displayName": "建立索引", "authority": "local_human", "execution": "provider_prepare" }
      ]
    }
  }
}
```

公共 DTO 不暴露 PID、port、Client handle、raw process error、命令行或本地绝对 Root。`lastUsedAt` 与 `inFlight` 默认只用于 Manager 调度和诊断，不作为远程公共契约。

任一 Provider unavailable 或某个 Workspace Slot error 不得把整个 Desktop 标红为启动失败，也不得覆盖其他 Provider/Workspace 的 Health。新增 Provider 的展示名、安装状态、Readiness、Stage、Action 和 Runtime state 来自 Descriptor/统一 DTO，不增加 UI 特判。

---

# 42. Serena 与 CodeGraph 新定位

Serena 只承担可选 Semantic Capability：

```text
symbols overview
find symbol
find references
```

不再承担：

```text
Workspace authority
Filesystem authority
Application startup dependency
```

Serena Project Configuration、Runtime、optional Index 和 Onboarding 是该 Capability 的内部阶段，不再反向控制 Project 是否能加入 SerenaDesktop。首次 Tool acquire 可以自动完成最小 Project Configuration + Runtime Activation；Index/Onboarding 独立延后。

CodeGraph 只承担可选 Workspace-scoped Graph Capability。它的 index/runtime 由 `CodeGraphCapabilityProvider` 接入通用 Manager，不再与 DesktopSelectedWorkspace、Serena Client 或 Broker 全局 Active slot 绑定。

`WorkspaceCapabilityProvider` 统一探测、生命周期和调用边界，但不强行统一各工具的业务语义；V0.2 也不在此处定义动态 Plugin ABI。

---

# 43. Windows Installer

下一正式版本：

```text
不再发布 bare exe
```

发布：

```text
Tauri NSIS x64 current-user installer
```

---

## 43.1 Tauri Target

```json
{
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "windows": {
      "webviewInstallMode": {
        "type": "downloadBootstrapper",
        "silent": true
      },
      "nsis": {
        "installMode": "currentUser"
      }
    }
  }
}
```

---

## 43.2 WebView2

场景：

```text
WebView2 exists
→ use existing runtime

WebView2 absent + network
→ download bootstrapper

WebView2 absent + offline
→ V0.2 documented limitation
```

Offline Installer 留后续。

---

# 44. Product Version Policy

权威产品版本：

```text
src-tauri/tauri.conf.json::version
```

正式 Release Tag：

```text
vX.Y.Z
```

必须一致。

SerenaDesktop 项目级政策：

正式发布时同步：

```text
tauri.conf.json
Cargo.toml
package.json
git tag
```

这是项目政策，不是 Tauri 技术强制。

这样 `CARGO_PKG_VERSION` 上报 Codex 时也是实际产品版本。

---

# 45. Single Instance：Portable 与 Installed

Installed 与旧 portable 必须共享：

> **同一个 SerenaDesktop Single Instance Identity。**

配置与 StateStore 继续由固定应用 identifier `io.github.lifei6671.serena-desktop` 对应的 Tauri `app_config_dir()` / `app_data_dir()` 解析；V0.2 不新增数据路径迁移子系统。Release Gate 必须实测 portable 与 installed 对同一用户解析到相同 config/agent-state 路径。

不得同时运行两个 Host。

后启动者：

```text
detect existing Host
    ↓
do not open StateStore writer
    ↓
focus/notify existing instance where possible
```

必须验证：

```text
portable running → installed launch
installed running → portable launch
```

两种情况下都不能出现双 Host。

---

# 46. Autostart

安装版 Auto Start：

```text
必须指向 installed executable
```

不得继续引用旧 portable path。

---

# 47. Uninstall

Installer Remove：

```text
remove installed program files
```

默认保留：

```text
AppData config
Workspace Registry
agent-state.db
OAuth state
```

删除所有用户数据留后续显式选择。

---

# 48. Release Gate

当前 CI Gate 与目标 Gate 分开。

V0.2 Phase 0 增加：

```json
"test": "node --test src/*.test.mjs"
```

Target Release Gate：

```text
npm ci
npm run lint
npm run build
npm test

cargo fmt --check
cargo check --locked
cargo clippy --all-targets -- -D warnings
cargo test --locked

git diff --check
version consistency gate
```

如果新增 Gate 发现历史基线失败：

单独建立 baseline cleanup task，不在 Agent Platform 改造里顺手大修。

---

# 49. GitHub Actions Release

目标：

```text
Checkout
Node 22
Rust stable
npm ci
Quality Gates
Version Gate
Tauri NSIS build
Installer verification
GitHub Release
```

删除：

```text
--no-bundle
```

不再上传：

```text
target/release/serena-desktop.exe
```

---

# 50. Stable Error Codes

## Workspace

```text
WORKSPACE_NOT_FOUND
WORKSPACE_CONTEXT_REQUIRED
WORKSPACE_ALREADY_EXISTS
WORKSPACE_ROOT_NOT_FOUND
WORKSPACE_ROOT_NOT_DIRECTORY
WORKSPACE_NAME_INVALID
WORKSPACE_IN_USE
WORKSPACE_CHANGED
```

Picker Cancel 不是 error。

Workspace-scoped Tool 缺少 required `workspaceId` 返回 `WORKSPACE_CONTEXT_REQUIRED`；字段存在但类型错误、空串或仅空白时在 Schema/参数层返回 `INVALID_PARAMS`；只有 syntactically valid 但未注册的 ID 返回 `WORKSPACE_NOT_FOUND`。禁止用 DesktopSelectedWorkspace、Transport Session、最近请求、Broker 全局 ActiveWorkspace 或 `workspace_activate` 状态兜底。

---

## Workspace Capability

```text
WORKSPACE_CAPABILITY_NOT_FOUND
WORKSPACE_CAPABILITY_NOT_INSTALLED
WORKSPACE_CAPABILITY_NOT_PREPARED
WORKSPACE_CAPABILITY_PREPARATION_REQUIRED
WORKSPACE_CAPABILITY_PREPARING
WORKSPACE_CAPABILITY_PREPARE_FAILED
WORKSPACE_CAPABILITY_OBSERVE_FAILED
WORKSPACE_CAPABILITY_BUSY
WORKSPACE_CAPABILITY_START_FAILED
WORKSPACE_CAPABILITY_RUNTIME_LOST
WORKSPACE_CAPABILITY_STOP_FAILED
WORKSPACE_CAPABILITY_CONTRACT_ERROR
```

`WorkspaceCapabilityManager` 只产生统一错误；Provider Adapter / compatibility mapper 可以把它们映射为既有 Tool family 错误码。Readiness 观察失败只进入 Health 状态，不使 `workspace_register` 失败。`PREPARATION_REQUIRED` 表示需要 Local Human Action；`PREPARING` 表示已有同 key operation 正在运行，调用方应订阅 progress 或复用 operation，而不是重复启动。

---

## CodeGraph

```text
CODEGRAPH_BUSY
CODEGRAPH_NOT_INITIALIZED
CODEGRAPH_RUNTIME_START_FAILED
CODEGRAPH_RUNTIME_LOST
```

`CODEGRAPH_BUSY` 表示容量已满且所有 Slot 均 starting/stopping/in-flight，没有安全的 LRU 驱逐对象。

上述 CodeGraph codes 是 `WORKSPACE_CAPABILITY_*` 的兼容映射，不是 Manager 内部特判。

---

## Source

```text
SOURCE_INVALID_ARGUMENT
SOURCE_PATH_OUTSIDE_WORKSPACE
SOURCE_NOT_FOUND
SOURCE_IO_ERROR
SOURCE_OUTPUT_LIMIT_EXCEEDED
SOURCE_INPUT_LIMIT_EXCEEDED
SOURCE_FILE_TOO_LARGE
SOURCE_BINARY_REJECTED
SOURCE_RANGE_INVALID
SOURCE_ALREADY_EXISTS
SOURCE_VERSION_REQUIRED
SOURCE_VERSION_CONFLICT
SOURCE_CONTENT_NOT_FOUND
SOURCE_CONTENT_AMBIGUOUS
SOURCE_WRITE_REMOTE_DISABLED
```

---

## Semantic

```text
SEMANTIC_PROVIDER_UNAVAILABLE
SEMANTIC_PROVIDER_BUSY
SEMANTIC_RUNTIME_START_FAILED
SEMANTIC_RUNTIME_LOST
```

Capability Runtime error 必须携带 Workspace provenance 和 bounded safe message，不得返回 raw subprocess/transport error。Busy/StartFailed/RuntimeLost 只影响目标 Workspace Slot。

上述 Semantic codes 同样由 Serena Adapter 映射统一 Workspace Capability error。

---

## Provider

```text
AGENT_PROVIDER_NOT_FOUND
AGENT_PROVIDER_UNAVAILABLE
AGENT_PROVIDER_CAPABILITY_UNSUPPORTED
AGENT_PROVIDER_CONTRACT_ERROR
AGENT_PROVIDER_OPERATION_FAILED
```

Legacy `BACKEND_UNAVAILABLE` 只允许在 compatibility mapper 内出现。

---

## Activity

```text
AGENT_ACTIVITY_CONTRACT_ERROR
AGENT_OBSERVE_INVALID_ARGUMENT
```

---

## Usage

```text
USAGE_BASELINE_MISSING
USAGE_COUNTER_REGRESSION
USAGE_EVENT_INVALID
USAGE_IDENTITY_MISMATCH
USAGE_TELEMETRY_FROZEN
```

---

# 51. Security Boundaries

## 51.1 Workspace Registration

Workspace Add/Remove/Rename 只有 Local Desktop Human Authority。

Remote MCP 不可扩大文件访问 Root 集合。

Workspace Registry 是全局授权集合，不是全局执行上下文。DesktopSelectedWorkspace 只属于本地 UI；每个 Remote Workspace-scoped 请求必须显式提供已注册 `workspaceId`，不存在 Session binding、ActiveWorkspace 或最近 Workspace fallback。Transport Session 只能承载生命周期、日志、诊断和限流信息。

---

## 51.2 Source Read

- request.workspaceId → WorkspaceResolver → WorkspaceLease；
- 任意 path 参数 relative-only，并经 WorkspacePathResolver；
- absolute path 只由服务端从 Lease 派生；
- no root escape；
- junction/reparse validation；
- bounded traversal；
- no shell filesystem implementation。

---

## 51.3 Source Write

- Remote Direct Write 默认关闭；
- existing file modification requires expectedSha256；
- per-Workspace generation/root/registration commit revalidation；
- WorkspaceWriteGuard refcount blocks Remove until commit/abort；
- per-canonical-target Commit Mutex serializes Host-managed commits；
- locked SHA/path revalidation detects external changes best-effort，without claiming cross-process transaction isolation；
- binary reject；
- size limit；
- crash-safe replace。

---

## 51.4 Workspace-scoped Provider Runtime

- Provider 首版只能编译期注册；`providerId`/Tool name 必须唯一并通过 allowlist/schema 校验；
- `WorkspaceCapabilityProvider` 的 Descriptor 不能自行扩大 Remote Tool surface；
- Runtime key 使用服务端解析的 `(workspaceId, generation, canonicalRoot)`，不接受 caller root；
- 任意 Provider process 的 cwd/path/project 参数只能来自 WorkspaceLease；
- `workspace_capability_prepare` 只属于 Local Human Authority，Remote MCP 不得直接调用；
- 普通 Remote Tool acquire 只能执行 Descriptor 编译期声明为 `auto_on_first_tool_call` 的最小准备动作；首版只允许 Serena 默认 Project Configuration + Runtime Activation，禁止 Index、Onboarding、CodeGraph init/index/sync；
- Serena 自动 Project Creation 是唯一首版 Remote acquire 持久化例外：只允许在已注册 canonical Root 下、目标 `.serena/project.yml` 缺失时由 Serena 官方默认路径创建，禁止覆盖已有配置或写入其他路径；
- 自动准备只能写 Provider 声明的 Workspace 内受管路径，必须重验 canonical Root、后置条件并发布 safe Activity；
- Transport endpoint 只绑定 loopback，并归对应 Slot 所有；
- 不在 Workspace 间共享 Client、process handle、启动 Future 或 project-specific cache；
- `in_flight > 0` 禁止 eviction/remove；
- Runtime status 和公共错误不得泄露 PID、port、命令行、raw provider error 或未授权绝对路径；
- 一个 Slot crash 不得清理、切换或降级其他 Workspace Slot。
- Provider-specific readiness/prepare 命令只能存在于 Adapter，Core 不解析其人类文本或执行未声明副作用；
- 新 Provider 必须提供 installation/readiness probe、Prepare Authority/side-effect manifest、Runtime ownership、stop evidence、safe error mapping 和隔离测试。

---

## 51.5 Activity

只暴露 safe classification，不暴露 model reasoning。

---

## 51.6 Usage

Usage 不决定：

```text
retry
terminal
Claim release
Runtime safety
```

---

## 51.7 Provider

实现 `AgentProvider` 不自动获得 workspace_write 安全资格。

新 Provider 需要独立 Runtime Safety Review。

---

# 52. 分阶段实施计划

## Phase 0 — Contract Freeze / Baseline

内容：

- 冻结 revision003；
- 冻结 Workspace Registry API；
- 冻结必填 `workspaceId` Tool Schema、缺失字段的 `WORKSPACE_CONTEXT_REQUIRED`、统一 Workspace provenance，并确认无 Session Workspace Binding；
- 冻结 `workspace_list` Discovery-only 契约：返回 ID/catalog，不 Activate、不建立 Binding，已知 ID 后无需重复 Query；
- 冻结所有 path 参数为 Workspace-relative，服务端经 WorkspacePathResolver 从 Lease 派生 absolute target；
- 冻结 Capability Runtime Slot state/error contract；
- 冻结 object-safe `WorkspaceCapabilityProvider`、Descriptor、Registry 和统一 installation/readiness/stage/action/error/health DTO；
- 冻结 Serena `--project <canonicalRoot>` 首次激活的默认 Project Creation、后置 Root 校验、非交互行为与最低版本证据；
- 冻结 Serena Index/Onboarding 不阻止 Semantic Runtime ready 的契约，以及 index readiness 的可信证据来源；
- 冻结 CodeGraph `status --json` schema/Root identity 校验与 `init --yes`、`sync`、显式 rebuild 命令；
- 记录 Serena/CodeGraph 单实例 Windows working set、startup latency、idle stop latency 与并发调用证据；
- 验证 Serena 多进程可同时使用独立 loopback endpoint、各自固定 project state 且互不改写共享配置；
- 若共享 `SERENA_HOME/serena_config.yml` 的多进程验证失败，则阻止 Phase 2A.3 开始，并通过一次 DCR 在 per-slot `SERENA_HOME` 与 Serena `maxInstances=1` 的 Workspace-scoped Slot 策略中明确选择；后者在其他 Workspace Slot in-flight 时返回 busy，不使用可 retarget 的全局 Process。V0.2 不提前实现 Mutex/per-slot Home/单实例三套 fallback；
- 验证 CodeGraph 多进程可同时绑定不同 canonicalRoot/index，并确认现有 index 的 Root identity 校验来源；
- 据此冻结首版 max running instances / idle timeout 内部常量；
- 冻结 Source Write Contract；
- 冻结 Activity Observe Contract；
- 冻结 Activity/Usage DDL；
- 固定 Codex binary/version/hash；
- 运行真实 Usage Contract Test；
- 建立 Provider allowlist/forbidden identifier Gate；
- 增加 `npm test` script；
- 建立 Target Release Gates；
- 版本同步策略；
- 新依赖 license/supply-chain review。

Phase 0 负责冻结公共契约、建立基线并定义各阶段前置 Gate，不是“所有外部验证全部完成后才能开始任何后续开发”的单体阻塞点。某项外部 Contract Evidence 只阻塞实际依赖它的阶段：

```text
Serena multi-process / shared-config evidence
    → required before Phase 2A.3

CodeGraph status / multi-process evidence
    → required before Phase 2D

Codex Usage Contract evidence
    → required before Phase 4

Installer environment evidence
    → required before Phase 6
```

Phase 1、Phase 2A.1 等不依赖上述外部 Contract 的工作无需等待全部 Phase 0 evidence；下列 Gate 内容保持不变，只按依赖阶段解释其阻塞范围。

Gate catalog（按上述依赖阶段应用）：

```text
Runtime tests baseline known
Serena implicit project creation/activation evidence recorded
Serena shared-config multi-process test passed OR DCR resolved before Phase 2A.3
CodeGraph machine-readable status contract recorded
Serena/CodeGraph version contract probes recorded; no hash pin required
Workspace capability provider contract frozen
Work tests baseline known
Observe tests baseline known
frontend tests baseline known
fmt/check/clippy baseline known
Codex Usage contract evidence recorded
```

---

## Phase 1 — Provider-Agnostic Control Plane

内容：

- ProviderId；
- Descriptor；
- Capabilities；
- Registry；
- object-safe AgentProvider；
- AgentTaskManager → Registry；
- startupReconcile；
- EventSink；
- TelemetryProjector；
- Provider Product DTO；
- public Provider Error mapping。

Gate：

- Runtime/Recovery/Cancel 无回归；
- requestKey/Claim/Atomic Release 无回归；
- Control/Product 不依赖 thread/turn/job/historyMode；
- compatibility projector 是唯一 Provider-Opaque allowlist；
- Revision Hash 不依赖 Provider-private identity；
- CI/AST/rg Gate 可机械检查；
- forged safe/evidence 无法释放 Claim。

---

## Phase 2A.1 — Workspace Registry Authority / Project CRUD

内容：

- `ManagerConfig.workspaces` Authority；
- 停止 startup Serena sync，旧项目原样保留；
- Manual Directory Picker；
- `workspace_inspect_directory` 只做 Root/basic metadata 检查；
- Register / Rename / Remove / Reorder；
- CRUD 复用 `SupervisorState.operation` + atomic config persist；
- DesktopSelectedWorkspace；
- workspaceRegistryRevision / per-Workspace generation；
- Missing Root handling；
- optional Serena Import；
- Workspace 不要求 Git。

Gate：

```text
old Serena-synced projects preserved
startup Serena sync no longer overwrites ManagerConfig.workspaces after Phase 2A.1
manual directory add works and appears immediately
non-Git directory works
duplicate canonical root rejected case-insensitively on Windows
rename preserves id/root/generation
remove never deletes disk
selected workspace can be removed and UI selection clears
concurrent CRUD/config writes are serialized by existing operation mutex
restart preserves projects
Serena import is additive/idempotent
```

---

## Phase 2A.2 — Workspace Resolver / Request Authority / Execution Freeze

内容：

- WorkspaceResolver / WorkspaceLease；
- Remote Workspace-scoped Tool Schema 必填 `workspaceId`，缺少时返回 `WORKSPACE_CONTEXT_REQUIRED`；
- Local IPC 也显式传 `workspaceId`，不得从 DesktopSelectedWorkspace 推导；
- Workspace Discovery 与 Execution Authority 分离，已知 ID 后无需重复 `workspace_list`；
- 不建立 SessionWorkspaceBindings/Expiry/Persistence/Reconnect Binding，移除 Activate/Current/Deactivate 路由 Authority；
- 任意 path 参数只接受 Workspace-relative path，由 WorkspacePathResolver 解析；
- Agent Execution Workspace 创建时冻结；
- Registry Remove 与 WorkspaceWriteGuard / Agent Claim / Capability Operation exclusion；
- 删除 Broker 全局 ActiveWorkspace 对 Source/Git/Agent request routing 的执行 Authority。

Gate：

```text
missing required workspaceId is WORKSPACE_CONTEXT_REQUIRED
wrong-type/blank workspaceId is INVALID_PARAMS
syntactically valid unknown workspaceId is WORKSPACE_NOT_FOUND
request A(workspaceId=A) / B(workspaceId=B) concurrently resolve separate leases and do not cross
workspace_list/query does not bind later requests; known workspaceId can be reused without another query
HTTP transport remains sessionless JSON response mode
Desktop selection never becomes Remote or Local IPC execution Authority
execution workspace remains immutable across Continue/UI/other requests
active write/running execution workspace rejected by Remove
no long-lived global Workspace Registry lock
```

### Phase Gate Backend Continuity

从 Phase 2A.2 Gate 开始，每个 Phase Gate 结束后的可运行版本中，所有已经公开的 Workspace-scoped Tool 都不得继续依赖 Broker Global ActiveWorkspace。最终 Adapter 尚未落地时，可以通过 compatibility facade 保持功能连续，但 facade 也必须接收 `request.workspaceId → WorkspaceResolver` 产生的 `WorkspaceLease`，不得重新读取隐式 Active Workspace、DesktopSelectedWorkspace 或 Session Binding。Compatibility facade 只是既有 handler 的迁移接线，不构成第二套架构。

由于当前四个 Source Read Tool 仍由 Serena backend 提供，Phase 2A.2 的 Authority 代码可以先完成，但不能把仅有 2A.2、尚无 Lease-routed Serena backend 的构建标记为可运行 Gate 完成；该 Gate 的公开 Tool 连续性必须与 Phase 2A.3 中最小的 `WorkspaceLease → Serena Workspace Slot` compatibility route 一起落地。这里约束的是交付顺序，不合并 Phase，也不新增 Runtime 模型。

迁移顺序固定为：

```text
Phase 2A.2
    establish workspaceId / WorkspaceLease Authority
    ↓
Phase 2A.3
    establish Workspace-scoped Serena Runtime
    existing Serena-backed Source may temporarily use that Runtime/Lease
    ↓
Phase 2B
    switch four basic Source Read tools to Rust implementation
    ↓
Phase 2D
    converge Source/Git/CodeGraph adapters on WorkspaceCapabilityProvider
```

任何阶段提交或集成检查点都不得留下“Global ActiveWorkspace 已删除、Rust Source 尚未落地、Serena-backed Source 又没有 Lease 路由”的窗口。

---

## Phase 2A.3 — Capability Registry / Serena Workspace Runtime

内容：

- object-safe WorkspaceCapabilityProvider，`call` 显式接收 WorkspaceLease；
- WorkspaceCapabilityRegistry + Manager；
- 注册完成后异步 Capability Readiness observation / Descriptor-driven health UI；
- SerenaCapabilityProvider built-in adapter；
- 现有 Serena-backed Source compatibility facade 通过请求 WorkspaceLease 路由到对应 Serena Slot，保持到 Phase 2B 切换；
- Desktop selection 不 prepare/warm Provider；
- Local `workspace_capability_prepare` + Descriptor-driven actions；
- Serena first Tool Call 自动创建默认 Project Configuration、lazy start，并在 ready 后继续原调用；
- Serena Index 与 Onboarding 独立且不阻止 Semantic ready；
- Capability Preparation Activity/progress；
- Serena lazy start / single-flight / bounded capacity / LRU idle eviction；
- 删除 Broker 全局 ActiveWorkspace 对 Serena/CodeGraph Binding 的 Authority；
- Serena Optional Semantic。

Gate：

```text
Serena missing still works
workspace appears before capability observation completes
provider observe failure does not block registration
Serena shared-config multi-process Phase 0 gate passed or DCR resolved
existing Serena-backed Source tools use request Lease + Workspace-scoped Serena Slot, never Global ActiveWorkspace
Serena missing project.yml is auto-preparable, not workspace unavailable
first Serena tool call creates default project configuration without indexing/onboarding
root comparison/postcondition is canonical and case-insensitive on Windows
Desktop select/restore does not warm runtimes
explicit Serena prepare and build_index remain separate actions
registering many workspaces starts zero Serena runtimes
same-workspace concurrent first calls start one Serena process
different-workspace Serena calls never share or retarget a process
different-workspace Serena calls run concurrently only when maxInstances allows
maxInstances=1 + A in-flight + B request returns capability busy; never activate B in A process
in-flight Serena slot is never evicted
semantic acquire may auto-create only Serena default project configuration
semantic acquire never runs Serena index/onboarding
```

---

## Phase 2B — Rust Source Read

内容：

```text
source_read_file
source_list_dir
source_find_file
source_search_pattern
```

Gate：

- WorkspaceLease；
- required workspaceId schema, no implicit fallback；
- existing Source input field remains relative_path；
- no long global Workspace lock；
- full raw SHA；
- source_read_file max_bytes default 32 KiB / hard max 128 KiB；
- provenance；
- output budgets；
- cancellation；
- ignore/hidden/symlink/binary；
- junction escape。

---

## Phase 2C — Rust Source Write

内容：

六个 Source Write Tool。

Gate：

- hard request/file limits；
- closed range；
- empty-input contract；
- expectedSha256；
- per-Workspace generation/root/registration recheck；
- WorkspaceWriteGuard refcount / Remove exclusion；
- per-canonical-target Commit Mutex；
- same-target concurrent writes produce deterministic OCC conflict；
- replace first/all contract；
- Binary reject；
- newline tie-break；
- safe replace；
- stale hash；
- crash test；
- junction attack；
- Remote Write disabled。

---

## Phase 2D — Workspace Capability Adapters / CodeGraph / Health UI

内容：

- Source/Git Adapter；
- Git Tool 显式 `workspaceId`；
- `git -C canonicalRoot` 在 WorkspaceLease 上运行；
- Git 的可选 path 参数只接受 Workspace-relative path；
- CodeGraphCapabilityProvider built-in adapter；
- CodeGraph readiness 使用 `status --json` 校验 initialized/projectPath/index.state；
- CodeGraph init/sync/rebuild 只通过 Local explicit actions；
- Runtime acquire 独立验证 index/process readiness，绝不自动构建索引；
- 通用 Manager 按 `(providerId, workspaceId, generation)` 隔离 runtime slot；
- CodeGraph lazy start / single-flight / bounded capacity / LRU idle eviction；
- Descriptor-driven per-Workspace capability health；
- Runtime Remove / Host shutdown cleanup。

Gate：

```text
Serena unavailable does not fail Desktop Core
adding another built-in provider requires no Workspace Core branch
registering many workspaces starts zero CodeGraph runtimes
same-workspace concurrent first calls start one CodeGraph process
different-workspace queries never share or retarget a process
different-workspace CodeGraph queries run concurrently only when maxInstances allows
Source/Git/Serena/CodeGraph handlers route only after request workspaceId resolves a WorkspaceLease
WorkspaceCapabilityProvider.call receives that server-resolved Lease explicitly
capacity full evicts only zero-in-flight LRU slot
all slots in-flight returns CODEGRAPH_BUSY
idle timeout stops process but preserves index/registry
query acquire never initializes/syncs/rebuilds CodeGraph index
provider crash affects only one workspace slot
remove idle workspace stops runtime before registry deletion
host shutdown leaves no provider orphan process
```

---

## Phase 3 — Agent Activity Observe

内容：

- summaryCode deterministic mapping；
- activity-v2；
- activity_sequence；
- Activity 不 bump execution revision；
- bounded Activity History query；V0.2 no automatic prune；
- Observe Snapshot may coalesce intermediate revisions；
- public waitMs default 15000, range 0..=20000；
- knownActivityRevision；
- wakeOn；
- wakeReason；
- Work Adapter；
- MCP Schema / Description。

Gate：

```text
running/test → finalizing changes Activity Revision
same finalizing heartbeat does not
activity-only update does not increment executions.revision
known Activity mismatch immediate
same semantic heartbeat no wake
Activity change no controlRevision change
disconnect does not cancel
```

---

## Phase 4 — Usage

内容：

- Public Usage；
- private Codex checkpoint；
- atomic nullable baseline；
- terminal coverage；
- terminal grace；
- freeze；
- late events；
- Product projection。

Gate：

```text
fresh Thread
Continue
restart
nullable baseline
baseline missing
duplicate
out of order
regression
terminal partial
terminal complete
late within grace accepted
after grace rejected
runtime teardown immediately frozen
```

Usage 不影响 Claim/terminal。

---

## Phase 5 — Agent UI

内容：

- Provider；
- Activity；
- Token Usage；
- Hover Total Token。

Gate：

- unknown；
- partial；
- complete；
- real zero；
- historical task；
- no N+1 request。

---

## Phase 6 — Installer / Release

内容：

- bundle active；
- NSIS；
- currentUser；
- WebView2 bootstrapper；
- Target Gates；
- Version Sync；
- GitHub Actions；
- no bare exe；
- build.rs update；
- shared single-instance identity；
- portable → installed migration tests。

Gate：

```text
tag → CI → installer → Release
```

---

## Phase 7 — Manual E2E Acceptance

不是 CI Gate。

完整真实路径：

```text
upgrade existing user
    ↓
old Serena projects remain
    ↓
manual add non-Git workspace
    ↓
project immediately appears; capability readiness resolves asynchronously
    ↓
Desktop selects it as default for new task
    ↓
selection starts no Provider Runtime
    ↓
first Serena Tool Call auto-prepares default project + starts isolated Runtime
    ↓
optional Local [建立索引] prepares Serena/CodeGraph index with progress
    ↓
ChatGPT workspace_list discovers workspaceId=A/B once
    ↓
Request A: source_read_file(workspaceId=A, relative_path=...)
    ↕ concurrently, no shared Active Workspace / Session Binding
Request B: git_diff(workspaceId=B, path=...) / Source read(relative_path=...)
    ↓
Rust Source safe write(workspaceId=A)
    ↓
ChatGPT Work begin(workspaceId=A)
    ↓
agent_execute Codex
    ↓
agent_query wakeOn=activity
    ↓
ChatGPT sees activity
    ↓
Token Usage
    ↓
terminal
    ↓
Host review
    ↓
Work finish
    ↓
installer/uninstaller
```

---

# 53. 测试矩阵

## Workspace / Project Management

| 场景 | 预期 |
|---|---|
| 从旧版本升级 | Serena-synced projects 全部保留 |
| Serena 删除 | Registry 项目仍存在 |
| Phase 2A.1 后启动 | 不再调用 startup Serena sync 覆盖 `ManagerConfig.workspaces` |
| Picker Cancel | no-op |
| Serena/CodeGraph 状态观察很慢 | Workspace 先注册并立即展示，状态暂为 `checking/unknown` |
| Serena 未安装 | Workspace 注册成功；异步状态为 `not_installed` |
| `.serena/project.yml` 不存在 | Serena `not_prepared/auto_preparable`，Source/Git 正常 |
| `.serena/project.yml` 存在 | 只证明 Project Configuration，不推断 Runtime/Index/Onboarding ready |
| 首次 Serena Tool Call | 默认配置自动创建、独立 Runtime 启动、随后执行原 Tool |
| 两个调用同时首次准备 Serena | 复用同一 prepare/start operation，只创建一个 Project/Process |
| Serena 自动准备 | 不运行 Index 或 Onboarding |
| Serena 配置创建成功但 Runtime/LSP 失败 | 保留 project.yml；configuration=ready、runtime=error |
| Local Serena `[建立索引]` | 显式执行 index，发布进度；失败不删除 Project/Workspace |
| Serena/CodeGraph index while Tool in-flight | busy，不并发改写 index/cache |
| CodeGraph `status --json` Root 不匹配 | `error/unknown`，不复用 index |
| CodeGraph index complete | `ready` |
| CodeGraph pendingChanges/reindexRecommended | `degraded/stale`，不自动更新 |
| CodeGraph index absent | `not_prepared`，展示 `[建立索引]` |
| CodeGraph binary missing but `.codegraph/` exists | `not_installed/unknown`，不猜测或删除现有 index |
| Local CodeGraph `[建立索引]` | 执行 `init --yes`，完成后 readiness=ready |
| Remote CodeGraph 首次 query 且 index absent | `PREPARATION_REQUIRED`，不创建 `.codegraph` |
| 任一 Provider observe timeout | 该 Provider `unknown`，Workspace 注册和其他 Provider 不受影响 |
| 添加普通目录 | 成功 |
| 添加非 Git 目录 | Workspace ready / Git unavailable |
| 添加重复 canonical root | `WORKSPACE_ALREADY_EXISTS` |
| 同名不同 Root | 允许 |
| Rename | ID/Root 不变 |
| Remove | 磁盘目录不变 |
| Remove DesktopSelectedWorkspace | 成功并清除 UI 选择，不删除目录 |
| Remove Workspace with active Read Lease | Remove 成功，已开始的 Read 可完成，未来解析 not found |
| Remove Workspace with active WorkspaceWriteGuard | `WORKSPACE_IN_USE` |
| Remove Workspace with Capability Prepare/Index running | `WORKSPACE_IN_USE` |
| Remove Workspace with capability Provider in-flight | `WORKSPACE_IN_USE` |
| Remove Workspace with idle Provider Slot | stop/dispose Slot 后成功 Remove |
| Idle Provider stop fails during Remove | Registry 保留，`WORKSPACE_CAPABILITY_STOP_FAILED` |
| Remove Running Execution Workspace | `WORKSPACE_IN_USE` |
| Root Missing | Registry 保留 |
| Resolve missing Root | `WORKSPACE_ROOT_NOT_FOUND` |
| Restart | 手工项目仍存在 |
| Serena Import twice | 幂等 |
| Serena Import | 不删除 Local Workspace |
| Missing required `workspaceId` | `WORKSPACE_CONTEXT_REQUIRED`，不读取任何 fallback |
| Wrong-type/blank `workspaceId` | `INVALID_PARAMS` |
| Valid but unknown `workspaceId` | `WORKSPACE_NOT_FOUND` |
| Request A(workspaceId=A) 与 Request B(workspaceId=B) 并发 | 分别解析 A/B WorkspaceLease；无共享 Active Workspace、无 Session Binding、互不串线 |
| ChatGPT 已通过一次 `workspace_list` 获得 A | 后续 Source/Git/CodeGraph/Semantic 请求无需重复 Query，但每个请求仍显式携带 A |
| `workspace_list` / `workspace_get(A)` | 只返回 Discovery 数据，不使后续缺少 ID 的请求自动使用 A |
| Transport reconnect | 只改变连接生命周期；不恢复或建立 Workspace Binding，后续请求仍显式携带 `workspaceId` |
| Desktop selects B while request uses A | 请求继续使用 A |
| 显式选择任意 Workspace | 只更新 Desktop selection，不启动/准备 Provider |
| 恢复上次 Desktop selection | 不自动 warm Provider Runtime |
| Rename/Reorder B while write A | A generation 不变 |

---

## Source Read

| 场景 | 预期 |
|---|---|
| Phase 2A.3 已完成但 Phase 2B 尚未切换 | 四个现有 Source Read 经 request Lease + Serena compatibility facade 保持可用，不读取 Global ActiveWorkspace |
| `../` escape | 拒绝 |
| Windows/Unix absolute path、UNC path 或 Workspace root | 拒绝；不接受 caller-provided Root |
| Source Read input schema | 保持 `relative_path`，不重命名为 `path` |
| junction escape | 拒绝 |
| UI selection / other request changes Workspace | 当前 Read 返回原 Lease provenance |
| Workspace removed after Read captures Lease | Read 可完成；未来请求 not found |
| truncated read | SHA 仍为完整文件 |
| source_read_file omits max_bytes | 使用 32 KiB 默认预算 |
| source_read_file max_bytes=131072 | 合法；最多返回 128 KiB |
| source_read_file max_bytes=0/131073 | `INVALID_PARAMS` |
| binary content | skip |
| binary filename match | 可命中 path |
| huge repo | bounded stop |
| cancel | 快速停止 |

---

## Source Write

| 场景 | 预期 |
|---|---|
| Source Write input schema | 保持 `relative_path`，不重命名为 `path` |
| create existing | already exists |
| write overwrite no SHA | version required |
| stale SHA | version conflict |
| content > hard limit | input limit exceeded |
| target > file max | file too large |
| insert empty | invalid argument |
| replace oldContent empty | invalid argument |
| delete 3–5 | 删除 3/4/5 |
| delete 1–N | 空文件 |
| replace empty content | 删除 range |
| insert N+1 | append |
| expectedMatches mismatch | ambiguous |
| mode=all no max | invalid argument |
| invalid UTF-8 | binary rejected |
| NUL | binary rejected |
| CRLF/LF tie | first newline wins |
| Desktop/other request changes Workspace before commit | 不影响当前 Write |
| Other Workspace changes before commit | 不影响当前 Write |
| Same Workspace removed/root authority changes | `WORKSPACE_CHANGED`，不得 commit |
| Same target two writes both read SHA=A | Commit 串行；至多一个成功，另一个 version conflict |
| Different target paths in same Workspace | 可并发 Commit |
| External editor changes target before locked revalidation | SHA 不同则 version conflict；不声明跨进程事务隔离 |
| Remove during active WorkspaceWriteGuard | `WORKSPACE_IN_USE` |
| crash during write | no half file |
| Remote default | write unavailable |

---

## Capability Runtime / Git / Agent Workspace Isolation

| 场景 | 预期 |
|---|---|
| Git A 与 Git B 并发 | 分别在 A/B canonicalRoot 执行 |
| `git_diff` / `git_log` path 为 absolute/UNC/escape | 拒绝；只接受 Workspace-relative path |
| 注册第三个 built-in WorkspaceCapabilityProvider | Core/Manager/UI 无 providerId 特判即可完成 observe/prepare/health/route |
| Provider Descriptor 重复 ID/Tool name | Registry 启动失败，`WORKSPACE_CAPABILITY_CONTRACT_ERROR` |
| Serena/CodeGraph version probe incompatible | 仅对应 Capability unavailable；Core/其他 Provider 正常 |
| 注册 30 个 Workspace 后启动 Desktop | Serena/CodeGraph live process 均为 0 |
| 两个请求首次同时请求 Serena A | single-flight，只启动一个 A Process |
| 两个请求首次同时请求 CodeGraph A | single-flight，只启动一个 A Process |
| Serena Project 未配置 | Tool acquire 自动使用默认配置创建并启动，不执行 index/onboarding |
| Serena optional index absent/stale | Semantic Tool 仍可运行，Health 展示 index 状态 |
| CodeGraph index 不存在 | `CODEGRAPH_NOT_INITIALIZED/PREPARATION_REQUIRED`，不创建 `.codegraph` |
| `.codegraph/` 存在但 status/index 不可用 | readiness error/degraded；Runtime acquire 不自动 reindex |
| Serena A 与 B，容量允许 | 分别使用 A/B Process 真正并发，不 activate 切换 |
| Serena `maxInstances=1`，A in-flight 时请求 B | 返回对应 capability busy error；不复用 A Process activate B |
| CodeGraph A 与 B，容量允许 | 分别查询 A/B runtime/index，可真正并发 |
| CodeGraph 容量不允许且无安全 LRU | 返回 `CODEGRAPH_BUSY`，不 retarget live process |
| 同一 Slot 并发能力尚未验证 | Slot 内串行；其他 Workspace 仅在 Provider 容量允许时并发 |
| Runtime 容量满且存在 idle Slot | stop LRU idle Slot 后启动新 Workspace |
| Runtime 容量满且所有 Slot in-flight | Serena/CodeGraph 返回各自 busy error，不驱逐 |
| LRU Slot stop fails | 保留 handle/容量，不启动替代进程，不产生 orphan |
| Slot 超过 Idle Timeout | process 停止，Registry/index 保留 |
| in-flight Slot 超过 Idle Timeout | 不停止；guard 释放后重新判断 |
| Provider process crash | 仅目标 Slot 进入 error，其他 Workspace/Core 正常 |
| 同一 Slot 同时 eviction/remove/shutdown | stop single-flight，不重复 kill |
| Host shutdown | 所有 live Serena/CodeGraph process 收敛，无 orphan |
| Execution A 运行时 UI 选择 B | Execution 仍使用冻结的 A |
| Execution A 运行时其他请求显式访问 B | Execution 仍使用冻结的 A |
| Source/Git Adapter call | 从显式 WorkspaceLease 取得 Root，不读取 Tool caller root |
| Serena Semantic / CodeGraph Adapter call | 从请求 WorkspaceLease identity 取得对应 Workspace-scoped Runtime Slot |
| Runtime Handle identity 与 call Lease 不一致 | `WORKSPACE_CAPABILITY_CONTRACT_ERROR`，不调用 Provider |
| Continue Execution A | 不接收新 workspaceId，继续使用 A |
| Recovery identity mismatch | fail-closed，保留 Claim |

---

## Provider

| 场景 | 预期 |
|---|---|
| CLI missing | provider unavailable |
| unknown provider | not found |
| public branch reads threadId | architecture Gate fail |
| controlRevision includes turnId | Gate fail |
| forged safe | reject |
| forged terminal telemetry | reject |
| recovery unknown | Claim retained |
| finalize missing evidence | rollback |
| finalize valid | atomic terminal+release |

---

## Activity

| 场景 | 预期 |
|---|---|
| provider | provider.processing |
| edit | tool.edit |
| test | tool.test |
| finalizing | execution.finalizing |
| running/test → finalizing | Activity Revision changes |
| finalizing heartbeat | Revision unchanged |
| Activity-only update | `executions.revision` unchanged |
| A1→A2 | activity wake |
| old v1 token | initial mismatch |
| empty Activity token | invalid argument |
| waitMs omitted / 0 / 20000 | 分别使用 15000 / immediate snapshot / 合法上限 |
| waitMs=20001 | `AGENT_OBSERVE_INVALID_ARGUMENT` |
| A1→A2→A3 before wake response | 允许直接返回最新 A3 Snapshot，不承诺逐 revision 事件 |
| Activity History query | Server-enforced limit + cursor；不自动 prune |
| Usage update | no activity wake |
| raw reasoning | not persisted |

---

## Usage

| 场景 | 预期 |
|---|---|
| unknown | null |
| real zero | 0 |
| invalid float | reject |
| overflow | reject |
| complete baseline/current | delta |
| baseline required field null | whole delta unknown |
| current required field null | whole delta invalid/unknown |
| Provider optional field null | allowed |
| Provider supplies total_tokens | 原样校验/投影，不从 breakdown 重算 |
| Provider omits total_tokens | 保持 null，不相加推导 |
| stale checkpoint | unknown |
| duplicate | no-op |
| regression | reject |
| terminal no coverage | partial |
| terminal covered | complete |
| terminal + 1s late Usage | accepted |
| grace expired | frozen |
| runtime teardown | frozen immediately |
| frozen late event | dropped |

---

## Installer

| 场景 | 预期 |
|---|---|
| tag/version mismatch | CI fail |
| WebView2 present | PASS |
| absent + network | bootstrap |
| absent + offline | documented limitation |
| Chinese username | PASS |
| currentUser | no forced admin |
| portable running + installed launch | no second Host |
| installed running + portable launch | no second Host |
| portable / installed data paths | 固定 identifier 下解析到同一 app_config_dir/app_data_dir |
| autostart | installed path |
| uninstall | binaries removed |
| user AppData | preserved |
| Release | no bare exe |

---

# 54. Migration Strategy

## 54.1 Workspace

现有 `ManagerConfig.workspaces` 条目的 ID/Name/Root 与顺序原样保留。

不需要重新从 Serena 导入。

Phase 2A.1 起，升级后的 startup 直接保留该 Registry，不再执行 Serena Sync 覆盖；Serena Registry 只允许通过第 9.2 节显式 additive Import 读取。

配置迁移为缺失字段补默认值：

```text
Workspace.generation = 1
ManagerConfig.workspace_registry_revision = 1
```

不得因补版本字段重新生成 Workspace ID 或重新 canonicalize 成另一个 Root。

旧版本持久化的 Desktop 当前项目只迁移为 `DesktopSelectedWorkspace`，不得继续作为 MCP/Agent 执行 Authority。V0.2 不迁移、不创建 SessionWorkspaceBindings、Session Expiry、Session Persistence 或 Reconnect Binding；旧 `workspace_activate/current/deactivate` 状态不进入新请求路由。旧客户端若保留这些调用，只能命中 deprecated compatibility surface，不能获得隐式 Workspace Context。

如果升级时存在 nonterminal Execution，只能从其权威持久化 Work/Execution/Claim identity 回填冻结的 Workspace；无法唯一证明时按 Unknown/Fail-Closed 处理，不得使用升级瞬间的 DesktopSelectedWorkspace 猜测。

---

## 54.2 Serena Config

Serena fields 暂不删除，以允许 Optional Semantic Provider 与版本回滚。

---

## 54.3 Workspace Capability Runtime

WorkspaceCapabilityRegistry 在启动时注册 built-in Adapter；Manager 的 Slot、`lastUsedAt`、`inFlight`、starting Future 和 error state 都是进程内状态，不迁移、不持久化。升级后所有 Slot 从 absent/stopped 开始，禁止因 Registry 中存在 Workspace 而 eager start。

旧 Workspace 不批量执行 Provider prepare、Runtime start 或 index；启动后可以低优先级异步观察已安装 Provider 的 Readiness，首次打开详情或 Tool acquire 时按统一 policy 刷新。历史 `initialized` 布尔值不迁移为权威状态，必须投影为新的 installation/readiness/stages DTO。

现有 `.serena/project.yml`、Serena cache/memory 和 `.codegraph` index 原样保留，不批量重建。首次 Serena/CodeGraph acquire 必须重新验证 canonicalRoot 与 Provider evidence 后才能复用。旧全局 Active Serena Client/CodeGraph Binding 不迁移成任意 Workspace Slot，也不从 DesktopSelectedWorkspace 猜测恢复。

---

## 54.4 Activity

历史 Activity 不伪造 Timeline。

可以从当前 execution row 确定性派生当前 `summaryCode`。

History table 保持空。

---

## 54.5 Usage

历史 Execution：

```text
no usage row
→ unknown
```

不补 0。

---

## 54.6 Installer

Portable 不自动删除。

Installed / Portable 共享 Single Instance Identity。

---

# 55. Rollback Strategy

## Control Plane

保留 Codex compatibility facade。

回滚不得修改既有 Runtime Evidence。

## Workspace

Workspace Registry 不重新由 Serena 覆盖。

Serena Import 保持 additive。

Phase 2A.1 之后回滚任何后续 Phase，都不得恢复 startup Serena Sync；否则会重新破坏已经生效的 `ManagerConfig.workspaces` Authority。

回滚不得恢复 Broker 全局 ActiveWorkspace、DesktopSelectedWorkspace、Transport Session、最近请求或 `workspace_activate` 作为 Source/Git/CodeGraph/Serena/Agent 的执行 Authority。兼容 handler 只能维持 deprecated surface，不能建立 Session binding；无法继续满足显式 `workspaceId` + 服务端 `WorkspaceLease` 时，应禁用对应 Workspace-scoped Tool。

## Source Read

可暂时路由回 compatibility backend，但必须继续使用请求 `workspaceId` 解析出的 WorkspaceLease；如果 backend 是 Serena，仍必须经 WorkspaceCapabilityManager 的 `serena` Slot，不能恢复全局 activate/call，也不能接受 caller-provided Root。

## Source Write

可单独 disable。

Read / Agent 保持可用。

## Workspace Capability Runtime

任一 WorkspaceCapabilityProvider 可单独 disable/unregister，并将对应 Workspace capability 标为 unavailable。回滚不得恢复可跨 Workspace retarget 的全局 Serena Process 或 CodeGraph Binding；如果无法维持隔离，宁可 capability unavailable。

## Activity

可以临时退回 control-only observe，但 Activity Schema 保留。

## Usage

可以隐藏 UI，但不能把 null 回填 0。

## Installer

撤销 Release，发布新版本号。

不替换相同 Tag 的二进制。

---

# 56. 最终 Release Acceptance Checklist

以下全部满足才允许 Release。

1. Serena 未安装时 Desktop 启动正常；
2. Serena 失败不影响 Core；
3. 旧 Serena 项目升级后保留；
4. Phase 2A.1 完成后，startup 不再调用 Serena Sync 覆盖 Workspace Registry；
5. 用户可以手动选择目录注册项目；
6. 普通非 Git 目录可成为 Workspace；
7. Workspace Rename/Remove/Reorder 正常；
8. Remove 不删除磁盘目录；
9. Active Write/Running Execution Workspace 不能 Remove，Read Lease 不阻止 Remove；
10. Broker 全局 ActiveWorkspace 不再是任何工具的执行 Authority；
11. 四个 Source Read 全部 Rust 化；
12. Read 不长持 Workspace Registry global lock；
13. Read 返回 Workspace provenance；
14. Read SHA 基于完整文件；
15. Source Write 有 hard size limits；
16. Source Write 使用 expectedSha256；
17. Delete/Replace 使用 1-based inclusive range；
18. 同一 Workspace Entry 的 Path Authority 改变后旧 Write 不能 commit，无关 Workspace/UI/其他请求变化不影响 Write；
19. Source Write crash 不产生半文件；
20. Remote Direct Source Write 默认关闭；
21. Serena 仅是 Optional Semantic Capability；
22. Public Control Plane 不解释 Codex Thread/Turn/Job/historyMode；
23. Provider Registry 首版只有 Codex；
24. `Arc<dyn AgentProvider>` object-safe；
25. Runtime Safety Tests 无回归；
26. ProviderRunResult/Telemetry 无法绕过 Claim Evidence；
27. Finalize 继续重读权威 Evidence；
28. summaryCode 派生规则唯一确定；
29. running/test → finalizing 改变 Activity Revision；
30. heartbeat 不改变 Activity Revision；
31. Activity-only update 不 increment `executions.revision`；
32. Work Adapter 真正支持 `knownActivityRevision`；
33. `wakeOn=activity` 真正到达 Observe；
34. mismatch 首次读取立即返回；
35. MCP description 明确 Activity 模式不要只看 `unchanged`；
36. Activity 不泄露 Reasoning/command/stdout/source；
37. Usage null 与真实 0 可区分；
38. Continue baseline 是原子可信 Snapshot；
39. Usage 没 terminal coverage 不标 complete；
40. terminal grace 内 late Usage 可写；
41. Runtime teardown / grace 后 Usage frozen；
42. Usage 错误不影响 Claim / terminal；
43. Task Detail 展示 Usage；
44. Task Hover 展示 Total Token；
45. Provider UI 名称来自 Descriptor；
46. `npm test` 成为正式 Gate；
47. Target CI Gates 全绿；
48. 产品版本一致；
49. NSIS current-user Installer 正常；
50. WebView2 Bootstrapper 策略验证；
51. Portable / Installed 共用 Single Instance Identity；
52. Autostart 指向 Installed Path；
53. GitHub Release 不再发布裸 exe；
54. Clean Windows Manual E2E 全通过；
55. DesktopSelectedWorkspace 只影响 UI 和新 Task 默认值；
56. `workspace_list` / Workspace Query 只做 Discovery，不 Activate、不建立 Binding；
57. ChatGPT 一旦获得 `workspaceId`，后续请求无需重复 Query，但每个 Workspace-scoped Tool 仍显式传递该 ID；
58. Source/Git/CodeGraph/Serena Semantic/未来 Workspace-scoped Capability Tool/Agent 新建请求显式接收 `workspaceId`；
59. Workspace-scoped Tool 的 `workspaceId` 在公共 Schema 中必填；
60. 缺少 required `workspaceId` 返回 `WORKSPACE_CONTEXT_REQUIRED`；
61. wrong-type、空串或仅空白 ID 返回 `INVALID_PARAMS`；
62. syntactically valid 但未注册的 ID 返回 `WORKSPACE_NOT_FOUND`；
63. Request A/B 并发时分别解析 A/B WorkspaceLease，无共享 Active Workspace、无 Session Binding、互不串线；
64. 不新增 Session Map/Expiry/Persistence/Reconnect Binding，Transport Session 不参与 Workspace Routing；
65. 任意 path 参数只接受 Workspace-relative 语义；Source 保留 `relative_path`、Git 保留 `path`，caller 不能传 absolute/UNC/Root/escape path，绝对路径只由服务端从 Lease 派生；
66. workspaceRegistryRevision 与 per-Workspace generation 语义分离；
67. Agent Execution Workspace 创建时冻结，Continue/UI/其他请求均不能改变；`agent_query`/cancel/continue 不接收新 `workspaceId`，Continue 继承原 Execution Workspace；
68. Git 通过 WorkspaceResolver 后在对应 canonicalRoot 执行；
69. CodeGraph runtime/index 按 Workspace 隔离，资源冲突 fail explicit 而非隐式切换；
70. Serena 每个 live Slot 拥有独立 Process/endpoint/Client，生命周期内不 retarget；容量不足时允许某些 Workspace Slot 非 live；
71. 注册或启动 Desktop 不 eager start Serena/CodeGraph Runtime；
72. 同一 Workspace 并发首次调用通过 single-flight 只启动一个 Runtime；
73. 不同 Workspace Runtime 在 Provider `maxInstances` 允许时可并发；容量不足时走 LRU/BUSY，failure/health 和 Workspace identity 始终隔离；
74. Serena/CodeGraph 都有独立的有界实例数与 Idle Timeout；
75. LRU eviction 只选择 `in_flight == 0` 的 Slot；
76. 容量满且无可驱逐 Slot 时返回 capability busy error，不复用或 retarget 其他 Workspace 的 live Runtime；
77. Idle eviction 保留 Workspace Registry Entry 和 CodeGraph index；
78. Workspace Remove 阻止 runtime acquire/in-flight，并先停止 idle Slot；
79. Runtime stop 失败时 Registry Entry 保留；
80. Capability Health 按 Workspace 展示 stopped/starting/ready/error/stopping；
81. Host shutdown 后无 Serena/CodeGraph orphan process；
82. Workspace 注册不等待 Provider observe/prepare/start/index，成功后立即显示；
83. Capability Readiness 使用动态 Stage/Action DTO，不使用统一 `initialized: bool`；
84. Desktop select/restore 不启动、准备或索引任何 Provider；
85. Serena Project Configuration 缺失时首次 Semantic Tool acquire 可用默认配置自动创建并继续原调用；
86. Serena 自动准备只包含 Project Configuration + Runtime Activation，不执行 Index/Onboarding；
87. Serena Index/Onboarding 独立展示，缺失不阻止 Semantic Runtime ready；
88. CodeGraph readiness 来自 `status --json` 的 initialized/projectPath/index evidence，不仅检查 `.codegraph/` 目录；
89. CodeGraph init/sync/rebuild 只由 Local Human Action 触发，Remote query 不隐式执行；
90. Capability prepare/start/index 通过 single-flight Activity 报告安全进度；
91. `WorkspaceCapabilityProvider` object-safe，Core/Manager/UI 不按 Serena/CodeGraph ID 分支；
92. 新增第三个 built-in Provider 无需修改 Workspace Core、Runtime Slot 或 Health DTO；
93. 新 Provider 仍需编译期注册、Tool Schema allowlist、Prepare side-effect manifest 与独立安全测试；
94. `WorkspaceCapabilityProvider::call` 显式接收服务端 `WorkspaceLease`；
95. `source_read_file` 默认 32 KiB、hard max 128 KiB，截断不改变 full raw SHA；
96. `WorkspaceWriteGuard` 仅以 per-Workspace refcount 阻止 Remove，不串行不同 target path；
97. 同一 canonical target path 的 Source Write Commit 通过 keyed mutex 串行并在锁内重验 SHA/path；
98. Source Write 只声明 Host 内严格 OCC，外部进程修改不声明跨进程事务隔离；
99. 公共 Observe `waitMs` 默认 15000、范围 `0..=20000`；
100. Observe 是最新 Snapshot，可合并或跳过中间 revision；
101. `total_tokens` 只能来自 Provider，不从可能重叠的 breakdown 相加；
102. V0.2 不自动 prune Activity History，查询必须 bounded + cursor；
103. Workspace 交付拆为 Phase 2A.1 Registry、2A.2 Resolver/Authority、2A.3 Capability/Serena；每个 Gate 后公开 Tool 均有 Lease 路由，不出现 Source backend 断档；
104. Serena shared-config 多进程验证失败时阻止 2A.3，并先完成 DCR；若冻结 `maxInstances=1`，其他 Workspace in-flight 时返回 busy，绝不恢复 retarget；
105. Workspace CRUD 复用现有 `SupervisorState.operation` 和 atomic config persist；
106. Serena/CodeGraph 使用 Version Contract/probe，不复制 Codex binary hash pin；
107. Portable/Installed 通过固定 identifier 的相同 config/data path Gate，不新增迁移子系统。

---

# 57. 后续项

以下留到后续版本：

- Claude Provider；
- Claude Code Provider；
- Gemini Provider；
- Provider Plugin Marketplace；
- 第二 Provider Runtime Evidence；
- Semantic Provider abstraction；
- Serena / CodeGraph / LSP 统一语义层；
- Token 成本；
- Usage 报表；
- Updater；
- Authenticode；
- MSI；
- ARM64；
- Offline WebView2 Installer；
- Remote Source Write 高级授权策略；
- Source Write ADS 完整保持；
- MCP Server Push；
- 实时 Terminal Stream；
- Command/Test Evidence Registry。

---

# 58. 最终架构定位

完成 V0.2 revision003 后：

```text
SerenaDesktop
│
├── Desktop Core
│
├── Workspace Registry
│   ├── Manual Directory Registration
│   ├── Rename
│   ├── Remove
│   ├── Reorder
│   └── Serena Import (optional)
│
├── Workspace Context
│   ├── DesktopSelectedWorkspace (UI only)
│   ├── Workspace Discovery (workspace_list/query; no binding)
│   ├── Request.workspaceId (execution authority)
│   ├── WorkspaceResolver
│   ├── WorkspaceLease (workspaceId/canonicalRoot/generation)
│   ├── WorkspacePathResolver (relative path only)
│   └── Immutable Execution Workspace Snapshot
│
├── Workspace Capability Registry
│   └── Arc<dyn WorkspaceCapabilityProvider>
│       ├── Source Adapter (in-process)
│       ├── Git Adapter (stateless)
│       ├── Serena Adapter (built-in)
│       ├── CodeGraph Adapter (built-in)
│       └── Future Built-in Adapters
│
├── Workspace Capability Manager
│   └── (providerId, workspaceId, generation)
│       └── lazy bounded Runtime Slot
│
├── MCP Broker
│   └── Transport Session (lifecycle/log/diagnostics/rate-limit only)
├── Remote / OAuth
│
├── Rust Source
│   ├── Read
│   ├── List
│   ├── Find
│   ├── Search
│   └── Safe Write
│
├── Work Orchestration
│
├── Agent Control Plane
│   ├── Work
│   ├── Execution
│   ├── Provider Registry
│   ├── Activity
│   ├── Usage
│   └── Product
│
├── Agent Providers
│   └── Codex
│       ├── App Server
│       ├── Runtime
│       ├── Windows Job
│       ├── Thread / Turn
│       ├── Recovery
│       └── Evidence
│
└── Optional Workspace Capabilities
    ├── Serena Semantic
    └── CodeGraph
```

核心变化不是“用 Rust 重写 Serena”。

而是：

> **SerenaDesktop 正式从 Serena 的桌面管理器，演化为拥有自身 Workspace Registry、Request/Execution Scoped Workspace Context、Workspace-scoped Capability Runtime 和 Provider-Agnostic Agent Control Plane 的本地 Agent Host。**

Serena、Codex、CodeGraph 都只是能力提供方，不再决定 SerenaDesktop Core 能不能运行。
