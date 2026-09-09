# SerenaDesktop Agent 异步观察协议技术方案

## 1. 目标

在现有 SerenaDesktop Agent Runtime Foundation 之上，补齐长时间 Agent Execution 与 ChatGPT 之间的异步交互能力。

目标交互模型：

```text
ChatGPT
   │
   │ agent(start)
   ▼
SerenaDesktop
   │
   ├─ 创建 Execution
   ├─ 获取 Workspace Claim
   ├─ 将 Execution 交给 Host-owned Worker
   └─ 立即返回 Execution Receipt
   │
   ▼
ChatGPT ← executionId / status / revision

           Codex 后台继续执行
                  │
                  ▼
          Runtime / Thread / Turn
                  │
                  ▼
            最终结果持久化


ChatGPT
   │
   │ agent(observe,
   │       knownRevision,
   │       waitMs=20000)
   ▼
SerenaDesktop
   │
   ├─ 有变化 → 立即返回
   └─ 无变化 → 最多等待 20 秒后返回
```

核心原则：

> MCP Tool Call 生命周期与 Codex Execution 生命周期彻底解耦。

ChatGPT 不需要等待一个可能持续几分钟甚至几十分钟的 Codex Turn。

---

# 2. 对齐 TaskQuay 的范围

TaskQuay 当前 `agent_task` 已明确提供：

```text
waitMs
knownRevision
includeResponse
```

其中 `waitMs` 最大 25 秒，默认 20 秒；`knownRevision` 是任务/进度变化 token；`includeResponse` 用来显式读取 terminal response。

它的 `observe` 会循环检查状态：

```text
状态变化
→ 立即返回

达到 deadline
→ 返回 unchanged

任务 terminal
→ 立即返回
```

而不是保持一个 MCP 请求直到 Agent 完成。

TaskQuay 的后台执行同样与调用方分离：先建立受管 Promise，登记到 `activeTurns`，随后立即返回当前 record；真正的 `runTurn()` 在后台执行。

SerenaDesktop 本轮只对齐这一交互层。

明确不引入 TaskQuay 的：

```text
work_task
usage
claims action
contextKey
freshContext
多 Agent 调度
资源队列
Token 统计
Acceptance 工作台
```

这些不是当前问题所需能力。

---

# 3. 当前 SerenaDesktop 基线

当前架构已经完成最重要的一半：

```text
AgentProductService
        ↓
AgentTaskManager
        ↓
host-owned tokio task
        ↓
CodexProvider
        ↓
Codex Runtime
```

`start/continue` 已经通过 receipt 与后台 worker 解耦，不需要重新设计 Runtime。

现有以下能力全部保持：

```text
Execution persistence
Workspace Claim
requestKey idempotency
Agent lineage
Runtime ownership
Windows Job Object
Provider terminal evidence
Background cleanup
Result recovery
Crash recovery
PendingExplicitResume
Cancellation
Atomic Claim Release
unknown fail-closed
```

这些属于已经冻结的 Runtime Foundation，不应由 Observe 协议重新设计。

本轮主要缺口是：

```text
observe = 单次数据库读取
```

需要升级成：

```text
observe = 可选有界 long-poll + revision + result-on-demand
```

---

# 4. MCP Agent Tool 保持单工具设计

继续只公开：

```text
agent
```

Action 集合不变：

```text
start
continue
resume_pending
observe
cancel
list
```

不新增：

```text
wait
poll
result
status
stream
subscribe
```

避免工具数量膨胀。

所有长任务观察能力收敛到：

```text
observe
```

---

# 5. Observe 输入契约

现有：

```json
{
  "action": "observe",
  "executionId": "E1"
}
```

扩展为：

```json
{
  "action": "observe",
  "executionId": "E1",
  "knownRevision": "opaque-token",
  "waitMs": 20000,
  "includeResult": false
}
```

字段定义：

| 字段            | 类型       | 语义                               |
| ------------- | -------- | -------------------------------- |
| executionId   | string   | 精确 Execution 身份                  |
| knownRevision | string?  | 调用方上次看到的观察版本                     |
| waitMs        | integer? | 有界等待，默认 20000，范围 0..25000        |
| includeResult | boolean? | 是否显式获取已持久化 Final Result，默认 false |

`waitMs=0` 表示普通即时读取。

---

# 6. Revision 设计

这里不建议直接公开当前数据库中的：

```text
executions.revision
```

虽然 SerenaDesktop 已经存在这个字段，但它目前承担内部：

```text
CAS
状态转换并发控制
Evidence identity
```

职责。

Product Protocol 不应该让 ChatGPT 依赖这个安全内部字段。

因此新增产品层：

```text
observationRevision
```

它是 opaque token，例如：

```text
SHA-256(
    status
    dispatchState
    providerTerminalStatus
    resultCompleteness
    resultAvailable
    interruptRequested
    interruptAcknowledged
    interruptTimedOut
    attention
    availableActions
    progressPhase
    progressToolCategory
)
```

调用方只能：

```text
保存
比较
原样传回
```

不能解释 token 内容。

## 不参与 revision 的字段

以下变化不得唤醒 long-poll：

```text
updatedAt
elapsed time
heartbeat
日志增长
累计 Token
普通 runtime diagnostic
UI refresh timestamp
```

否则执行一个 30 分钟 build 时，大量无意义更新会不断唤醒 ChatGPT。

这正是 TaskQuay 当前的重要约束：累计 usage、更新时间和经过时长本身不触发 revision 变化。

---

# 7. Observe 返回契约

建议：

```json
{
  "ok": true,
  "data": {
    "executionId": "E1",

    "status": "running",
    "dispatchState": "dispatched",

    "revision": "79ab...",
    "unchanged": false,

    "resultAvailable": false,

    "progress": {
      "phase": "running",
      "toolCategory": "test"
    },

    "nextAction": {
      "action": "observe",
      "waitMs": 20000
    },

    "...existingExecutionFields": "..."
  }
}
```

当任务完成：

```json
{
  "status": "completed",
  "revision": "ab91...",
  "resultAvailable": true,

  "nextAction": {
    "action": "review_result",
    "includeResult": true
  }
}
```

默认不返回完整：

```text
finalResult
```

---

# 8. Final Result 按需获取

这是本轮第二个关键变化。

当前 ExecutionView 会直接携带：

```text
finalResult
```

调整为：

```text
includeResult=false
→ 不附完整 finalResult

includeResult=true
→ 返回持久化 finalResult
```

同时始终返回：

```text
resultAvailable
resultCompleteness
```

例如：

```json
{
  "status": "completed",
  "resultAvailable": true,
  "resultCompleteness": "complete"
}
```

ChatGPT 判断需要结果后：

```json
{
  "action": "observe",
  "executionId": "E1",
  "waitMs": 0,
  "includeResult": true
}
```

再拿结果。

这样可以显著降低长任务观察过程中的上下文消耗。

TaskQuay 当前也是 terminal 后先暴露 `responseAvailable`，只有 `includeResponse=true` 才真正附加结果。

---

# 9. Result 必须是可重复读取的

`includeResult` 是纯读取操作。

禁止：

```text
读取后删除
标记 consumed
移动 cursor
触发新的 Provider 请求
thread/resume
重新执行 Turn
```

必须满足：

```text
observe(includeResult=true)
observe(includeResult=true)
observe(includeResult=true)
```

都可以取得相同的 persisted Final Result。

并且：

```text
knownRevision == currentRevision
```

也不能阻止调用方显式读取结果。

TaskQuay 专门测试了这一点：断线、重新建立 MCP Client 后，用相同 revision 再次 `includeResponse=true`，仍能读取原 terminal result，而且不会再次启动模型。

SerenaDesktop 必须具有同样语义。

---

# 10. Long Poll 算法

第一版不建议新增 EventBus。

直接采用 Product Layer bounded polling。

伪代码：

```text
observe(executionId, knownRevision, waitMs, includeResult)

deadline =
    now + clamp(waitMs ?? 20000, 0, 25000)

loop:

    snapshot = StateStore.product_read(executionId)

    currentRevision =
        observation_revision(snapshot)

    if terminal(snapshot):
        return observation

    if knownRevision exists
       AND currentRevision != knownRevision:
        return observation

    if now >= deadline:
        return observation {
            unchanged =
                currentRevision == knownRevision
        }

    sleep <= 500ms
```

建议：

```text
poll interval = 500 ms
```

原因：

* 本机 SQLite；
* 单 Execution 查询成本极低；
* 最大等待仅 25 秒；
* 无需侵入所有 Foundation transaction；
* 不会给 StateStore 增加 Notify/EventBus 生命周期；
* 行为与 TaskQuay 当前 bounded polling 接近。

后续如果 Agent 数量明显增大，再升级：

```text
StateStore commit
→ ExecutionObserverHub
→ tokio::watch
```

第一版没必要。

---

# 11. MCP 连接断开语义

这是非常重要的不变量：

```text
MCP observe future 被取消
≠
Execution 被取消
```

如果发生：

```text
浏览器刷新
Cloudflare stream 断开
ChatGPT Tool Call 取消等待
网络断开
```

只终止当前：

```text
observe
```

不得：

```text
cancel Execution
Terminate Runtime
interrupt Turn
release Claim
```

Codex 后台继续运行。

之后新的 ChatGPT Tool Call：

```text
observe(E1)
```

直接读取 StateStore。

---

# 12. ChatGPT 推荐交互流程

## Start

```text
ChatGPT
   ↓
agent(start)
```

返回：

```text
executionId = E1
status = running
revision = R1
```

ChatGPT不等待整个任务。

---

## Running

```text
agent(
    action=observe,
    executionId=E1,
    knownRevision=R1,
    waitMs=20000
)
```

### 20 秒无变化

返回：

```text
status=running
revision=R1
unchanged=true
```

### 中途状态变化

例如：

```text
running
→ finalizing
```

立即返回：

```text
revision=R2
unchanged=false
```

---

## Terminal

返回：

```text
status=completed
revision=R3
resultAvailable=true
```

随后：

```text
observe(
    executionId=E1,
    includeResult=true,
    waitMs=0
)
```

读取 Final Result。

---

# 13. ChatGPT 不需要无限 observe

产品协议只提供能力，不应该强迫 ChatGPT：

```text
observe
observe
observe
observe
...
```

工具描述应告诉 ChatGPT：

> 对耗时任务使用有界 observe；当前回复没有必要持续等待时，可以保留 executionId，稍后通过 observe/list 恢复状态。

因此存在两种合法流程：

### 当前回答内等待

```text
start
↓
observe 20s
↓
任务很快完成
↓
Review
```

### 长任务

```text
start
↓
observe 20s
↓
仍 running
↓
ChatGPT 告诉用户任务仍在执行
```

后续用户询问：

```text
“刚才的任务怎么样了？”
```

ChatGPT：

```text
observe(E1)
```

继续获取结果。

不需要保持原 MCP 连接。

---

# 14. Progress 能力

TaskQuay 当前还提供非常克制的 progress：

```text
phase
toolCategory
```

并明确不返回：

```text
具体命令
stdout
模型思维
```

SerenaDesktop 建议同样采用这种模型。

第一版 progress：

```text
phase:

dispatching
running
finalizing
reconciling
terminal
```

直接由现有 Execution 状态推导，不新增数据库字段。

如果固定 Codex App Server Schema 能可靠识别工具活动，可以再增加：

```text
toolCategory:

file
command
build
test
other
```

但必须基于当前固定 Codex 版本的真实 Notification Contract。

不能通过：

```text
命令字符串猜测
模型输出文本猜测
```

来产生安全或业务事实。

因此：

### Phase A

先上线：

```text
progress.phase
```

### Phase B

完成固定 Codex Notification Contract 验证后，再增加：

```text
progress.toolCategory
```

---

# 15. nextAction

为了降低 ChatGPT 自己推断生命周期的成本，增加：

```text
nextAction
```

它只是产品提示，不是安全授权。

映射建议：

| 当前状态                        | nextAction        |
| --------------------------- | ----------------- |
| dispatch_pending/running    | observe           |
| cancel_requested/cancelling | observe           |
| finalizing                  | observe           |
| reconciling                 | observe           |
| terminal + resultAvailable  | review_result     |
| pending_explicit_resume     | resume_pending    |
| manual_resolution_required  | manual_resolution |

例如：

```json
{
  "nextAction": {
    "action": "observe",
    "waitMs": 20000
  }
}
```

或者：

```json
{
  "nextAction": {
    "action": "review_result",
    "includeResult": true
  }
}
```

ChatGPT不需要重新理解全部内部状态机。

---

# 16. Product DTO Amendment

现有：

```text
Action::Observe {
    execution_id
}
```

调整：

```text
Action::Observe {
    execution_id,
    known_revision: Option<String>,
    wait_ms: Option<u32>,
    include_result: Option<bool>,
}
```

Execution Product View 新增：

```text
revision: String
unchanged: bool       # observe only
resultAvailable: bool
progress: Progress
nextAction: NextAction
```

`finalResult` 调整为：

```text
includeResult == true
    → Some(...)

否则
    → None
```

Start / Continue / Resume 的返回中也应包含：

```text
revision
resultAvailable
progress
nextAction
```

这样 ChatGPT 在 `start` 后即可直接携带 revision 进入第一次 long-poll。

---

# 17. 不暴露内部 CAS Revision

当前代码存在：

```text
ExecutionRecord.revision: i64
```

现有 Product Test 还明确要求：

```text
revision
```

不进入 Product View。

这个设计原则继续保留：

```text
Internal execution revision
≠
Product observation revision
```

因此应删除/修改的是：

```text
“Product 不允许任何 revision”
```

这一旧契约，

而不是直接把数据库：

```text
revision = 17
```

返回给 ChatGPT。

Product 返回的是：

```text
revision = opaque semantic token
```

这样不会把 Runtime Foundation 的 optimistic concurrency contract 暴露成公共 API。

---

# 18. 代码修改范围

预计主要涉及：

```text
src-tauri/src/agent/product.rs
```

负责：

```text
Observe DTO
ObservationRevision
long-poll
includeResult
resultAvailable
nextAction
progress projection
```

```text
src-tauri/src/agent/store/transactions/product.rs
```

只补高效 exact Execution snapshot 查询。

原则上：

```text
无需 Schema Migration
```

```text
src-tauri/src/mcp/registry.rs
```

增加 flat compatibility schema：

```text
knownRevision
waitMs
includeResult
```

并更新 Agent lifecycle 描述。

Tauri：

```text
agent_operation
```

继续复用同一 Action DTO，不建立第二套协议。

---

# 19. 明确禁止改动

本轮不得改变：

```text
Execution 11-state graph
Dispatch State
RuntimeInstance ownership
Workspace Claim
Job Object
Runtime termination evidence
Provider Terminal Evidence
Final Result Recovery
HistoryMode contract
Background Terminal Cleanup
Cancellation attribution
PendingExplicitResume
Cross-Runtime Recovery
Atomic Claim Release
requestKey idempotency
Agent lineage
workspace_write contract
```

Observe 永远只是：

```text
read/wait/read
```

不具有任何：

```text
Provider side effect
Workspace side effect
Recovery side effect
```

---

# 20. 测试矩阵

至少覆盖：

### Long Poll

```text
knownRevision 相同
+
waitMs=100
+
无变化
→ bounded timeout
→ unchanged=true
```

```text
knownRevision=R1
+
执行状态 R1→R2
→ deadline 前立即返回
```

```text
waitMs > 25000
→ AGENT_INVALID_ARGUMENT
```

### Result

```text
terminal
+
includeResult=false
→ resultAvailable=true
→ finalResult absent
```

```text
terminal
+
includeResult=true
→ exact persisted finalResult
```

```text
重复 includeResult
→ 相同结果
→ 不启动 Runtime
→ 不产生 Turn
```

### Reconnect

```text
Execution terminal
↓
MCP Client disconnect
↓
new MCP Client
↓
observe(includeResult=true)
→ exact persisted result
```

### Revision

以下必须改变 revision：

```text
running → finalizing
finalizing → completed
resultCompleteness partial → complete
cancel requested
attention change
meaningful progress phase change
```

以下不得改变：

```text
updatedAt only
elapsed time
heartbeat
日志追加
重复 observe
```

### Safety

```text
observe timeout
→ Claim 不变
```

```text
observe future 被 drop
→ Runtime 不变
→ Execution 继续
```

```text
includeResult
→ 不 thread/resume
→ 不创建 Runtime
→ 不调用 Provider
```

---

# 21. 验收标准

全部满足才可认为：

```text
Agent Async Interaction = PASS
```

必须证明：

```text
1. start/continue 不等待 Codex terminal。

2. 一个 10 分钟 Execution 不要求一个 10 分钟 MCP 请求。

3. observe 默认 20 秒、最大 25 秒 bounded long-poll。

4. revision 未变化时返回 unchanged=true。

5. meaningful state change 可提前唤醒 observe。

6. result 默认不进入普通 observe。

7. includeResult 可重复取得 persisted Final Result。

8. MCP 断线后可由新的 MCP Session 继续 observe。

9. observe / includeResult 不产生新的 Provider 推理。

10. observe 中断不取消后台 Execution。

11. Runtime Foundation、Claim、Recovery、Cancellation 和 Atomic Release 全部无回归。
```

---

# 22. 实施顺序

建议分四步：

```text
Stage 1
Product Contract Amendment
    ↓
Observe DTO
revision semantics
result-on-demand semantics

Stage 2
bounded long-poll
    ↓
waitMs
knownRevision
unchanged

Stage 3
result retrieval
    ↓
resultAvailable
includeResult
reconnect test

Stage 4
Product ergonomics
    ↓
progress.phase
nextAction
MCP description
真实 ChatGPT + Cloudflare 验收
```

其中：

```text
workspace_write Amendment
```

和本方案保持独立。

先分别通过 Contract Review，再按仓库现有 Gate 实施，不应一次修改两套 Contract 后再混合验收。

---

# 23. 最终产品模型

完成后 SerenaDesktop 的职责关系会非常清楚：

```text
                    ChatGPT
                       │
         ┌─────────────┴─────────────┐
         │                           │
         ▼                           ▼
 source/git/codegraph/media       agent(start)
         │                           │
         │                           ▼
   ChatGPT 自己分析            Codex 后台执行
                                     │
                                     │
                         ┌───────────┴───────────┐
                         │                       │
                         ▼                       ▼
                    observe 20s            持久化结果
                         │                       │
                         └───────────┬───────────┘
                                     ▼
                              includeResult
                                     │
                                     ▼
                                  ChatGPT
                                     │
                                     ▼
                          git_diff/source Review
```

最终原则：

> ChatGPT 掌握控制权；Agent 的执行可以很久，但任何一次 MCP 调用都必须是短生命周期、可恢复、可重入的控制面操作。
