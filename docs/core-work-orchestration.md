# SerenaDesktop Work Orchestration V0.1

目标不是再造一个 TaskQuay，而是在现有 Agent Runtime 上增加一层：

```text
Work
  ↓
Agent Executions
  ↓
Result / Evidence
  ↓
Acceptance
```

底层已经比较成熟的 `Execution → Runtime → Thread → Turn → Claim → Recovery` 完全不重新设计。你现有设计已经把进程归属、Crash Recovery、幂等、Workspace Claim、安全终态等做得比 LocalWorks 更严格。

LocalWorks 值得吸收的是它的 Host-first、Work Run、Query/Execute 分离和版本化上下文。

---

## 一、先确定 V0.1 边界

这次只做四件事：

| 能力                       | V0.1 |
| ------------------------ | ---- |
| Work Run 顶层任务            | 做    |
| Agent Query / Execute 分离 | 做    |
| ChatGPT → Agent 版本化上下文   | 做    |
| Work Acceptance 收口       | 做    |
| ChatGPT 直接写文件            | 不做   |
| 通用 Shell / command_start | 不做   |
| 完整 Workflow DAG          | 不做   |
| 多 Agent 自动规划             | 不做   |
| 新调度器                     | 不做   |

这样最终架构是：

```text
ChatGPT
  │
  ├── Serena / CodeGraph / Git
  │       ↓
  │   读取、分析、规划
  │
  ├── work_update
  │       ↓
  │    Work Run
  │
  ├── agent_execute
  │       ↓
  │ AgentTaskManager
  │       ↓
  │ Existing Execution Runtime
  │
  ├── agent_query
  │       ↓
  │  Observe Result
  │
  └── work_update(finish)
          ↓
       Acceptance
```

最重要的架构约束：

> **Work 是业务任务容器，Execution 才是实际执行单元。**

不要把两个概念混在一起。

---

# 二、公开 MCP 工具重新整理成 4 个

我建议最终新增/调整为：

| Tool            | 类型   | 作用                                        |
| --------------- | ---- | ----------------------------------------- |
| `work_query`    | 只读   | 查询 Work                                   |
| `work_update`   | 有副作用 | 创建/完成/取消 Work                             |
| `agent_query`   | 只读   | 查询/观察 Execution                           |
| `agent_execute` | 有副作用 | start / continue / cancel / resumePending |

这是 LocalWorks 设计里非常值得照搬的一点：它明确把 observation 和 execution 拆开。

不要做：

```text
agent
  action=start
  action=get
  action=list
  action=cancel
  action=observe
  action=continue
  ...
```

这种万能工具长期会越来越难给 ChatGPT 描述权限语义。

---

# 三、`work_update`

建议 Schema 只支持三个 action：

```text
begin
finish
cancel
```

## begin

输入：

```json
{
  "action": "begin",
  "workspaceId": "...",
  "title": "修复 MCP reconnect 后任务状态不同步",
  "goal": "定位问题、完成修复并通过相关测试"
}
```

返回：

```json
{
  "workRunId": "wrk_xxx",
  "status": "active",
  "workspaceId": "...",
  "revision": 1
}
```

### Work 不持有 Workspace Claim

这一点非常重要。

现在：

```text
Execution
    ↓
Workspace Claim
```

继续保持。

不要改成：

```text
Work
    ↓
Workspace Claim
```

否则一个 Work 内多个顺序 Execution 会和当前 Claim 模型冲突。

Work 只是逻辑聚合。

---

# 四、Work 状态只保留四个

不要设计复杂工作流状态机。

```text
active
completed
failed
cancelled
```

正常路径：

```text
begin
 ↓
active
 ↓
agent execution 1
 ↓
agent execution 2
 ↓
verification
 ↓
finish
 ↓
completed
```

失败：

```text
active → failed
```

用户主动终止：

```text
active → cancelled
```

不要现在增加：

```text
planning
implementing
testing
reviewing
acceptance_pending
paused
...
```

这些属于未来 Workflow Engine，不属于第一版。

---

# 五、数据库不要侵入现有 Execution 核心表

你的 `executions` 表现在承载大量安全不变量和 trigger。

我不建议为了 WorkRun 大幅修改它。

新增两张表即可。

```sql
CREATE TABLE work_runs (
    id TEXT PRIMARY KEY NOT NULL,

    workspace_id TEXT NOT NULL,
    canonical_workspace_root TEXT NOT NULL,

    title TEXT NOT NULL,
    goal TEXT,

    status TEXT NOT NULL CHECK (
        status IN ('active', 'completed', 'failed', 'cancelled')
    ),

    revision INTEGER NOT NULL DEFAULT 0,

    acceptance_json TEXT,

    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER
);
```

再增加：

```sql
CREATE TABLE work_execution_links (
    work_run_id TEXT NOT NULL,
    execution_id TEXT NOT NULL UNIQUE,

    parent_execution_id TEXT,

    delegation_context_json TEXT,

    created_at INTEGER NOT NULL,

    PRIMARY KEY(work_run_id, execution_id),

    FOREIGN KEY(work_run_id)
        REFERENCES work_runs(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(execution_id)
        REFERENCES executions(id)
        ON DELETE RESTRICT
);
```

这样关系就是：

```text
WorkRun W1

├── Execution E1
├── Execution E2
└── Execution E3
```

而：

```text
Execution Runtime
Workspace Claim
Provider Evidence
Final Result
```

全部保持原样。

---

# 六、Execution 创建和 Work Link 必须原子

这个细节很重要。

不能：

```text
create Execution
COMMIT

↓

insert work_execution_links
```

否则 Crash 后可能出现：

```text
Execution 已存在甚至开始执行
但没有 WorkRun 归属
```

正确方式：

```text
BEGIN IMMEDIATE

验证 WorkRun = active

创建 Execution

创建 Workspace Claim

INSERT work_execution_links

COMMIT

↓

才允许 Provider Dispatch
```

也就是说现有：

```text
create_execution(...)
```

增加一个可选：

```text
work_context
```

但不要复制一套新的 Execution 创建逻辑。

---

# 七、`agent_execute`

建议：

```text
start
continue
cancel
resume_pending
```

### start

```json
{
  "action": "start",
  "workRunId": "wrk_123",
  "requestKey": "fix-reconnect-v1",
  "prompt": "实现已经确认的修复方案",
  "context": {
    "summary": "ChatGPT 已确认 reconnect 后恢复逻辑存在状态覆盖问题。",
    "files": [
      {
        "path": "src-tauri/src/agent/task_manager.rs",
        "sha256": "..."
      }
    ]
  }
}
```

返回应该很快：

```json
{
  "executionId": "exec_123",
  "status": "dispatch_pending",
  "revision": 1
}
```

不要等待 Codex 完成。

---

# 八、continue 不得重新打开旧 Execution

这是一个特别容易实现错的地方。

假设：

```text
E1 = completed
```

ChatGPT 发现还需要：

```text
“再补一个测试”
```

不能：

```text
E1 completed → running
```

你现有设计已经明确 terminal Execution 是 absorbing state。

正确模型：

```text
Work W1

E1
completed
   │
   │ continue
   ▼
E2
running
```

E2 可以：

```text
复用 E1 对应 Codex Thread
```

但必须是：

```text
新的 Execution
```

因此：

```text
agent_execute {
    action: "continue",
    workRunId,
    parentExecutionId,
    requestKey,
    prompt
}
```

内部：

```text
reuse Thread
+
create New Execution
```

而不是修改旧 Execution。

---

# 九、`agent_query`

我建议支持：

```text
get
list
observe
```

其中真正重要的是：

```text
observe
```

例如：

```json
{
  "action": "observe",
  "executionId": "exec_123",
  "knownRevision": 7,
  "waitMs": 15000
}
```

行为：

```text
revision > 7
    ↓
立即返回

没有变化
    ↓
最多等待 15 秒
    ↓
返回 unchanged
```

这样 ChatGPT 不需要：

```text
每秒 agent_status
每秒 agent_status
每秒 agent_status
```

这和 TaskQuay/LocalWorks 的 bounded observation 思路一致。

建议最大：

```text
waitMs <= 20_000
```

MCP 调用依然是短生命周期。

---

# 十、Host-first 最关键的一步：文件版本引用

这是我认为这次最值得实现的增强。

把现有：

```text
source_read_file
```

返回扩展为：

```json
{
  "path": "...",
  "content": "...",
  "sha256": "...",
  "truncated": false
}
```

注意：

> SHA256 应计算整个文件原始内容，而不是只 hash 返回给 ChatGPT 的截断内容。

于是流程变成：

```text
ChatGPT
 ↓
source_read_file
 ↓
A.rs
sha256 = AAA

 ↓
ChatGPT 分析

 ↓
agent_execute
context.files:
A.rs = AAA
```

Agent 真正 Dispatch 之前：

```text
重新计算 A.rs SHA256
```

如果已经变成：

```text
BBB
```

则：

```text
CONTEXT_STALE
```

并且：

```text
Provider Turn 不得启动
```

LocalWorks 也是使用完整文件 SHA-256 来解决 Host 分析和 Worker 执行之间的上下文漂移。

这个机制非常适合你现在：

```text
ChatGPT = 大脑
Codex = 执行器
```

的架构。

---

# 十一、Agent 上下文不要把大量源码复制进 Prompt

推荐结构：

```text
ChatGPT 提供：

summary
+
文件引用 path + sha256
+
验收要求
```

例如：

```text
Host verified context:

Summary:
Reconnect recovery incorrectly overwrites terminal execution state.

Versioned source references:
- src/agent/task_manager.rs @ SHA256 AAA
- src/agent/store.rs @ SHA256 BBB

Task:
Implement the smallest fix...

Acceptance:
...
```

Codex 自己仍然可以读取 Workspace。

不要把：

```text
整个 ChatGPT 对话
几十 KB 源码
所有 Serena 搜索结果
```

一股脑塞给 Codex。

这正是 LocalWorks Host-first 优化的核心目的。

---

# 十二、Work Finish

调用：

```json
{
  "action": "finish",
  "workRunId": "wrk_123",
  "outcome": "completed",
  "acceptance": {
    "summary": "修复完成，相关单元测试和集成测试通过。",
    "executionIds": [
      "exec_1",
      "exec_2"
    ]
  }
}
```

服务端必须验证：

```text
WorkRun == active

所有关联 Execution：
    completed / failed / cancelled / interrupted
```

只要存在：

```text
dispatch_pending
running
cancel_requested
cancelling
finalizing
reconciling
unknown
```

就返回：

```text
WORK_HAS_ACTIVE_EXECUTIONS
```

不能结束 Work。

尤其：

```text
unknown
```

必须视为 unresolved。

这和你当前 fail-closed 原则保持一致。

---

# 十三、Acceptance V0.1 不要假装成自动验证系统

这一点需要克制。

因为当前没有：

```text
command_start
command receipt
test result registry
```

所以不能把：

```text
ChatGPT 说“测试通过”
```

包装成：

```text
SYSTEM_VERIFIED_TEST_PASS
```

第一版只保存：

```text
Host Acceptance
```

例如：

```json
{
  "decision": "accepted",
  "summary": "...",
  "executionIds": ["..."],
  "acceptedAt": 123456
}
```

以后真的增加：

```text
CommandSession
TestEvidence
Artifact
```

再升级成机器可验证 Evidence。

LocalWorks 的完整 Work receipt 是建立在它同时控制 command、agent、file operation 的基础上的。

SerenaDesktop 第一版不要假装已经拥有同等证据强度。

---

# 十四、错误码建议冻结

| Code                         | 含义                      |
| ---------------------------- | ----------------------- |
| `WORK_NOT_FOUND`             | Work 不存在                |
| `WORK_NOT_ACTIVE`            | Work 已结束                |
| `WORK_HAS_ACTIVE_EXECUTIONS` | 有未决 Execution           |
| `WORKSPACE_CONTEXT_MISMATCH` | Work 与当前 Workspace 不一致  |
| `EXECUTION_NOT_IN_WORK`      | Execution 不属于该 Work     |
| `CONTEXT_STALE`              | Host 引用源码已变化            |
| `WORK_ACCEPTANCE_REQUIRED`   | completed 缺少 acceptance |

不要根据错误文本做控制逻辑。

---

# 十五、Tool Schema 要同时做这件事

所有新增工具直接带：

```text
inputSchema
outputSchema
annotations
```

尤其：

```text
work_query
agent_query
```

明确：

```text
readOnlyHint = true
```

而：

```text
work_update
agent_execute
```

明确是 mutation。

LocalWorks 的 web tools 也专门给所有工具补了 `outputSchema`。

这能明显改善 ChatGPT 的工具选择。

---

# 十六、不要再建 Provider Pipeline

Codex 实现时最需要强调：

```text
agent_execute
```

只是：

```text
MCP Adapter
       ↓
AgentTaskManager
       ↓
现有 Provider Pipeline
```

禁止出现：

```text
agent_execute
  ↓
直接调用 CodexAppServerClient

或

新的 CodexRuntime
```

必须完整复用当前：

```text
AgentTaskManager
CodexProvider
CodexAppServerClient
WorkspaceExecutionCoordinator
StateStore
```

以及已经实现的：

```text
networkAccess
AGENTS.md
Codex global instructions
Execution Profile
Job Object
Cancellation
Recovery
```

这些能力。

你的 V0.4 Runtime 本身就是底座。

---

# 十七、建议实施顺序

我建议让 Codex 按下面顺序做，而且每个阶段独立 Review：

| Phase | 内容                                      |
| ----- | --------------------------------------- |
| 1     | WorkRun State Store                     |
| 2     | `work_query / work_update`              |
| 3     | `agent_query / agent_execute` Adapter   |
| 4     | Execution ↔ Work 原子绑定                   |
| 5     | `source_read_file.sha256`               |
| 6     | Agent versioned context 校验              |
| 7     | Work Finish / Acceptance                |
| 8     | MCP Schema / outputSchema / annotations |
| 9     | Restart / Crash / E2E                   |

**不要先改 UI。**

先把 MCP contract 和 StateStore 跑通。

---

# 十八、必须覆盖的测试矩阵

核心验收至少做到下面这些：

| 场景                          | 预期                                |
| --------------------------- | --------------------------------- |
| begin work                  | active                            |
| restart 后 query work        | 仍存在                               |
| start Agent                 | Execution 与 Work 原子关联             |
| 创建 Execution 后事务失败          | 不 Dispatch                        |
| 相同 requestKey 重试            | 不产生第二次 Provider 调用                |
| continue terminal Execution | 创建 E2，不修改 E1                      |
| cancel                      | 复用现有 cancellation pipeline        |
| observe revision 变化         | 提前返回                              |
| observe 无变化                 | bounded timeout                   |
| source SHA 未变化              | 可以 Dispatch                       |
| SHA 已变化                     | CONTEXT_STALE，0 Provider Turn     |
| Work 有 running Execution    | finish 拒绝                         |
| Work 有 unknown Execution    | finish 拒绝                         |
| 所有 Execution terminal       | 可以 finish                         |
| Work completed 后 start      | WORK_NOT_ACTIVE                   |
| Workspace 已切换               | WORKSPACE_CONTEXT_MISMATCH        |
| Host Crash                  | Existing Execution Recovery 行为不改变 |
| `tools/list`                | 四个工具 Schema 与 annotation 正确       |

最后真实 ChatGPT 验收一次：

```text
work begin
 ↓
source_read_file
 ↓
ChatGPT 分析
 ↓
agent_execute start
 ↓
agent_query observe
 ↓
agent_execute continue
 ↓
agent_query
 ↓
git_diff / source review
 ↓
work finish
```

这条完整跑通才算完成。

---

# 十九、最终产品语义

做完以后，SerenaDesktop 的职责会很清楚：

```text
                    ChatGPT
                       │
          Planning / Reasoning / Review
                       │
        ┌──────────────┴──────────────┐
        │                             │
        ▼                             ▼
 Perception Plane              Orchestration Plane

 Serena                        Work Run
 CodeGraph                     Agent Query
 Git                           Agent Execute
 Source SHA                         │
        │                            ▼
        │                    Execution Runtime
        │                            │
        └──────────────┬─────────────┘
                       ▼
                    Evidence
                       │
                       ▼
                   Acceptance
```

这样你的产品就不再只是：

> “ChatGPT 可以远程启动 Codex。”

而会升级成：

> **ChatGPT 负责理解、规划、决策和验收；SerenaDesktop 提供本地感知、任务编排和可靠 Agent 执行。**
