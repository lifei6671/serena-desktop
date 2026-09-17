# SerenaDesktop ChatGPT 控制恢复与 Agent 完成提醒技术方案 V0.2

## 1. 背景

当前已经观察到一种真实的 ChatGPT Web 行为：

```text
浏览器显示：
“消息发送超时，请重试”
或
“连接已中断，正在等待完整回复”

与此同时：
OpenAI 服务端旧 Turn 仍可能继续调用 SerenaDesktop MCP
Codex Execution 仍可能继续运行并最终完成
```

因此，SerenaDesktop 不能把浏览器页面、ChatGPT 回复流、MCP Transport Session 或某一次 Tool Call 的生命周期视为 Work / Execution 生命周期 Authority。

另一方面，当 Codex 已经完成任务后，如果 ChatGPT 因前端流中断、Turn 超时或其他原因一直没有回来读取最终结果，用户目前无法感知，只能长时间盯着 ChatGPT 页面等待。

本方案同时解决两个问题：

1. ChatGPT 当前回复丢失后，新的 ChatGPT Turn 可以重新发现原 Work 和 Execution，并安全继续控制；
2. Codex Execution 已完成、但 ChatGPT 长时间没有继续消费结果时，由 SerenaDesktop 发出本地声音和系统通知，引导用户回 ChatGPT 查看会话状态。

核心模型：

```text
ChatGPT Turn / Browser Connection
        可丢失
        可重连
        可短暂并存
        不属于业务 Authority

Work / Execution
        Durable
        Idempotent
        Recoverable
        Fenced

Completion Reminder
        只观察产品层事实
        不参与 Runtime Safety
```

---

# 2. 设计原则

## 2.1 SerenaDesktop 不判断 ChatGPT 是否“已经超时”

SerenaDesktop 无法可靠知道：

```text
ChatGPT Browser 是否仍连接
OpenAI Server Turn 是否仍存活
WebSocket 是否已经恢复
用户是否正在看另一个 ChatGPT 页面
```

因此系统不产生：

```text
chatgptTimedOut = true
chatgptDisconnected = true
```

这类未经证明的业务事实。

提醒条件只基于 SerenaDesktop 自己可以证明的事实：

```text
Execution 已进入安全终态
+
Final Result 已持久化并可读取
+
经过短暂 Grace Period
+
没有观察到 ChatGPT 控制端继续消费该 Execution
```

对应用户语义：

> Agent 任务已经结束，但 ChatGPT 尚未继续处理任务结果，请返回 ChatGPT 查看当前会话。

---

## 2.2 Transport Session 不属于业务 Authority

不新增：

```text
ChatGPT Conversation → Work Binding
MCP Session → Work Binding
Transport Session → Workspace Binding
Browser Session → Execution Binding
```

新的 ChatGPT Turn 通过持久化 Work / Execution 重新发现状态。

---

## 2.3 Work 是业务容器，Execution 是执行单元

继续保持：

```text
WorkRun W1

E1
 ↓ continue
E2
 ↓ continue
E3
```

Work 不持有 Workspace Claim。

Execution 继续拥有：

```text
Workspace Claim
Runtime Ownership
Provider Evidence
Cancellation
Recovery
Final Result
```

---

## 2.4 通知属于 Product Telemetry

Agent 完成提醒不得影响：

```text
Execution Status
Dispatch State
Workspace Claim
Runtime
Provider Terminal Evidence
Recovery
Cancellation
Atomic Claim Release
```

通知失败也不能把 Execution 从：

```text
completed
```

改成：

```text
failed
```

---

# 3. 非目标

本次不实现：

```text
ChatGPT Browser WebSocket 探测
ChatGPT 页面 DOM 探测
浏览器 Heartbeat
Conversation ID Binding
MCP Session Ownership
Work Controller Lease
分布式锁
新的 Runtime Recovery
新的 Execution Status
新的 Provider Pipeline
自动重放 uncertain Provider Request
```

也不允许因为长时间没有 `agent_query` 自动：

```text
Cancel Execution
Terminate Runtime
Release Workspace Claim
Resume Pending
重新创建 Execution
```

---

# 4. Work Revision 正式成为控制面版本

继续复用现有：

```text
work_runs.revision
```

不增加第二个 Work Revision。

定义：

> `work_runs.revision` 是 Work Control Revision，用于检测基于陈旧 Work 状态发起的 mutation。

初始：

```text
work begin
→ revision = 1
```

以下成功接受的控制操作必须递增：

```text
agent_execute start
agent_execute continue
agent_execute resume_pending
agent_execute cancel

work_update finish
work_update cancel
```

以下事件不得改变 Work Revision：

```text
Execution running → finalizing
Execution terminal
Activity update
Usage update
heartbeat
observe
日志更新
UI refresh
Completion Notification
Controller Follow-up
```

因此：

```text
Work Revision
≠ Execution Lifecycle Revision
≠ Activity Revision
≠ Usage Revision
≠ Notification Revision
```

---

# 5. Work Mutation 使用 expectedWorkRevision

所有属于现有 Work 的 Mutation 增加：

```text
expectedWorkRevision
```

例如：

```json
{
  "action": "continue",
  "workRunId": "wrk_123",
  "parentExecutionId": "exec_1",
  "expectedWorkRevision": 7,
  "requestKey": "continue-after-review-v1",
  "prompt": "补充缺失测试"
}
```

必须在同一个 StateStore Transaction 中：

```text
BEGIN IMMEDIATE

读取 WorkRun

验证：
    status == active
    revision == expectedWorkRevision

执行业务 mutation

UPDATE work_runs
SET revision = revision + 1

COMMIT
```

禁止事务外检查：

```text
read revision
    ↓
释放事务
    ↓
执行 mutation
```

Revision 不匹配返回：

```text
WORK_REVISION_CONFLICT
```

响应：

```json
{
  "code": "WORK_REVISION_CONFLICT",
  "workRunId": "wrk_123",
  "expectedRevision": 7,
  "currentRevision": 8
}
```

MCP Tool Description 必须明确：

> 收到 `WORK_REVISION_CONFLICT` 后重新执行 `work_query`，不得原参数直接重试 mutation。

---

# 6. Agent Start 只允许创建 Work 的首个 Execution

为了防止浏览器中断后新的 ChatGPT Turn 因不知道已有 Execution 而创建第二条根执行链，冻结：

> 一个 Work 只能通过 `start` 创建第一个 Execution，之后必须通过 `continue` 创建后续 Execution。

事务：

```text
BEGIN IMMEDIATE

验证：
    Work 存在
    Work.status == active
    Work.revision == expectedWorkRevision

检查：
    Work 尚无任何 work_execution_links

创建 E1
创建 Workspace Claim
创建 Work Link

Work.revision += 1

COMMIT

之后才允许 Provider Dispatch
```

如果 Work 已经存在任意 Execution：

```text
WORK_ALREADY_STARTED
```

同时返回：

```text
latestExecutionId
```

如果已有 unresolved Execution，也可以投影：

```text
WORK_HAS_ACTIVE_EXECUTIONS
```

但不得创建新的 Root Execution。

现有 `(agent_id, request_key)` 幂等继续保持：

```text
same requestKey + same hash
→ 返回原 Execution

same requestKey + different hash
→ EXECUTION_REQUEST_KEY_CONFLICT
```

二者职责不同：

```text
requestKey
→ 防同一请求重复 Provider Dispatch

Work Revision
→ 防陈旧 ChatGPT Turn 控制 Work
```

---

# 7. Continue 固定为单链模型

Continue 创建新 Execution：

```text
E1 terminal
   ↓
continue
   ↓
E2
```

禁止修改 E1。

V0.2 暂不支持：

```text
E1
├── E2
└── E3
```

增加数据库防御：

```sql
CREATE UNIQUE INDEX IF NOT EXISTS
work_execution_one_child_per_parent
ON work_execution_links(work_run_id, parent_execution_id)
WHERE parent_execution_id IS NOT NULL;
```

Migration 前必须检查现有数据是否已经存在一个 Parent 多 Child。

如果存在：

```text
DESIGN / DATA MIGRATION BLOCKER
```

不得自动删数据或选择某个 Child。

Continue Transaction 内验证：

```text
Work active
Work revision == expectedWorkRevision

parentExecution 属于 Work
parentExecution 是 terminal
parentExecution 是当前 lineage head
Work 当前没有 unresolved Execution
parentExecution 尚无 child
```

成功：

```text
创建 E2
parentExecutionId = E1

创建 Workspace Claim
创建 Work Link

revision += 1
```

已有 Child：

```text
WORK_CONTINUATION_EXISTS
```

并返回：

```text
existingChildExecutionId
```

ChatGPT 应 Observe 已有 Child，而不是重新 Continue。

---

# 8. Cancel 使用相同控制 CAS

`agent_execute cancel`：

```json
{
  "action": "cancel",
  "workRunId": "wrk_123",
  "executionId": "exec_1",
  "expectedWorkRevision": 7
}
```

在持久化用户取消意图的同一个 Transaction 中：

```text
验证 Execution 属于 Work
验证 Work active
验证 expectedWorkRevision
持久化现有 interrupt_requested_at / cancel intent
Work revision += 1
```

然后继续复用现有：

```text
turn/interrupt
Cancel ACK
Cancel Timeout
Provider Terminal Race
Cancellation Attribution
```

禁止重新实现 Cancellation Pipeline。

---

# 9. resume_pending 边界保持

`resume_pending` 同样加入：

```text
workRunId
expectedWorkRevision
```

继续要求已有冻结条件：

```text
dispatch_pending
+
not_dispatched
+
runtime_instance_id IS NULL
+
provider_terminal_status IS NULL
+
无 Runtime Attempt
+
Workspace Claim 属于当前 Execution
```

Work Revision 验证合并到已有原子 Resume Guard 中。

成功接受 Resume：

```text
revision += 1
```

以下事实永远不能触发 `resume_pending`：

```text
浏览器超时
ChatGPT 回复断线
MCP Observe 被 Cancel
长时间没有 agent_query
```

---

# 10. Work Finish / Cancel

## Finish

增加：

```text
expectedWorkRevision
```

Transaction 内验证：

```text
Work active
revision matched

全部关联 Execution 均为：
completed
failed
cancelled
interrupted
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

返回：

```text
WORK_HAS_ACTIVE_EXECUTIONS
```

成功：

```text
Work → completed / failed
revision += 1
completed_at = now
```

---

## Cancel Work

Work 层不隐式 Cancel Agent。

若仍有 unresolved Execution：

```text
WORK_HAS_ACTIVE_EXECUTIONS
```

调用顺序必须是：

```text
agent_execute cancel
    ↓
agent_query observe
    ↓
Execution terminal
    ↓
work_update cancel
```

避免出现第二套 Cancellation Pipeline。

---

# 11. Work Query 增强：恢复入口

新的 ChatGPT Turn 可能完全不知道上一轮的 `executionId`。

因此 `work_query` 必须承担恢复发现职责。

## list

支持：

```text
workspaceId?
status?
limit
cursor
```

Active Work 返回：

```json
{
  "workRunId": "wrk_123",
  "workspaceId": "project-a",
  "title": "修复 reconnect 状态覆盖",
  "status": "active",
  "revision": 8,

  "latestExecutionId": "exec_2",
  "unresolvedExecutionIds": ["exec_2"],

  "updatedAt": 123456789
}
```

---

## get

返回：

```text
Work 基础信息
revision
latestExecutionId

Execution lineage:
    executionId
    parentExecutionId
    status
    createdAt
    completedAt
```

不在 Work Query 中返回完整 Final Result。

Final Result 继续：

```text
agent_query observe(
    executionId,
    includeResult=true
)
```

---

# 12. Recovery First MCP 行为

修改：

```text
work_query
agent_query
agent_execute
```

的工具说明。

当用户表达：

```text
继续刚才
刚才超时了
刚才断开了
重试刚才
看看执行到哪了
```

或者上下文无法证明上一轮 Agent 是否已经创建时，ChatGPT 应执行：

```text
1. work_query 查 active Work

2. 找到 latest / unresolved Execution

3. unresolved Execution 存在
      ↓
   agent_query observe

4. Execution terminal + resultAvailable
      ↓
   agent_query includeResult=true

5. 确认不存在对应 Execution
      ↓
   才允许 agent_execute start
```

工具说明明确：

> Previous ChatGPT response interruption, MCP observation cancellation or browser timeout is not evidence that an Execution failed or was never dispatched.

以及：

> Do not create a replacement Execution until the active Work and its existing Executions have been queried.

---

# 13. Controller Follow-up：定义 ChatGPT 是否“回来处理结果”

本方案不判断 ChatGPT 是否在线，只记录：

> Execution terminal 之后，SerenaDesktop 是否观察到了足以证明控制流程继续推进的后续动作。

增加独立 Product 表：

```sql
CREATE TABLE execution_controller_state (
    execution_id TEXT PRIMARY KEY NOT NULL,

    terminal_at INTEGER NOT NULL,

    controller_followup_at INTEGER,
    controller_followup_kind TEXT
        CHECK (
            controller_followup_kind IS NULL OR
            controller_followup_kind IN (
                'result_read',
                'continue',
                'work_finish',
                'work_cancel'
            )
        ),

    notification_due_at INTEGER NOT NULL,
    notification_sent_at INTEGER,

    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,

    FOREIGN KEY(execution_id)
        REFERENCES executions(id)
        ON DELETE RESTRICT
);
```

该表是：

```text
Product Telemetry
```

不是：

```text
Runtime Evidence
```

禁止 Runtime / Provider / Workspace Coordinator 根据它作安全判断。

---

# 14. 哪些行为算 Controller Follow-up

## 14.1 Final Result Read

当：

```text
agent_query observe(
    executionId=E1,
    includeResult=true
)
```

满足：

```text
E1 terminal
resultAvailable=true
persisted Final Result 可成功读取
```

则记录：

```text
controller_followup_at = now
controller_followup_kind = result_read
```

普通：

```text
observe(includeResult=false)
```

即使看到了：

```text
status=completed
```

也不算 Follow-up。

原因是 ChatGPT 可能刚看到：

```text
resultAvailable=true
```

浏览器便断开。

我们仍希望 10 秒后提醒用户。

---

## 14.2 Continue

成功：

```text
agent_execute continue(parentExecutionId=E1)
```

意味着 Controller 已经消费 E1 的语义结果并推进下一步。

记录：

```text
controller_followup_kind = continue
```

---

## 14.3 Work Finish

成功 Finish Work，且 E1 属于本次 Work：

```text
controller_followup_kind = work_finish
```

---

## 14.4 Work Cancel

控制端明确收口 Work：

```text
controller_followup_kind = work_cancel
```

---

# 15. Follow-up 记录的事务边界

对于：

```text
continue
work finish
work cancel
```

Follow-up 标记必须与对应控制 Mutation 在**同一个 StateStore Transaction** 中提交。

例如 Continue：

```text
BEGIN IMMEDIATE

validate Work revision
create E2
create Claim
create Link
Work revision += 1

mark E1 controller_followup = continue

COMMIT
```

避免：

```text
E2 已创建
但 E1 仍显示等待 ChatGPT
```

对于：

```text
agent_query includeResult=true
```

它是只读 Product 操作。

要求：

```text
读取 exact persisted Final Result
构造合法 Product Response
    ↓
upsert controller_followup=result_read
```

这里的语义是：

> SerenaDesktop 已经观察到 ChatGPT Controller 主动请求并成功读取结果。

它不声称浏览器最终成功渲染了该响应。

---

# 16. Execution Completion Reminder

增加 Product Service：

```text
ExecutionCompletionNotifier
```

职责：

```text
监听 Execution 进入安全终态
        ↓
Final Result ready
        ↓
建立 Controller State
        ↓
等待 Grace Period
        ↓
检查是否已有 Controller Follow-up
        ↓
决定是否通知
```

第一版固定：

```text
COMPLETION_REMINDER_GRACE_MS = 10_000
```

不要暴露成用户可调数字。

---

# 17. 通知触发条件

Execution 成功进入：

```text
completed
failed
interrupted
cancelled
```

且：

```text
resultAvailable = true
```

后：

```text
terminalAt = Execution.completed_at

notificationDueAt =
    terminalAt + 10 seconds
```

10 秒后重新读取：

```text
Execution
execution_controller_state
```

仅当全部满足：

```text
Execution 仍为 terminal
resultAvailable = true

controller_followup_at IS NULL
notification_sent_at IS NULL
```

才发送提醒。

---

# 18. 通知内容

## completed

标题：

```text
Agent 任务已完成
```

正文：

```text
ChatGPT 尚未读取任务结果，请返回 ChatGPT 查看当前会话状态。
```

可以安全附加：

```text
Work Title
Workspace Display Name
```

但禁止附加：

```text
Prompt
源码
命令行
stdout
stderr
Diff
Token
Credential
```

---

## failed / interrupted

标题：

```text
Agent 任务已结束
```

正文：

```text
ChatGPT 尚未读取执行结果，请返回 ChatGPT 查看任务状态。
```

---

## cancelled

如果是明确用户发起的 Cancel，可以默认不播放额外声音。

若取消归因无法证明来自用户，按：

```text
Agent 任务已结束
```

处理。

具体以现有 Cancellation Attribution 为准，不重新判断。

---

# 19. 通知方式

第一版同时支持：

```text
Windows 系统通知
+
系统提示音
```

优先复用当前 Tauri / Windows 已有通知能力。

如果系统通知能力和声音能力是两个独立 API，则：

```text
System Notification Failure
```

不得阻止：

```text
Sound Notification
```

反之亦然。

通知属于 Best-Effort UX。

通知失败：

```text
记录诊断
```

不得：

```text
改变 Execution
改变 Work
重新通知无限重试
```

---

# 20. 通知点击行为

点击系统通知：

```text
显示 SerenaDesktop
    ↓
打开 Agent 任务详情
    ↓
定位到对应 Execution
```

如果当前平台通知 API 不支持直接携带 Execution Deep Link，至少：

```text
显示并聚焦 SerenaDesktop 主窗口
```

第一版不自动：

```text
打开特定 ChatGPT Conversation
```

因为 SerenaDesktop 没有可靠的 ChatGPT Conversation Authority。

通知正文已经明确引导：

> 返回 ChatGPT 查看当前会话状态。

---

# 21. 设置项

设置页新增：

```text
Agent 任务完成提醒

[✓] 当 ChatGPT 未及时处理结果时提醒我
```

默认：

```text
enabled = true
```

第一版：

```text
Grace Period = 固定 10 秒
System Notification = 开启
Sound = 开启
```

暂不增加：

```text
5s / 10s / 30s 下拉
自定义铃声
按 Workspace 单独配置
按 Agent 单独配置
```

避免过度设计。

---

# 22. Agent 详情 UI 状态

Execution Terminal 后增加辅助产品状态。

## 等待 Controller

```text
已完成

等待 ChatGPT 获取结果
```

---

## Controller 已继续

```text
已完成

ChatGPT 已继续处理
```

这里的“已继续处理”只表示：

```text
controller_followup_at != null
```

不要显示：

```text
ChatGPT 在线
ChatGPT 已收到最终回答
```

---

## 通知已发送

可以显示一个非常轻量的状态：

```text
已发送完成提醒
```

用于诊断。

不需要作为主要状态徽章。

---

# 23. 通知调度实现边界

通知 Scheduler 不进入 Runtime。

推荐：

```text
Agent Product Service
        │
        ├── Execution Projection
        ├── Observe
        └── Completion Notifier
```

流程：

```text
Runtime Foundation
    ↓
Atomic Terminal Commit 完成
    ↓
Product Layer 观察 terminal
    ↓
ensure execution_controller_state
    ↓
spawn bounded 10s timer
```

Timer 到期：

```text
重新读取 StateStore
```

不得依赖 10 秒前缓存的 Snapshot。

---

# 24. SerenaDesktop 重启语义

Completion Reminder 是 UX 能力，不要求建立第二套 Crash Recovery。

第一版：

```text
Host 活着时产生 terminal
→ 正常安排通知
```

Host 在 Grace Period 内退出：

```text
不保证重启后补发旧通知
```

不做：

```text
启动扫描所有历史 terminal Execution
```

防止升级后突然产生大量旧提醒。

`execution_controller_state` 仍持久化，用于：

```text
防重复
UI
诊断
后续扩展
```

未来如确有需要，可增加 bounded recent-pending replay，本版不实现。

---

# 25. 旧 Turn / 新 Turn 并存竞态

真实可能出现：

```text
Browser 已显示 Turn A 超时
        ↓
用户启动 Turn B
        ↓
OpenAI Server Turn A 仍继续 MCP
```

假设：

```text
Turn A 读取 Work revision=7
Turn B 读取 Work revision=7
```

B 先执行：

```text
continue E1
```

成功：

```text
E2 created
revision = 8

E1.controller_followup = continue
```

A 随后：

```text
continue / cancel / finish
expectedWorkRevision = 7
```

必须：

```text
WORK_REVISION_CONFLICT
```

旧 Turn Query 后看到：

```text
E2 已存在
```

不得再创建新的 Execution。

系统不判断：

```text
Turn A / Turn B 哪一个是“正版控制者”
```

只提供：

> 同一 Work Revision 上的竞争 mutation 最多一个成功。

---

# 26. Agent 完成但 ChatGPT 断开的真实流程

正常开始：

```text
Turn A

work begin
 ↓
W1 revision=1

agent start(expected=1)
 ↓
E1 running
W1 revision=2
```

浏览器：

```text
消息发送超时，请重试
```

但：

```text
Codex
 ↓
继续执行
 ↓
E1 completed
 ↓
Final Result persisted
```

Product Layer：

```text
T0:
E1 terminal

T0 + 10s:
controller_followup_at == null
```

SerenaDesktop：

```text
🔔

Agent 任务已完成
ChatGPT 尚未读取任务结果，请返回 ChatGPT 查看当前会话状态。
```

用户回到 ChatGPT，发送：

```text
继续刚才的任务
```

Turn B：

```text
work_query
 ↓
W1 revision=2
latestExecutionId=E1

agent_query includeResult=true
 ↓
读取 E1 Final Result
 ↓
E1 controller_followup=result_read
```

然后：

```text
source / git Review
```

需要继续：

```text
agent_execute continue(
    parentExecutionId=E1,
    expectedWorkRevision=2
)
```

产生：

```text
E2
revision=3
```

---

# 27. 稳定错误码

继续保留：

```text
WORK_NOT_FOUND
WORK_NOT_ACTIVE
WORK_HAS_ACTIVE_EXECUTIONS
EXECUTION_NOT_IN_WORK
EXECUTION_REQUEST_KEY_CONFLICT
```

增加：

```text
WORK_REVISION_CONFLICT
WORK_CONTINUATION_EXISTS
WORK_ALREADY_STARTED
```

通知错误不向 MCP 暴露业务错误码。

通知失败只进入本地诊断：

```text
AGENT_COMPLETION_NOTIFICATION_FAILED
```

不影响 Execution / Work。

---

# 28. Runtime Foundation 保持不变

必须提供机械证据确认本次没有修改：

```text
Execution 11-state graph
Dispatch State graph
runtime_instance_id immutable
Workspace Claim ownership
Provider Terminal Evidence
Background Terminal Cleanup
Runtime Termination Evidence
Windows Job Object
Cross-Runtime Recovery
Unknown Fail-Closed
Atomic Claim Release
Cancellation Attribution
HistoryMode Recovery
```

以下都只能消费 Runtime 已形成的事实：

```text
Work Revision
Controller Follow-up
Completion Notification
```

禁止反向控制 Runtime。

---

# 29. 测试矩阵

## 29.1 Recovery Discovery

```text
W1 + E1 running
↓
原 MCP Client 消失
↓
新的 MCP Client
↓
work_query
↓
仍能找到 W1 / E1
```

---

## 29.2 Cross-client Final Result

```text
E1 completed
↓
原 Client 消失
↓
新 Client
↓
work_query → E1
↓
agent_query includeResult=true
↓
exact persisted Final Result
```

验证：

```text
0 new Runtime
0 new Turn
0 Provider Dispatch
```

---

## 29.3 Duplicate Start

```text
W1 已存在 E1
↓
agent_execute start
↓
WORK_ALREADY_STARTED
```

0 新 Execution。

---

## 29.4 Work Revision Race

```text
A expectedRevision=7
B expectedRevision=7
```

并发 Mutation。

预期：

```text
恰好一个成功
Work revision=8
另一个 WORK_REVISION_CONFLICT
```

---

## 29.5 Continue Race

```text
E1 completed

Turn A continue E1
Turn B continue E1
```

预期：

```text
最多一个 E2
```

数据库不存在：

```text
E2(parent=E1)
E3(parent=E1)
```

---

## 29.6 Old Turn After New Turn

B 已：

```text
E1 → E2
revision=8
```

A 使用：

```text
revision=7
```

执行：

```text
continue
cancel
finish
```

全部不能成功。

---

## 29.7 Observe Disconnect

```text
observe long-poll
↓
MCP disconnect
↓
future drop
```

预期：

```text
Execution unchanged
Runtime unchanged
Claim unchanged
```

---

## 29.8 Completion Reminder — 正常消费

```text
E1 completed
T0

T0 + 3s:
agent_query includeResult=true
```

预期：

```text
controller_followup=result_read
0 notification
0 sound
```

---

## 29.9 Completion Reminder — ChatGPT 无 Follow-up

```text
E1 completed
T0

10 秒内无 includeResult / continue / finish
```

预期：

```text
1 system notification
1 sound
notification_sent_at != null
```

---

## 29.10 普通 Observe 不阻止提醒

```text
E1 completed
↓
agent_query observe(includeResult=false)
↓
resultAvailable=true
↓
没有其他 Follow-up
```

10 秒后仍通知。

---

## 29.11 Continue 阻止提醒

```text
E1 completed
↓
9 秒时 continue(E1) 成功
```

预期：

```text
controller_followup=continue
0 notification
```

---

## 29.12 Work Finish 阻止提醒

```text
E1 completed
↓
work_finish 成功
```

0 notification。

---

## 29.13 通知与 Follow-up 临界竞态

模拟：

```text
T0 + 10s
```

同时发生：

```text
Notifier Check
agent_query includeResult=true
```

必须通过 StateStore 原子更新保证最终只有两种合法结果：

### A

```text
Follow-up 先成功
→ 不通知
```

### B

```text
Notifier 先原子 claim notification
→ 通知一次
→ 随后 Follow-up 正常记录
```

禁止重复通知。

---

## 29.14 Notification Failure

模拟：

```text
Windows notification API failure
sound API failure
```

预期：

```text
Execution terminal 保持
Work 保持
Claim 不存在回归
无 Runtime mutation
记录安全诊断
```

---

## 29.15 App Shutdown During Grace

```text
E1 terminal
↓
5 秒后 App Exit
```

预期：

```text
无 Runtime 回归
无强制等待 10 秒
退出流程正常
```

第一版重启后不补发。

---

# 30. Notification 并发实现要求

必须防止：

```text
两个 Timer
两个 Product Observer
两个 UI Refresh
```

导致两次通知。

Notifier 到期后使用原子 claim，例如逻辑：

```text
BEGIN IMMEDIATE

UPDATE execution_controller_state
SET
    notification_sent_at = now,
    updated_at = now
WHERE
    execution_id = ?
    AND controller_followup_at IS NULL
    AND notification_sent_at IS NULL

检查 affectedRows == 1

COMMIT
```

只有：

```text
affectedRows == 1
```

的调用者允许真正触发通知。

如果：

```text
affectedRows == 0
```

不通知。

---

# 31. 代码边界建议

预计涉及：

```text
src-tauri/src/agent/
```

中的 Work / Product / Store 层。

建议责任：

```text
StateStore
    ├── Work Revision CAS
    ├── Work Execution Lineage
    └── Execution Controller State

Work Orchestration
    ├── Recovery Discovery
    ├── Start / Continue guards
    └── Work Control Mutation

Agent Product
    ├── includeResult follow-up
    └── CompletionNotifier

Desktop/Tauri
    ├── System Notification
    ├── Sound
    └── Focus Execution Detail
```

不得让：

```text
Windows Launcher
Codex App Server Client
Codex Provider
Runtime
WorkspaceExecutionCoordinator
```

直接处理 Completion Notification。

---

# 32. 实施顺序

本需求一次性完成，但内部必须按以下顺序实施并分别运行聚焦测试。

## Phase 1 — Baseline

阅读：

```text
docs/core-work-orchestration.md
docs/codex-agent-observe.md
docs/codex-agent-runtime.md
docs/technical-design-agent-platform-v0.2*.md
```

核对实际：

```text
work_runs
work_execution_links
work_query
work_update
agent_query
agent_execute
create_execution
continue
cancel
resume_pending
```

若本文假设与真实 Schema / Contract 存在 Material Difference：

```text
停止受影响阶段并报告
```

不得自行复制第二套模型。

---

## Phase 2 — Work Recovery Query

完成：

```text
latestExecutionId
unresolvedExecutionIds
lineage projection
```

补 Query 测试。

---

## Phase 3 — Work Revision CAS

依次接入：

```text
start
continue
cancel
resume_pending
work finish
work cancel
```

每项独立聚焦测试。

---

## Phase 4 — Single Continuation Chain

增加：

```text
one child per parent index
lineage head validation
WORK_ALREADY_STARTED
WORK_CONTINUATION_EXISTS
```

跑并发 Race Test。

---

## Phase 5 — Controller Follow-up State

增加：

```text
execution_controller_state
result_read
continue
work_finish
work_cancel
```

证明该表不参与 Runtime Safety。

---

## Phase 6 — Completion Notifier

实现：

```text
10s Grace
atomic notification claim
Windows notification
sound
Agent detail focus
settings toggle
```

---

## Phase 7 — MCP Contract

更新：

```text
expectedWorkRevision
Recovery First Tool Description
新错误码
tools/list schema
```

---

## Phase 8 — E2E / Race / Reconnect

完整执行第 29 节矩阵。

---

# 33. 最终 Acceptance Gate

全部 PASS 才认为：

```text
ChatGPT Control Recovery & Completion Reminder = READY
```

必须证明：

```text
1. ChatGPT Browser / MCP Observe Disconnect 不取消 Execution。

2. 新 ChatGPT Turn 即使不知道旧 executionId，
   也可以通过 Work 恢复 Execution。

3. Final Result 可以跨 MCP Client 重复读取。

4. 一个 Work 只能 Start 一个 Root Execution。

5. Continue 始终创建新的 Execution。

6. 一个 Parent Execution 最多一个 Child。

7. requestKey 重试不产生第二个 Provider Dispatch。

8. 相同 Work Revision 的并发 Mutation 最多一个成功。

9. 旧 ChatGPT Turn 的 stale mutation 被拒绝。

10. Work Revision Mutation 与对应业务变更原子提交。

11. Execution terminal 不自动修改 Work Revision。

12. 普通 observe 不算 Controller Follow-up。

13. includeResult=true 成功读取 Final Result 后记录 Follow-up。

14. Continue / Work Finish / Work Cancel 原子记录 Follow-up。

15. Execution Terminal + Final Result Ready 后启动 10 秒 Reminder Grace。

16. Grace 内存在 Follow-up 时不提醒。

17. Grace 后无 Follow-up 时恰好提醒一次。

18. 通知包含声音与 Windows 系统通知。

19. Notification Failure 不影响 Work / Execution / Runtime。

20. Reminder 状态不属于 Runtime / Claim / Recovery Evidence。

21. App Shutdown 不等待 Notification Timer。

22. 不新增 ChatGPT / MCP Session 业务 Binding。

23. 不修改 Runtime Foundation Safety Contract。

24. Browser Timeout、MCP Disconnect、Controller Silence
    都不构成 Provider Failure Evidence。
```

实施完成后输出以下证据：

```text
修改文件
Schema Migration
每阶段聚焦测试
并发 Race Tests
Reconnect Test
Notification Test
完整 Gate 结果

PASS / FAIL / NOT_RUN / UNAVAILABLE
```

不得用“编译通过”替代上述 Acceptance Gate。
