# SerenaDesktop Codex Agent Runtime 技术方案 V0.4

本次修订仅修正标准流句柄继承和 Named Job 的 Windows Session 证据约束。

## 1. Windows Runtime 创建契约

Windows Codex Runtime 必须保证：

> `codex.exe` 从创建成功的第一个时刻起就属于 SerenaDesktop 管理的 Job Object。

禁止采用：

```text id="h1crkf"
CreateProcess
    ↓
AssignProcessToJobObject
```

这种两阶段绑定方式。

Windows 10+ 使用：

```text id="a1xxe3"
CreateJobObjectW
        ↓
SetInformationJobObject
        ↓
InitializeProcThreadAttributeList
        ↓
PROC_THREAD_ATTRIBUTE_JOB_LIST = [codexJob]
        ↓
CreateProcessW(
    EXTENDED_STARTUPINFO_PRESENT |
    CREATE_NO_WINDOW
)
```

其中 Job 通过 `PROC_THREAD_ATTRIBUTE_JOB_LIST` 直接参与进程创建。

因此不存在：

```text id="eh0h1h"
codex.exe 已运行
但尚未进入 Job
```

的合法运行窗口。

如果操作系统或当前启动环境不支持该能力：

```text id="14b2ku"
CODEX_JOB_AT_CREATION_UNSUPPORTED
```

Codex Runtime 不启动。

---

## 2. Job Object 配置

每个 Runtime Instance 创建独立 Job：

```text id="0dqsjd"
Local\SerenaDesktop.Codex.<runtimeInstanceId>
```

Job Handle 必须满足：

```text id="3pya4e"
inheritable = false
```

创建时使用不可继承的 Job Handle。

进程创建：

```text id="f64j8u"
bInheritHandles = TRUE
STARTF_USESTDHANDLES
PROC_THREAD_ATTRIBUTE_HANDLE_LIST = [childStdinRead, childStdoutWrite, childStderrWrite]
```

只有这三个子进程标准流管道句柄设为可继承，并分别写入 `STARTUPINFOEX.StartupInfo` 的 `hStdInput`、`hStdOutput`、`hStdError`。父进程使用的管道端保持不可继承；创建成功后关闭父进程持有的子进程端副本，避免阻止 EOF 传播。

`PROC_THREAD_ATTRIBUTE_HANDLE_LIST` 与 `PROC_THREAD_ATTRIBUTE_JOB_LIST` 同时配置：前者仅传递标准流，后者负责创建时绑定 Job。Job Handle 保持不可继承，且绝不加入 HANDLE_LIST。使用 HANDLE_LIST 时，Windows 要求 `bInheritHandles = TRUE`，不能为禁止 Job Handle 继承而关闭所有句柄继承。[Microsoft API 契约](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute)

SerenaDesktop：

```text id="8drjzh"
不 Duplicate Job Handle 给子进程
不通过 HANDLE_LIST 传递 Job Handle
不通过 IPC 暴露 Job Handle
```

Job Limits 必须包含：

```text id="j99f5x"
JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
```

不得包含：

```text id="3s7j3t"
JOB_OBJECT_LIMIT_BREAKAWAY_OK
JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK
```

因此正常使用 Win32 子进程创建机制产生的 Codex 后代进程继续属于该 Job。

---

## 3. Runtime 持久化

`runtime_instances` 增加：

```text id="tseirf"
id

owner_host_instance_id

job_name
job_session_id
job_creation_mode
job_handle_inheritable
job_kill_on_close
job_breakaway_allowed

codex_pid
codex_process_start_token

state

started_at
stopped_at

termination_evidence_type
termination_evidence_at
```

固定记录：

```text id="iw4b12"
job_creation_mode =
proc_thread_attribute_job_list

job_handle_inheritable = false

job_kill_on_close = true

job_breakaway_allowed = false
```

这些字段属于 Runtime Evidence Contract。

`job_session_id` 保存创建 Named Job 的 Host 所在 Windows Session ID，通过 `ProcessIdToSessionId` 获取成功后，与 jobName 一同落盘。它不是 `hostInstanceId`。无法取得 Session 身份时不得继续创建 Runtime。

---

## 4. Runtime 创建顺序

Runtime 创建流程：

```text id="mw636g"
1. BEGIN

2. 创建 runtimeInstance
   state = preparing

3. 保存 jobName、jobSessionId 和预期 Job Policy

4. COMMIT

5. CreateJobObject

6. 设置 KILL_ON_JOB_CLOSE
   且禁止 Breakaway

7. 创建标准流管道并构建 STARTUPINFOEX
   STARTF_USESTDHANDLES 指向三个子进程端
   父进程端与 Job Handle 不可继承

8. 设置 PROC_THREAD_ATTRIBUTE_JOB_LIST
   同时设置 PROC_THREAD_ATTRIBUTE_HANDLE_LIST，仅包含三个子进程标准流句柄

9. CreateProcessW
   bInheritHandles = TRUE
   codex app-server
   进程创建时即属于 Job

10. 获取 PID + Process Start Token

11. 持久化
    state = starting
    codexPid
    processStartToken

12. JSON-RPC initialize

13. state = running
```

第 9 步之后即使 SerenaDesktop 立即崩溃：

```text id="09xhcn"
Host 持有的不可继承 Job Handle 被关闭
        ↓
KILL_ON_JOB_CLOSE
        ↓
Job 内进程被终止
```

不存在后绑定清理窗口。

---

# 5. Job Handle 所有权

SerenaDesktop Host 是 Codex Job Handle 的唯一合法持有者。

以下行为禁止：

```text id="zcm72r"
Job Handle inheritance

DuplicateHandle 到 Codex

向任何 Child Process 传递 Job Handle

向其他 SerenaDesktop 进程传递 Job Handle
```

V0.4 不允许多个 SerenaDesktop Host 共同持有同一 Codex Job Handle。

这使：

```text id="7uc8tx"
Host Handle 生命周期
```

可以成为 Runtime 生命周期的一部分。

---

# 6. Job 级终止证据

不得使用：

```text id="03n6dw"
codex 主 PID 不存在
```

作为 Runtime 已完全终止的充分证据。

因为：

```text id="v64gou"
主进程退出
≠
所有 Job 内子进程退出
```

Runtime 安全终止要求 Job 级证据。

正常持有 Job Handle 时使用：

```text id="56vah8"
QueryInformationJobObject(
    JobObjectBasicAccountingInformation
)
```

检查：

```text id="yhlttt"
ActiveProcesses == 0
```

只有：

```text id="ma650n"
ActiveProcesses == 0
```

才认定：

```text id="ey8bfw"
JOB_EMPTY_CONFIRMED
```

---

# 7. Runtime 显式终止

需要主动终止 Runtime 时：

```text id="h95zsl"
TerminateJobObject(job)
        ↓
poll QueryInformationJobObject
        ↓
ActiveProcesses == 0
```

使用 bounded polling。

确认：

```text id="b6cgyj"
ActiveProcesses == 0
```

后记录：

```text id="c4ow1r"
terminationEvidenceType =
job_active_processes_zero
```

然后才能关闭 Job Handle 并完成 Runtime 收敛。

仅终止：

```text id="xgx3qf"
codexPid
```

不属于合法 Runtime Cleanup。

---

# 8. 重启后的 Named Job 核对

Job 使用可预测唯一名称：

```text id="prhujz"
Local\SerenaDesktop.Codex.<runtimeInstanceId>
```

SerenaDesktop 重启后，先取得当前 Windows Session ID，并与持久化的 `job_session_id` 比较。`Local\` 属于当前 Session 的命名空间；只有两者一致、确认查询原命名空间时，才执行下述自动核对。[Microsoft 命名空间说明](https://learn.microsoft.com/en-us/windows/win32/termserv/kernel-object-namespaces)

如果 Session 不一致、记录缺失或当前 Session 身份无法取得，保持 `unknown` 和 Workspace Claim，交由已有本地人工 Resolve 流程处理。V0.4 不增加跨 Session Job 管理能力，也不把当前 Session 中同名对象的查询结果用作旧 Runtime 证据。

在原 Session 命名空间中：

```text id="0o01mt"
OpenJobObject(jobName)
```

如果成功：

```text id="3pyiz2"
说明 Job Object 仍然存在
```

必须：

```text id="cjt3j7"
Query ActiveProcesses
```

不能根据旧 Codex PID 推断状态。

如果需要清理：

```text id="hxjv55"
TerminateJobObject
        ↓
ActiveProcesses == 0
```

取得 Job 级终止证据。

---

# 9. Job 对象已经不存在

如果：

```text id="0m4js9"
OpenJobObject(jobName)
→ ERROR_FILE_NOT_FOUND
```

自动接受该结果必须先满足第 8 节的原 Session 命名空间核验，并同时验证该 Runtime 的持久化 Policy：

```text id="fkn3a8"
job_creation_mode =
proc_thread_attribute_job_list

job_handle_inheritable = false

job_kill_on_close = true

job_breakaway_allowed = false
```

在这些不变量成立的前提下：

```text id="l8slxu"
Named Job 已不存在
```

可记录：

```text id="lunbus"
terminationEvidenceType =
managed_job_destroyed
```

`ERROR_FILE_NOT_FOUND` 只说明所查询命名空间中没有该对象。未确认原 Session 命名空间时不得记录 `managed_job_destroyed`；`ERROR_ACCESS_DENIED` 或其他查询错误也不得解释为对象不存在。

如果 Runtime Policy 数据不完整：

```text id="09ezdw"
不得推导 Job 已安全清空
```

保持：

```text id="r0yop4"
unknown
```

---

# 10. Job Evidence 不依赖主 PID

恢复过程中：

```text id="0bzxp2"
PID
Process Start Token
```

主要用于：

```text id="o2iyz3"
防 PID 重用
诊断
确认具体 Codex 主进程
```

但 Workspace Claim 的安全释放依据优先级为：

```text id="a3hfon"
Job-level evidence
>
Main-process evidence
```

主进程消失只能作为辅助证据。

---

# 11. Job 外部进程边界

Windows Job Object 和 Codex Background Terminal 管理共同覆盖 SerenaDesktop 受管 Codex Runtime。

V0.4 不宣称能够控制：

```text id="nsvsdt"
用户手工启动的外部终端

其他编辑器启动的进程

Windows Service

Scheduled Task

通过外部系统显式创建的独立服务
```

Workspace Claim 的保证范围是：

> SerenaDesktop 管理的 Codex Runtime、其 Job 进程树以及 Codex App Server 跟踪的 Background Terminals。

---

# 12. Execution 与 Runtime 归属

每个已经进入 Provider 派发阶段的 Execution 必须保存：

```text id="d6nt4c"
runtimeInstanceId
```

该字段一旦写入不可变。

所有 Provider 生命周期证据同时记录：

```text id="lcuk7f"
evidenceRuntimeInstanceId
```

包括：

```text id="kuge8r"
Turn Terminal Evidence

Background Terminal Cleanup Evidence

Runtime Termination Evidence
```

---

# 13. 同 Runtime Cleanup 原则

正常 Execution：

```text id="6hf2j1"
Execution.runtimeInstanceId = R1
```

则：

```text id="dr8u5j"
turn/completed

thread/backgroundTerminals/list

thread/backgroundTerminals/clean
```

形成的正常清理证据必须来自：

```text id="w0b3zc"
Runtime R1
```

保存：

```text id="oqd7u5"
backgroundCleanupRuntimeInstanceId = R1
```

只有：

```text id="4ha0jo"
backgroundCleanupRuntimeInstanceId
==
execution.runtimeInstanceId
```

才能作为正常 Runtime Cleanup 证据。

---

# 14. 跨 Runtime 恢复

假设：

```text id="9popht"
Execution E1
运行于 Runtime R1

SerenaDesktop 崩溃

新的 Runtime R2
恢复同一个 Thread
```

R2 得到：

```text id="7ko4un"
backgroundTerminals/list == []
```

不能证明：

```text id="42gek3"
R1 的后台进程已经结束
```

因此禁止：

```text id="5m3plw"
R2 cleanup evidence
替代
R1 termination evidence
```

---

# 15. 跨 Runtime Recovery 顺序

旧 Runtime 上存在未决 Execution 时：

```text id="qg4fzv"
Execution E1
Runtime R1
```

恢复顺序必须为：

```text id="ohm212"
1. 核对 R1 Runtime

2. 获得 R1 Job-level termination evidence

3. 确认旧 Runtime 不再可能写 Workspace

4. R2 initialize 成功后，thread/read(includeTurns=false)
   读取并验证 threadId、historyMode

5. 按 historyMode 选择第 40.3 节的只读恢复路径
   精确恢复 Execution.turn_id 的 terminal Turn 和 Final Result

6. R2 的 background terminal 状态只描述 R2
```

Final Result Recovery 不要求先 thread/resume；thread/resume 仅用于后续真正继续 Thread，不是历史结果读取的必要步骤。恢复结果成功不能替代 R1 Job-level termination evidence。

如果步骤 2 无法完成：

```text id="wqbpv6"
E1 = unknown
```

Workspace Claim 保留。

不会因为：

```text id="4bm7sw"
R2 list == []
```

释放。

---

# 16. Runtime Crash 后的 Cleanup

如果 R1 已获得：

```text id="fojnlq"
managed_job_destroyed
或
job_active_processes_zero
```

则对旧 Execution 可以认定：

```text id="2dvk6l"
R1 不再存在活动进程
```

这种情况下无需从 R2 获取：

```text id="1d3udq"
R1 Background Terminal Cleanup
```

因为 Runtime 级终止证据已经强于 Thread 级 Cleanup。

此时 R2 只用于：

```text id="m83129"
恢复 Thread Final Result
```

按第 15、40.3–40.5 节完成 historyMode-aware 只读恢复；仍需 exact threadId / Execution.turn_id、terminal status 无冲突及完整目标 Turn items。结果信息可随 interrupted Execution 保存，不改变第 26 节 Runtime-termination 仅 reconciling → interrupted 的要求。

---

# 17. Execution 状态更新统一入口

所有 Execution 状态变化必须走：

```text id="dqqqv9"
transition_execution()
```

以下组件不能直接覆盖 `status`：

```text id="tl03yo"
turn/start Response Handler

Turn Notification Handler

Cancel Handler

Interrupt Response Handler

Interrupt Timeout Handler

Recovery Manager
```

---

# 18. Provider Terminal Evidence

Execution 增加：

```text id="ft5dkj"
provider_terminal_status
provider_terminal_evidence_at
```

一旦：

```text id="eqv64u"
provider_terminal_status != NULL
```

说明 Provider Turn 已进入终态。

状态至少进入：

```text id="49gk3g"
finalizing
```

后续取消相关事件不得再退回执行阶段。

---

# 19. Cancel ACK 状态规则

`turn/interrupt` ACK 仅允许：

```text id="pa7o89"
cancel_requested
    ↓
cancelling
```

前提：

```text id="qtknt0"
provider_terminal_status == NULL
```

如果收到 ACK 时已经：

```text id="mxqh0c"
provider_terminal_status != NULL
```

或者 Execution 已：

```text id="gu9f37"
finalizing
completed
failed
cancelled
interrupted
```

则只记录：

```text id="8qdlgu"
interrupt_ack_at
```

不修改 Execution Status。

---

# 20. Cancel Timeout 状态规则

Interrupt Timeout 只允许：

```text id="8yje99"
cancel_requested
cancelling
```

且：

```text id="xggnr7"
provider_terminal_status == NULL
```

时转：

```text id="ptlqb1"
reconciling
```

如果 Provider Terminal Evidence 已经存在：

```text id="7p6okl"
保持 finalizing
```

只记录：

```text id="8ax9b4"
interrupt_timeout_at
interrupt_diagnostic
```

不得：

```text id="bz0k5e"
finalizing → cancelling

finalizing → reconciling
```

仅因 Cancel ACK 或 Timeout 发生。

---

# 21. Execution 阶段单调性

状态不是简单数值排序，而采用显式允许转换图。

V0.4 Execution Status 固定为第 39.2 节的 11 个值，不增加调度状态。`transition_execution()` 必须实现以下完整核心允许图；未列出的状态转换一律拒绝：

```text
dispatch_pending → running
running → cancel_requested
cancel_requested → cancelling

dispatch_pending → finalizing  # Provider terminal evidence 先于 turn/start ACK
running → finalizing
cancel_requested → finalizing
cancelling → finalizing

dispatch_pending → reconciling
running → reconciling
cancel_requested → reconciling
cancelling → reconciling
finalizing → reconciling

finalizing → completed | failed | cancelled | interrupted
reconciling → completed | failed | cancelled | interrupted | unknown
unknown → reconciling

# 第 25 节唯一的派发前取消特例
dispatch_pending → cancelled  # dispatch_state = not_dispatched
```

`completed / failed / cancelled / interrupted` 全部为 absorbing state，不允许再转换，包括恢复和迟到事件。重复请求读取既有状态不算状态转换；诊断补充不得改写终态。

`unknown → reconciling` 仅允许在重新获得新的可靠 Runtime / Job Evidence，或本地人工 Resolve 流程启动时发生。普通轮询、MCP 重试、等待时间和新 Runtime 启动均不能触发该转换。`finalizing → reconciling` 只用于收尾不确定性，不得由迟到 Cancel ACK/Timeout 触发。

任何进入安全业务终态的转换都必须具备完整 safe release evidence，并遵守 `finalize_and_release_execution()` 原子事务契约。第 25 节的派发前取消使用专门入口 `cancel_before_dispatch_and_release()`，以确定未派发作为安全证据，同样原子写终态和释放 Claim；不是绕过安全提交的普通状态更新。

核心不变量：

```text id="mi9h8u"
一旦取得 Provider Terminal Evidence

running-stage transitions permanently closed
```

即：

```text id="8uktim"
running
cancel_requested
cancelling
```

不再允许成为新状态。

之后只允许：

```text id="jbv25o"
finalizing
reconciling
safe terminal
unknown
```

---

# 22. Finalization

Provider Turn 终态后：

```text id="s0ribb"
finalizing
```

执行：

```text id="wepgwg"
Final Result Read（按第 40.3–40.5 节验证身份与 completeness）

Background Terminal Cleanup

Runtime Evidence Validation
```

当所有适用安全条件满足：

```text id="inl304"
safeToReleaseWorkspace = true
```

才进入业务终态。

---

# 23. 安全终态提交

Execution 进入：

```text id="dsbkuu"
completed
failed
cancelled
interrupted
```

且需要释放 Workspace Claim 时：

> 状态写入与 Claim 删除必须在同一个 SQLite 事务完成。

例如：

```text id="pszuov"
BEGIN IMMEDIATE

验证:
    execution.status == finalizing/reconciling
    safe_release_evidence == complete

UPDATE executions
SET
    status = :terminal,
    completed_at = :now,
    release_evidence_state = 'complete'

DELETE FROM workspace_claims
WHERE execution_id = :executionId

验证删除结果符合预期

COMMIT
```

不能：

```text id="lj3ucm"
COMMIT terminal
        ↓
另一个事务删除 Claim
```

---

# 24. Atomic Release

统一实现：

```text id="9wloc1"
finalize_and_release_execution()
```

负责：

```text id="iccrmk"
安全终态持久化

最终结果状态持久化

Release Evidence 持久化

Workspace Claim 删除
```

作为一个原子数据库操作。

如果事务失败：

```text id="3i8nvl"
Execution 保持 finalizing/reconciling

Claim 保持
```

可以安全重试。

---

# 25. Cancel-before-dispatch 原子释放

对于：

```text id="77bdx6"
dispatch_pending
+
not_dispatched
+
Claim 已持有
```

Cancel 同样使用事务：

仅当 `status = dispatch_pending AND dispatch_state = not_dispatched` 时允许调用 `cancel_before_dispatch_and_release()`。一旦 `dispatch_state != not_dispatched`，禁止使用该路径。条件更新须与派发条件更新原子竞争，并检查影响行数恰为 1；失败则不删除 Claim、不报告取消成功。派发已开始但尚无 Turn ID 时保留取消意图，等待可靠绑定或进入 reconciliation，不得伪装成派发前取消。

```text id="9hf95h"
BEGIN IMMEDIATE

条件更新:
dispatch_pending → cancelled

DELETE workspace_claim
WHERE execution_id = ?

COMMIT
```

因此不存在：

```text id="96mrj7"
cancelled Execution
+
遗留 Claim
```

的崩溃窗口。

---

# 26. Runtime termination 后原子释放

如果 Execution 因 Runtime 确认终止而收敛：

```text id="d34z4f"
reconciling
    ↓
interrupted
```

必须先持久化：

```text id="l2znjh"
runtimeTerminationEvidence = complete
```

然后同一事务：

```text id="b56sg1"
Execution → interrupted

DELETE workspace_claim
```

---

# 27. Claim Recovery

启动恢复不再只根据 Execution Status 扫描。

首先扫描：

```text id="00aitb"
workspace_claims
```

对每个 Claim：

```text id="fowfyc"
Claim
   ↓
Execution
   ↓
Release Evidence
```

分类处理。

---

# 28. Terminal Execution + Claim

在 V0.4 正常写入路径中：

```text id="6rrg19"
terminal Execution + Claim
```

不应该出现。

如果由于：

```text id="snef7s"
旧版本数据
迁移
数据库人工修改
```

发现：

```text id="dgb3ah"
Execution terminal
+
Claim still exists
```

只有当：

```text id="7qyxsz"
release_evidence_state == complete
```

时才能原子删除 Claim。

否则：

```text id="4c4tug"
WORKSPACE_CLAIM_INCONSISTENT
```

保持阻塞。

---

# 29. unknown Execution

`unknown` 永远不会自动删除 Claim。

解除方式仅有：

```text id="3q0vhj"
自动重新获得可靠 Runtime / Job Evidence

或

本地人工 Resolve Workspace Block
```

MCP 不提供 Force Unlock。

---

# 30. Runtime Evidence 优先级

Workspace Write Safety Evidence 强度：

```text id="kmtc3j"
同 Runtime：

Turn Terminal
+
Background Terminals Empty
        ↓
Safe

Runtime Crash：

Original Runtime Job-level Terminated
        ↓
Safe from old Runtime writes
```

以下都不足：

```text id="a1rv41"
Main PID exited

新 Runtime backgroundTerminals == []

turn/interrupt ACK

MCP Disconnect

等待了一段时间
```

---

# 31. Windows Runtime 验收标准

Windows 实现必须通过以下精确故障测试：

```text id="fezmmi"
Runtime DB row 创建后崩溃

Job 创建后崩溃

CreateProcessW 调用期间 Host 崩溃

CreateProcessW 返回后立即 Host 崩溃

PID 尚未来得及持久化时 Host 崩溃

Codex 主进程退出但 Job Child 仍存活

TerminateJobObject 后 Child 延迟退出

PID 被其他进程重用

旧 Runtime Job 仍存在

旧 Runtime Named Job 已销毁

标准流 HANDLE_LIST 只包含三个子进程端，JSON-RPC 双向通信及 stderr 捕获正常

Job Handle 不可继承且不在 HANDLE_LIST，Host 崩溃后不会被 Codex 持有

原 Session 中 Named Job 不存在且 Policy 完整，接受 managed_job_destroyed

恢复 Session 与 job_session_id 不同，即使查询返回 FILE_NOT_FOUND 也保持 unknown + Claim

Session 身份缺失或查询拒绝访问，不生成 managed_job_destroyed
```

其中最关键的验收：

> 不存在一个成功创建的 `codex.exe` 曾经在 SerenaDesktop Codex Job 之外运行的窗口。

---

# 32. Cancel 状态竞态测试

Fake App Server 必须覆盖：

```text id="ywyets"
turn/completed
先于
turn/interrupt ACK

turn/completed
先于
interrupt timeout handler

Execution 已 finalizing
随后收到 cancel ACK

Execution 已 finalizing
随后触发 cancel timeout
```

预期：

```text id="3uicf8"
finalizing 不倒退
```

---

# 33. Cross-Runtime Recovery 测试

场景：

```text id="s6kohp"
E1 → R1

R1 崩溃

R2 启动

R2 resume E1.thread
R2 backgroundTerminals/list = []
```

如果：

```text id="oc6a15"
R1 termination evidence 未完成
```

预期：

```text id="l0cshk"
E1 保持 unknown/reconciling
Workspace Claim 保持
```

R2 的空列表不改变这一结论。

---

# 34. Terminal + Claim Crash 测试

模拟：

```text id="ap2zs6"
Finalization 已完成
准备写 terminal

DB crash / process crash
```

由于：

```text id="kxdgls"
terminal update
+
Claim delete
```

处于同一事务：

最终数据库只能出现：

```text id="xjldz8"
A.
finalizing + Claim

或

B.
terminal + No Claim
```

不允许：

```text id="cqel3q"
terminal + Claim
```

作为 V0.4 正常提交结果。

---

# 35. 实现边界

V0.4 保持现有整体架构：

```text id="cixyis"
AgentTaskManager
CodexProvider
CodexAppServerClient
WorkspaceExecutionCoordinator
SQLite State Store
```

不新增调度层。

本轮重点是强化：

```text id="c9dhyw"
Process Creation Contract

Job Ownership Contract

Runtime Evidence Contract

State Transition Contract

Atomic Claim Release Contract
```

---

# 36. 核心可靠性不变量

实现必须始终满足：

```text id="tmom9t"
Codex process enters its Job at process creation.

Job handles are private and non-inheritable.

Only child standard-stream handles are inherited through an explicit handle list.

Named Job absence is evidence only in the verified original Windows Session namespace.

Breakaway is not permitted.

Main PID death is not Job termination evidence.

Job-level evidence owns Runtime termination truth.

Cleanup evidence belongs to the Runtime that produced it.

A new Runtime cannot prove an old Runtime is gone.

Provider terminal evidence permanently closes running-state transitions.

Cancellation diagnostics never regress finalization.

Safe terminal state and Workspace Claim release commit atomically.

Unknown side effects are never replayed or automatically unlocked.
```

# 37. Implementation Baseline

本节补充 V0.4 的实施基线。

第 1～36 节定义的：

```text
Process Creation Contract
Job Ownership Contract
Runtime Evidence Contract
State Transition Contract
Atomic Claim Release Contract
Cross-Runtime Recovery Contract
```

保持不变。

本节只解决：

```text
这些契约如何映射到当前 SerenaDesktop 代码库
```

不重新设计前述可靠性语义。

当前 SerenaDesktop 已存在：

```text
Tauri Application
SupervisorState
MCP Broker
Workspace 注册及活动项目管理
Serena Adapter
Git Builtin
CodeGraph Adapter
```

以下属于本次新增子系统，而不是既有组件：

```text
AgentTaskManager
CodexProvider
CodexAppServerClient
Codex Runtime
WorkspaceExecutionCoordinator
SQLite State Store
```

第 35 节列出的五个组件是目标架构名称，不表示当前仓库已有这些实现。其实施映射为：

> 保持现有 SerenaDesktop、MCP Broker 和 Workspace 架构不变，在其内部新增独立 Agent Runtime 子系统，不新增第二套 Broker 或独立调度服务。

---

# 38. Agent Runtime 模块边界

建议新增：

```text
src-tauri/src/agent/
├── mod.rs
├── task_manager.rs
├── coordinator.rs
├── execution.rs
├── store.rs
└── codex/
    ├── mod.rs
    ├── provider.rs
    ├── app_server.rs
    ├── protocol.rs
    ├── runtime.rs
    └── windows_launcher.rs
```

职责固定如下。

## AgentTaskManager

负责：

```text
创建 Execution
取消 Execution
恢复 Execution
调用 Provider
协调 Finalization
```

不直接操作 Win32 Job。

---

## WorkspaceExecutionCoordinator

负责：

```text
Workspace Claim 获取
Workspace Claim 冲突判断
Claim Recovery
Safe Release 判断
```

所有 Claim 最终释放必须通过：

```text
finalize_and_release_execution()
```

或：

```text
cancel_before_dispatch_and_release()
```

不得存在其他直接删除正常 Claim 的生产路径。

---

## CodexProvider

负责：

```text
Execution
        ↓
Codex Runtime
        ↓
Thread
        ↓
Turn
```

之间的 Provider 语义映射。

不直接实现 Win32 Process API。

---

## CodexAppServerClient

只负责：

```text
JSON-RPC framing
request / response correlation
notifications
initialize
thread/*
turn/*
backgroundTerminals/*
```

不拥有 Workspace Claim。

---

## Codex Runtime

负责：

```text
Codex Process
Job Object
stdio
Runtime Instance
Runtime termination
Runtime Evidence
```

它是 Job Handle 的唯一业务所有者。

---

## Windows Launcher

`windows_launcher.rs` 是 Codex Runtime 唯一合法的 Windows 进程创建入口。

当前 Serena Supervisor 中已有的：

```text
CreateProcess
        ↓
AssignProcessToJobObject
```

类进程 containment 实现不得用于 Codex Runtime。

当前代码定位为 `src-tauri/src/serena.rs` 的 `contain_process`。本次仅新增 Codex launcher，不修改 Serena 生命周期。`execution.rs` 定义状态、不可变归属和证据类型；`store.rs` 独占 SQL、迁移与事务；`app_server.rs` 实现 Reader/Writer 和消息分发，`protocol.rs` 保存固定版本协议类型。恢复由 task_manager 协调 runtime、coordinator 与 store，不新增独立服务。

Serena Process 生命周期与 Codex Runtime 生命周期是两套不同契约。

Codex Runtime 必须严格实现第 1～11 节的：

```text
PROC_THREAD_ATTRIBUTE_JOB_LIST
```

创建时绑定语义。

---

# 39. SQLite State Store

Agent Runtime 使用独立 SQLite State Store。

建议依赖：

```text
rusqlite
+
bundled SQLite
```

避免依赖机器上的外部 SQLite DLL。

具体 crate patch version 由实施时 Cargo.lock 固定，不在设计文档中锁死。

数据库访问统一经过：

```text
StateStore
```

不得让：

```text
Provider
AppServerClient
WindowsLauncher
UI
```

直接执行 SQL。

SQLite 阻塞操作不得直接长期占用 Tokio async worker。

---

## 39.1 runtime_instances

以下三个表及第 39.3 节 trigger 组成初始 Schema v1，按文档顺序执行。它们是本地 State Store Schema，不是 Codex API Schema。数据库位于应用数据目录的 `agent-state.db`，不放在项目目录。每个连接必须启用 `foreign_keys=ON`、`synchronous=FULL`，数据库使用 WAL，busy timeout 固定 5000 ms；事务失败必须完整回滚，不发送后续 Provider 请求。schema version 使用 `PRAGMA user_version=1`，仅在建表事务成功时提交。

ID 为本地生成的不透明 TEXT；业务时间为 Unix 毫秒 INTEGER。Process Start Token 保存 Creation FILETIME 的 16 位十六进制 TEXT，保留原始 64 位值，不转换为 Unix 毫秒。owner_host_instance_id 是创建 Host 的不透明身份快照，不依赖未建表的外键。

```sql
CREATE TABLE runtime_instances (
    id TEXT PRIMARY KEY NOT NULL,

    owner_host_instance_id TEXT NOT NULL,

    job_name TEXT UNIQUE,
    job_session_id INTEGER CHECK(job_session_id >= 0),

    job_creation_mode TEXT,
    job_handle_inheritable INTEGER,
    job_kill_on_close INTEGER,
    job_breakaway_allowed INTEGER,
    job_policy_verified_at INTEGER,
    codex_executable_path TEXT,
    codex_version TEXT,
    protocol_schema_sha256 TEXT,

    codex_pid INTEGER,
    codex_process_start_token TEXT,

    state TEXT NOT NULL CHECK(state IN
        ('preparing','starting','running','terminating','terminated','unknown')),

    started_at INTEGER,
    stopped_at INTEGER,

    termination_evidence_type TEXT,
    termination_evidence_at INTEGER,
    termination_evidence_state TEXT NOT NULL DEFAULT 'unknown'
        CHECK(termination_evidence_state IN ('unknown','complete')),
    last_error_code TEXT,
    last_error_message TEXT,

    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK(termination_evidence_state != 'complete' OR
        (termination_evidence_type IS NOT NULL AND termination_evidence_at IS NOT NULL)),

    CHECK (
        job_creation_mode =
        'proc_thread_attribute_job_list'
    ),

    CHECK (
        job_handle_inheritable = 0
    ),

    CHECK (
        job_kill_on_close = 1
    ),

    CHECK (
        job_breakaway_allowed = 0
    )
);
```

允许的 Runtime State 至少为：

```text
preparing
starting
running
terminating
terminated
unknown
```

`codex_process_start_token` 在 Windows 上定义为：

> 通过 `GetProcessTimes` 得到的 Creation FILETIME，将 high/low DWORD 合成为原始 64 位值，以固定 16 位十六进制 TEXT 保存并精确比较。

它和 PID 联合用于识别：

```text
原 Codex 主进程
```

但不作为 Job 已终止的充分证据。

---

## 39.2 executions

完整初始定义：Agent 身份和冻结配置作为 Execution 快照保存，Foundation 不依赖尚未实现的 Agent 表；同一 agent_id 后续执行必须由 State Store 校验 workspace、thread 和 profile 与已有快照一致。

```sql
CREATE TABLE executions (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    request_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    prompt TEXT NOT NULL,
    execution_profile_json TEXT NOT NULL,

    workspace_id TEXT NOT NULL,
    canonical_workspace_root TEXT NOT NULL,

    provider TEXT NOT NULL CHECK(provider = 'codex'),
    mode TEXT NOT NULL CHECK(mode IN ('read_only','workspace_write')),

    runtime_instance_id TEXT,

    thread_id TEXT,
    turn_id TEXT,

    status TEXT NOT NULL CHECK(status IN
        ('dispatch_pending','running',
         'cancel_requested','cancelling','finalizing','reconciling',
         'completed','failed','cancelled','interrupted','unknown')),
    dispatch_state TEXT NOT NULL DEFAULT 'not_dispatched'
        CHECK(dispatch_state IN ('not_dispatched','dispatching','dispatched','uncertain')),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(revision >= 0),

    provider_terminal_status TEXT,
    provider_terminal_evidence_at INTEGER,
    provider_terminal_evidence_runtime_instance_id TEXT,

    background_cleanup_runtime_instance_id TEXT,
    background_cleanup_evidence_at INTEGER,
    background_cleanup_state TEXT NOT NULL DEFAULT 'unknown'
        CHECK(background_cleanup_state IN ('unknown','accepted','polling','empty','uncertain')),

    runtime_termination_evidence_runtime_instance_id TEXT,
    runtime_termination_evidence_at INTEGER,

    release_evidence_state TEXT NOT NULL
        DEFAULT 'incomplete' CHECK(release_evidence_state IN ('incomplete','complete')),
    release_evidence_kind TEXT CHECK(release_evidence_kind IN
        ('not_dispatched','same_runtime_cleanup','runtime_terminated','operator_override')),
    release_evidence_json TEXT,

    final_result_json TEXT,
    result_completeness TEXT NOT NULL DEFAULT 'unknown'
        CHECK(result_completeness IN ('unknown','partial','complete')),
    started_at INTEGER,
    error_code TEXT,
    error_message TEXT,

    interrupt_requested_at INTEGER,
    interrupt_ack_at INTEGER,
    interrupt_timeout_at INTEGER,
    interrupt_diagnostic TEXT,

    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER,

    UNIQUE(agent_id, request_key),
    UNIQUE(id, canonical_workspace_root),
    FOREIGN KEY(runtime_instance_id) REFERENCES runtime_instances(id) ON DELETE RESTRICT,
    FOREIGN KEY(provider_terminal_evidence_runtime_instance_id) REFERENCES runtime_instances(id),
    FOREIGN KEY(background_cleanup_runtime_instance_id) REFERENCES runtime_instances(id),
    FOREIGN KEY(runtime_termination_evidence_runtime_instance_id) REFERENCES runtime_instances(id),
    CHECK(background_cleanup_state != 'empty' OR
        (runtime_instance_id IS NOT NULL AND
         background_cleanup_runtime_instance_id IS NOT NULL AND
         background_cleanup_runtime_instance_id = runtime_instance_id AND
         background_cleanup_evidence_at IS NOT NULL)),
    CHECK(release_evidence_state != 'complete' OR
        (release_evidence_kind IS NOT NULL AND release_evidence_json IS NOT NULL))
);
CREATE INDEX executions_runtime_state ON executions(runtime_instance_id, status);
CREATE UNIQUE INDEX executions_one_unresolved_per_agent ON executions(agent_id)
WHERE status NOT IN ('completed','failed','cancelled','interrupted');
```

`provider` V0.4 只允许：

```text
codex
```

Execution State 完整集合固定为以上 CHECK 中的 11 个值：

```text
dispatch_pending

running

cancel_requested
cancelling

finalizing
reconciling

completed
failed
cancelled
interrupted

unknown
```

所有状态变化必须经过：

```text
transition_execution()
```

---

## 39.3 runtimeInstanceId 不可变

允许：

```text
NULL
↓
R1
```

表示 Execution 第一次真正进入 Provider Dispatch。

一旦：

```text
runtime_instance_id != NULL
```

之后不得修改为其他 Runtime。

除应用层检查外，SQLite 增加防御性 Trigger：

```sql
CREATE TRIGGER prevent_execution_runtime_rebind
BEFORE UPDATE OF runtime_instance_id
ON executions
WHEN
    OLD.runtime_instance_id IS NOT NULL
    AND NEW.runtime_instance_id
        IS NOT OLD.runtime_instance_id
BEGIN
    SELECT RAISE(
        ABORT,
        'execution runtime instance is immutable'
    );
END;
```

Cross-Runtime Recovery 不修改该字段。

该 trigger 允许 NULL→NULL、NULL→R1、R1→R1，拒绝 R1→R2 和 R1→NULL。禁止通过 INSERT OR REPLACE 或删除重建绕过归属；State Store 不提供替换 Execution API。`dispatch_state: not_dispatched → dispatching` 的条件更新与首次 Runtime 绑定处于同一事务，要求 Runtime 已 running 且 Claim 属于该 Execution；更新行数不为 1 时禁止发送。此时 `status` 仍为 `dispatch_pending`，不能写入同名 dispatching Status。SQL CHECK/trigger 是底线，不代替第 17～26 节的状态转换和证据核验。

例如：

```text
E1.runtimeInstanceId = R1

R2 恢复 E1 Thread
```

仍保持：

```text
E1.runtimeInstanceId = R1
```

R2 产生的信息只能作为恢复结果信息，不能冒充 R1 Runtime Evidence。

---

## 39.4 workspace_claims

```sql
CREATE TABLE workspace_claims (
    canonical_workspace_root TEXT PRIMARY KEY NOT NULL,

    execution_id TEXT NOT NULL UNIQUE,
    claim_type TEXT NOT NULL CHECK(claim_type = 'exclusive_execution'),

    acquired_at INTEGER NOT NULL,

    FOREIGN KEY(execution_id, canonical_workspace_root)
        REFERENCES executions(id, canonical_workspace_root) ON DELETE RESTRICT
);
```

canonical root 使用现有 Workspace 规范化身份，不能用显示名或 workspace_id 别名代替互斥键。一个 canonical Workspace 同时最多一个受管 Execution Claim；同一目录的不同注册别名不能绕过互斥。

Claim 获取必须通过事务完成。

Claim 释放必须遵守第 23～29 节。

---

## 39.5 Evidence Migration Rule

所有具有安全意义的 Evidence 字段遵守：

> 不允许通过 Schema Migration 为历史记录补造可信 Evidence。

例如历史记录缺少：

```text
job_session_id
job_creation_mode
job_handle_inheritable
job_kill_on_close
job_breakaway_allowed
```

不能在 Migration 时简单填入：

```text
当前版本的默认安全值
```

并据此释放 Workspace。

历史 Evidence 缺失时：

```text
unknown
```

优先于推测。

Schema 中 Job Policy 字段允许 NULL，专门表示历史证据缺失；没有可信默认值。新 Runtime 在 preparing 阶段显式保存预期 Policy，Win32 配置成功后记录 job_policy_verified_at；预期值不等于已执行证据。历史导入不得填入当前 Session、当前 Runtime ID、当前安全 Policy、empty、complete 或伪造时间。缺失字段保持 NULL、证据状态 unknown，旧未决 Execution 保持 unknown 和 Claim；无法确定归属时停止迁移并报告，不能删除记录解锁。新创建路径缺失必需 Policy 时拒绝启动。迁移须事务化，失败保留旧库及版本，不部分应用；不以数据迁移绕过第 8～9 节核验。

State Store 验证包括：执行上述 DDL、外键、NULL→R1 与重绑定拒绝、同 canonical root 冲突、终态与 Claim 删除事务回滚、历史 NULL Evidence 不产生 release permission。只重试确认回滚的数据库事务，不重试未知 Provider 派发。

---

## 39.6 Dispatch State 契约

`dispatch_state` 固定为以下四值，与 Execution lifecycle 独立：

| 值 | 语义 |
| --- | --- |
| not_dispatched | 尚未进入任何可能产生 Provider 副作用的发送边界 |
| dispatching | 已经跨过安全重放边界，正在向原 Runtime 派发 |
| dispatched | 完整请求已经写入并 flush 成功 |
| uncertain | 无法证明请求未产生 Provider 副作用 |

唯一允许的状态变化：

```text
not_dispatched → dispatching
dispatching → dispatched
dispatching → uncertain
```

禁止其他变化，尤其禁止 `uncertain → not_dispatched / dispatching / dispatched` 和 `dispatched → not_dispatched`。迟到响应可以补充证据，不能将 uncertain 改成 dispatched。进程崩溃、stdio 故障或恢复时发现持久化 dispatching，必须转为 uncertain；若完整写入和 flush 成功后尚未落盘便崩溃，也按 uncertain 处理。已持久化 dispatched 的请求故障后仍不得重放，由 Execution 进入 reconciliation。成功 flush 不代表 Turn running，更不代表任务完成。

创建 Execution 的初值为 `status=dispatch_pending, dispatch_state=not_dispatched`。初始化、等待必要前提不新增 Execution Status；V0.4 不新增调度层。任何可能产生 Provider 副作用的发送前必须先持久化 dispatching；uncertain 请求不得自动重放。状态与派发确定性由 State Store 同一统一入口校验，不能由 Writer/Response Handler 各自覆盖。

## 39.7 Request Key 幂等与 Agent 串行

`(agent_id, request_key)` 是 Execution 创建的幂等键：

```text
相同键 + request_hash 相同
    → 返回已有 Execution，不创建新 Execution，不再次 Provider Dispatch

相同键 + request_hash 不同
    → EXECUTION_REQUEST_KEY_CONFLICT，不创建新 Execution，不 Provider Dispatch
```

State Store 使用 `BEGIN IMMEDIATE`，在同一事务内查键、比 hash，并仅在不存在时检查 Agent unresolved 约束和 INSERT；不得在事务外先查询再无条件 INSERT。已有键的幂等判定先于 unresolved 检查，避免重试被误判为 Agent busy。request_hash 基于规范化完整创建输入，缺省值规范化规则固定，不能仅 hash prompt。事务失败不派发，唯一约束为并发创建提供数据库底线。

保留 `executions_one_unresolved_per_agent`：每个 agent_id 同时最多一个 unresolved Execution。只有 completed、failed、cancelled、interrupted 为 resolved；unknown 仍为 unresolved，继续阻止该 Agent 新建 Execution。需要并行必须使用不同 agent_id。

Agent serialization 约束 Agent；Workspace Claim 约束 Workspace 写入所有权，两者独立，不能互相替代。不同 agent_id 操作相同 Workspace 仍必须遵守已有 Claim 契约。

---

# 40. Codex App Server Protocol Contract

V0.4 固定使用：

```text
codex app-server
```

的 stdio transport。

启动时显式指定 stdio transport，具体等价命令为：

```text
codex.exe app-server --listen stdio://
```

启动过程不得经过：

```text
cmd.exe
PowerShell
shell command string
```

---

## 40.1 Transport Framing

App Server stdio 使用：

```text
newline-delimited JSON
JSONL
```

规则：

```text
一行
=
一个完整 JSON-RPC Message
```

不得：

```text
按任意 read() chunk
直接解释为一个 JSON-RPC Message
```

必须先完成按换行 framing。

stdin：

```text
SerenaDesktop
    ↓
Codex App Server
```

stdout：

```text
Codex App Server
    ↓
SerenaDesktop JSON-RPC Reader
```

stderr：

```text
独立读取
    ↓
Runtime Log
```

stderr 不能混入 JSON-RPC Parser。

---

## 40.2 Managed Thread Creation Contract

SerenaDesktop V0.4 新创建的 managed Codex Thread 必须显式发送：

```json
{
  "ephemeral": false,
  "historyMode": "paginated"
}
```

不得依赖 Codex 的默认 historyMode。thread/start Response 必须验证返回的 threadId 及 `historyMode == paginated`；模式不符返回 `CODEX_APP_SERVER_INCOMPATIBLE`，不得 Dispatch Execution。

Legacy 仅作为已有或明确 legacy Thread 的兼容读取模式。禁止 paginated 失败后自动创建/切换为 legacy 或尝试 legacy reader。

## 40.3 HistoryMode-aware Final Result Recovery

跨 Runtime 恢复先满足第 15 节的 R1 Job-level termination evidence，再完成 R2 initialize。R2 通过 `thread/read(includeTurns=false)` 读取 metadata，验证 `threadId == Execution.thread_id`，并按真实返回的 historyMode 选择以下只读路径。未知/缺失模式或身份不符为 `CODEX_APP_SERVER_INCOMPATIBLE`，不得猜测模式。无需先 thread/resume，也不得为获取历史结果重放 Turn。

### Paginated

当且仅当 metadata.historyMode 为 paginated：

```text
thread/read(includeTurns=false) → metadata
thread/turns/list → persisted Turns
thread/items/list(threadId, turnId=Execution.turn_id) → persisted ThreadItems
```

禁止把 paginated `thread/read(includeTurns=true)` 用作历史恢复 API；该组合不属于 V0.4 必需 compatibility contract。

- 以 `Execution.turn_id` 为唯一目标 Turn 身份；不得用 latest Turn、最后一个 Turn 或最新 completed Turn 替代。缺失目标身份不得推断。
- thread/turns/list 从 cursor=null 开始，分页直到找到 exact turnId，或完整扫描结束。找到目标即可停止 Turn 定位；只有 explicit nextCursor:null 才能证明未找到目标的完整扫描结束。每个已读取页面仍须满足第 40.4 节。
- 完整扫描找不到目标时，`result_completeness` 不得为 complete；进入 reconciliation / 保持安全状态，不伪造结果。
- 验证 `turn.id == Execution.turn_id` 且 status 为固定协议的 terminal status（completed、interrupted、failed；inProgress 不是 terminal）。恢复出的 terminal status 必须与已有 Provider Terminal Evidence 相容；冲突返回 `CODEX_APP_SERVER_INCOMPATIBLE` 或稳定 protocol/evidence conflict error，不能覆盖已有 Evidence。
- 找到目标后，使用 exact threadId 和 turnId 调用 thread/items/list，读取所有分页；响应 entry 的 turnId 必须属于目标 Turn。不能混入其他 Turn 的 items，也不能用 summary items 代替尚未完整取得的目标 Turn items。

### Legacy

当且仅当真实 metadata.historyMode 为 legacy，调用 `thread/read(includeTurns=true)`，验证返回 threadId/historyMode，并从返回 turns 中按 `Execution.turn_id` 精确定位目标。目标不存在、非 terminal 或与已有 Evidence 冲突时不得形成 complete；目标终态及 Provider Terminal Evidence 冲突校验与 paginated 相同，必须取得完整目标 Turn items。

不得切换真实 historyMode，不得在 paginated API 失败时尝试 legacy reader。该兼容读取模式不是新 managed Thread 的创建默认值。

## 40.4 Result Recovery Pagination Contract

thread/turns/list 与 thread/items/list 沿用固定 binary 的严格分页契约：客户端明确区分 nextCursor Missing、explicit Null、Cursor(String)，不得使用丢失字段存在性的 Option 解析。

- 只有 explicit null 表示当前方向分页结束；string 表示继续。data=[] 不能单独证明结束。
- missing 或 malformed cursor 使扫描 invalid，返回 `CODEX_APP_SERVER_INCOMPATIBLE` 或稳定 protocol contract error；不能将 missing 归一化为 null。
- 检测 repeated cursor、RPC error、timeout、oversized response、Runtime identity change；任一发生均为 Result Recovery incomplete，不能伪造 complete。分页绑定同一恢复 Runtime、threadId 和 exact target turnId，不拼接不同 Runtime 的页面。
- 使用第 43 节的有界消息、队列和控制 RPC deadline；完整恢复操作也必须有总 deadline，不允许各页重新计时而形成无界恢复。

## 40.5 Result Identity / Completeness

稳定身份为：Thread identity = threadId；Execution result identity = turnId。Item ID 不属于跨 Runtime Result Recovery 的稳定身份契约，不得要求 live ThreadItem ID 等于 persisted ThreadItem ID。已验证 legacy live ID 为 msg_...、历史为 item-2，而目标 Turn、终态、final phase 和 Final Agent Result 一致。

`final_result_json` 必须明确绑定 threadId、turnId、historyMode、terminal Turn，以及完整 target-Turn persisted items / protocol-derived final result；不能仅保存无身份来源的文本字符串。具体 Rust representation 留给实现。

只有 exact target turn found + terminal status verified + 所需 items 完整（分页路径完整结束）+ 无 Evidence conflict，才允许 `result_completeness = complete`。其余只能为 partial 或 unknown。结果恢复成功不等于 Workspace safe release：Job evidence、Background Terminal Cleanup、Atomic Claim Release、immutable runtime_instance_id、Cross-Runtime Evidence ownership 和 unknown fail-closed 契约均保持不变。

---

# 41. App Server Initialize

每个新 Runtime 只允许一次初始化握手：

```text
CreateProcess
    ↓
stdio ready
    ↓
initialize
    ↓
initialize response
    ↓
initialized notification
    ↓
Runtime running
```

初始化请求至少携带：

```json
{
  "method": "initialize",
  "id": 1,
  "params": {
    "clientInfo": {
      "name": "serena-desktop",
      "title": "SerenaDesktop",
      "version": "<desktop-version>"
    },
    "capabilities": {
      "experimentalApi": true
    }
  }
}
```

收到成功 Response 后发送：

```json
{
  "method": "initialized"
}
```

在：

```text
initialize response
+
initialized
```

完成前：

```text
不得创建 Thread
不得 Dispatch Execution
不得标记 Runtime running
```

之所以强制：

```text
experimentalApi = true
```

是因为 V0.4 Runtime Cleanup Contract 使用：

```text
thread/backgroundTerminals/list
thread/backgroundTerminals/clean
```

这些属于 Codex App Server Experimental API。

如果当前固定 Codex 版本不支持这些 API：

```text
CODEX_APP_SERVER_INCOMPATIBLE
```

Runtime 不进入 running。

---

# 42. Background Terminal Cleanup Contract

`thread/backgroundTerminals/clean` 返回成功：

```json
{}
```

只表示：

```text
cleanup request accepted
```

不得立即形成：

```text
BACKGROUND_TERMINALS_EMPTY
```

Evidence。

正常 Cleanup 必须执行：

```text
thread/backgroundTerminals/clean
        ↓
bounded polling
        ↓
thread/backgroundTerminals/list
        ↓
读取所有分页
        ↓
全部 data == []
        ↓
Background Cleanup Evidence complete
```

`thread/backgroundTerminals/list` 必须处理：

```text
cursor
limit
nextCursor
```

直到：

```text
nextCursor == null
```

才完成一次完整快照。

只有一次完整扫描中：

```text
所有页面 data 均为空
```

才能形成：

```text
backgroundCleanupRuntimeInstanceId
=
execution.runtimeInstanceId
```

正常清理证据。

初版 cleanup 总 deadline 为 30 秒，包含 clean、所有 list 分页和等待；轮询间隔 250 ms，list 的 limit 固定为 100（须经第 49 节固定版本验证），每次 RPC 使用剩余总预算与 15 秒中较小值。每轮从 cursor=null 开始，依 nextCursor 获取全部分页；任意一页非空则本轮不形成 empty evidence，下轮重新从头扫描。重复 cursor、字段缺失、超时或 Runtime 更换均使本次扫描无效，进入 reconciliation 并保持 Claim；不能无限循环，不能把不同轮次或不同 Runtime 的页面拼成空结果。clean 成功只记录 accepted，empty 必须记录同一原 Runtime、Thread、Execution、完成扫描时间及分页完整性。

如果出现：

```text
timeout
RPC error
stdio disconnect
invalid response
Runtime crash
分页无法完成
```

则本次 Background Cleanup：

```text
incomplete
```

不能释放 Claim。

如果原 Runtime 已取得：

```text
job_active_processes_zero
```

或：

```text
managed_job_destroyed
```

则按照第 16 节：

```text
Runtime-level termination evidence
```

强于 Thread-level Background Cleanup。

---

# 43. JSON-RPC Reader / Writer

CodexAppServerClient 内部至少包含：

```text
Writer
Reader
Pending Request Map
Notification Dispatcher
Runtime Cancellation Token
```

## Writer

负责：

```text
request serialization
request id allocation
stdin write
newline append
flush
```

同一 Runtime 的 stdin write 必须串行。

---

## Reader

负责：

```text
stdout JSONL framing
JSON decode
Response correlation
Notification dispatch
```

Response：

```text
存在 id，不存在 method，且 result/error 恰有一个
```

根据 request id 唤醒等待方。

Notification：

```text
method != null
id == null
```

交由 Notification Handler。

同时存在 id 和 method 是 Server Request，必须交给独立 Server Request Handler，按固定版本的拒绝/取消响应类型回答；未知请求返回协议错误并判断兼容性，不能静默丢弃或自动批准。未知 Notification 可以忽略，已使用的核心事件缺少必需字段则视为协议不兼容。

Pending Request Map 按 Runtime 内单调递增 ID 建立，必须先登记再排队写入；每项保存 method、deadline、响应等待方及 Execution 关联。Writer 单任务写完整 UTF-8 JSON 加 LF 并 flush，禁止交错写；请求响应与 Server Request 回答共用 Writer。Reader 不能等待模型任务或数据库事务，分发至有界队列；Notification Dispatcher 按 Execution 串行进入 transition_execution。注册 thread→Execution 路由后才发送 turn/start，迟到 ACK 只补充证据。

初始固定上限：单条入站/出站消息 16 MiB（含终止 LF），Writer 与事件队列各 128 条、各累计最多 32 MiB，Pending Request Map 最多 128 项，stderr tail 64 KiB。Reader 在累积未换行字节时即检查上限，不能等 read_line 分配完再检查；允许 CRLF，拒绝非法 UTF-8/JSON 和 EOF 残帧。写入前做出站大小校验。队列超限或无法排空必须明确失败进入 reconciliation，不能丢弃终态或无界扩容。

初始化总 deadline 30 秒，普通控制 RPC 15 秒；模型 Turn 不套用 RPC deadline，turn/start ACK 与 Turn 完成分开。响应超时移除等待项并标记派发是否 uncertain，不自动重发 turn/start；迟到响应只能补充同 Runtime 证据。Runtime 故障时停止接受新派发，唤醒所有 Pending Request 失败并保留关联 Claim；stdio EOF、非法 JSON、初始化失败和协议不兼容均进入原 Runtime 的 Job-level reconciliation。Job 终止无法证实时保持 unknown，不能仅关闭客户端后启动 R2 解除阻塞。

---

## Protocol Failure

以下情况视为 Runtime Protocol Failure：

```text
stdout EOF while Runtime should be alive

invalid JSON

invalid JSON-RPC shape

duplicate impossible response

initialize rejected

experimental API unavailable

single message exceeds configured limit
```

处理：

```text
Runtime
    ↓
terminating / unknown
    ↓
Job-level reconciliation
```

不得简单重启一个 R2 并认为 R1 已结束。

单条 JSONL Message 必须设置显式最大长度。

初版冻结值：

```text
16 MiB
```

超过限制：

```text
CODEX_PROTOCOL_MESSAGE_TOO_LARGE
```

不得无界缓存 stdout。

---

# 44. Windows Launcher 精确实施契约

Codex Runtime 不使用：

```text
tokio::process::Command::spawn()
```

作为最终 CreateProcess 实现。

Windows 下直接封装 Win32：

```text
CreateJobObjectW
SetInformationJobObject
CreatePipe
SetHandleInformation
InitializeProcThreadAttributeList
UpdateProcThreadAttribute
CreateProcessW
GetProcessTimes
QueryInformationJobObject
TerminateJobObject
```

---

## 44.1 Pipe Handle

stdin：

```text
Parent:
    stdinWrite

Child:
    stdinRead
```

stdout：

```text
Child:
    stdoutWrite

Parent:
    stdoutRead
```

stderr：

```text
Child:
    stderrWrite

Parent:
    stderrRead
```

只有：

```text
stdinRead
stdoutWrite
stderrWrite
```

允许继承。

父进程端：

```text
stdinWrite
stdoutRead
stderrRead
```

必须：

```text
HANDLE_FLAG_INHERIT = false
```

Job Handle 同样：

```text
inheritable = false
```

---

## 44.2 Attribute List

一次 `STARTUPINFOEX` 同时配置两个 Attribute：

```text
PROC_THREAD_ATTRIBUTE_JOB_LIST
=
[codexJob]

PROC_THREAD_ATTRIBUTE_HANDLE_LIST
=
[
    stdinRead,
    stdoutWrite,
    stderrWrite
]
```

Job Handle 不得出现在：

```text
HANDLE_LIST
```

中。

CreateProcessW 使用：

```text
EXTENDED_STARTUPINFO_PRESENT
|
CREATE_NO_WINDOW
```

并且：

```text
bInheritHandles = TRUE
```

---

## 44.3 Process Command Line

`lpApplicationName` 使用：

```text
codex.exe 的绝对路径
```

不得依赖：

```text
PATH 搜索
Shell 解析
```

argv 独立构建。

Windows command-line quoting 必须使用专门实现并有单元测试，不允许简单：

```text
args.join(" ")
```

至少覆盖：

```text
普通参数
带空格路径
双引号
反斜杠
空字符串
Unicode 路径
```

---

# 45. Named Job Collision

创建：

```text
Local\SerenaDesktop.Codex.<runtimeInstanceId>
```

后必须检查：

```text
GetLastError()
```

如果：

```text
ERROR_ALREADY_EXISTS
```

不得复用该 Job。

返回：

```text
CODEX_JOB_NAME_COLLISION
```

并进入 Runtime Recovery / fail-closed 路径。

`CreateJobObjectW` 成功后立即读取 `GetLastError`，不得先调用其他可能覆盖错误码的 API。碰撞时关闭本次获得的句柄，但不得 SetInformation、Terminate 或复用同名旧 Job；记录本次启动失败。旧 Runtime 的恢复仍只能依据其原始记录和第 8～9 节证据。

一个新 Runtime Instance：

```text
永远不能接管
同名旧 Job
```

---

# 46. Process Creation Success Boundary

唯一认为：

```text
Codex Runtime process exists
```

的边界是：

```text
CreateProcessW == success
```

因为 Job 已通过：

```text
PROC_THREAD_ATTRIBUTE_JOB_LIST
```

参与创建，所以一旦 CreateProcessW 成功：

```text
codex.exe
```

从第一个可运行时刻即属于 Job。

CreateProcessW 成功后立即：

```text
关闭 Parent 持有的 Child pipe ends

stdinRead
stdoutWrite
stderrWrite
```

Parent 保留：

```text
stdinWrite
stdoutRead
stderrRead

Process Handle
Job Handle
```

随后立即读取：

```text
PID
Process Creation Time
```

并持久化 Runtime。

---

# 47. Runtime Creation Failure

Runtime Creation 的每一步都必须可失败收敛。

例如：

```text
DB runtime row 已创建
        ↓
Job 创建成功
        ↓
Pipe 创建失败
```

此时：

```text
关闭 Job Handle
关闭 Pipe Handles
更新 Runtime 状态
```

不能残留受管资源。

如果：

```text
CreateProcessW
```

尚未成功：

```text
不存在 Codex Process
```

可以安全释放本次创建资源。

如果 CreateProcessW 已成功：

```text
后续任何失败
```

均必须：

```text
TerminateJobObject
        ↓
ActiveProcesses == 0
        ↓
记录 Runtime termination evidence
```

之后才允许认为 Runtime 已收敛。

---

# 48. Stable Runtime Error Codes

至少固定以下错误：

```text
CODEX_JOB_AT_CREATION_UNSUPPORTED

CODEX_JOB_NAME_COLLISION

CODEX_JOB_CREATE_FAILED

CODEX_PROCESS_CREATE_FAILED

CODEX_PROCESS_IDENTITY_FAILED

CODEX_APP_SERVER_INIT_FAILED

CODEX_APP_SERVER_INCOMPATIBLE

CODEX_PROTOCOL_INVALID_MESSAGE

CODEX_PROTOCOL_MESSAGE_TOO_LARGE

CODEX_STDIO_EOF

CODEX_RUNTIME_TERMINATION_TIMEOUT

CODEX_RUNTIME_EVIDENCE_INCOMPLETE

WORKSPACE_CLAIM_INCONSISTENT
```

错误码属于程序契约。

可读错误文本可以演进。

不得通过解析错误文本进行状态判断。

---

# 49. Codex Version Contract

SerenaDesktop Release 必须针对一个明确测试过的 Codex CLI / App Server 版本完成兼容性验收。

运行时至少执行：

```text
codex --version
```

并记录到日志。

V0.4 不要求实现通用：

```text
任意 Codex 版本自动兼容
```

如果 App Server initialize 或必需方法与当前契约不兼容：

```text
CODEX_APP_SERVER_INCOMPATIBLE
```

不得降级绕过：

```text
Background Terminal Evidence
Job Evidence
Atomic Claim Release
```

等安全契约。

Codex Experimental API 的变化必须作为后续 Runtime Contract 变更处理。

本次依据[官方 App Server 文档](https://learn.chatgpt.com/docs/app-server)核对了 stdio JSONL、初始化顺序及后台终端分页能力；本文请求示例不是已固定版本的完整 API Schema。`clean` 成功仅被本控制面视为 accepted，这是安全证据规则，不是对其所有内部行为的推测。

实施 Phase 3 前必须选择并记录实际 Codex 精确版本、codex.exe 路径和二进制摘要，使用该版本导出的 JSON Schema/协议定义（先核验该版本工具用法）及真实 Contract Test 冻结 request、response、Server Request、Notification 字段和终态映射。记录 schema SHA-256、Windows 版本及测试结果。未验证版本不进入兼容白名单；SerenaDesktop release 只承诺兼容该白名单版本，不承诺任意 Codex App Server 版本。

如固定版本在初始化、后台清理分页、归属或终态语义上与正文不同，立即停止受影响实现并报告 **Material Contract Difference**：实际版本与证据、具体字段/行为差异、影响的正文契约、需要用户决定的事项。不得猜测 Schema、删减分页验证、把 accepted 当 empty、自动换执行路径或弱化任何可靠性契约。无关且已授权的 State Store 工作可继续。

---

## 49.1 Approved History Recovery Contract Amendment

本有界 Amendment 为 **HistoryMode-aware Final Result Recovery Contract**，依据固定候选：

- version: codex-cli 0.153.4
- binary SHA-256: 444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B
- protocol schema SHA-256: B06F77062369D481A59CC70720C12B89CB9DD49C385863923262102D3AD6C978
- exact source commit: 3d2ee51ca2d5db578f328aa75e20aa22c0197c9a

历史事实保持：paginated Thread 的 pre-Turn `thread/read(includeTurns=true)` 曾真实返回 `-32601 / list_turns is not supported yet`。原始 [MCD Evidence](tasks/evidence/TASK-005/codex-0.153.4/thread-read-verification/material-contract-difference.md) 不删除、不覆盖，不要求本 Amendment 解释其内部 backend 根因。该组合已明确不属于 V0.4 必需兼容能力，因此不阻塞新的第 40.3 节契约。

[专项真实 Contract Evidence](tasks/evidence/TASK-005/codex-0.153.4/history-recovery-verification/verification.md) 已证明 paginated R1/R2 与 legacy R1/R2 均能恢复相同 threadId、turnId、terminal status、final phase、Final Agent Result。Legacy Item ID 不稳定另行记录，不被误报为稳定。

该历史读取 MCD 的 Resolution 为 `RESOLVED_BY_HISTORY_MODE_AWARE_FINAL_RESULT_RECOVERY_CONTRACT`。先前 nextCursor MCD 的 `RESOLVED_BY_NARROWER_FIXED_BINARY_CONTRACT` 继续有效；原始 Schema 允许 omitted 的事实、严格 raw-wire explicit null 规则和 MULTI_PAGE_WIRE = UNAVAILABLE 均保留。专项 PASS 不等于完整 TASK-005 Contract Gate PASS，生产实现仍须在 Amendment Review 后另行继续实施/验证/Review/用户验收。

---

# 50. 实施顺序

实施不得从 UI 开始。

## Phase 1 — State Store

先完成：

```text
SQLite
runtime_instances
executions
workspace_claims

transition_execution()

finalize_and_release_execution()

Claim Recovery
```

并完成纯数据库 Crash Transaction 测试。

Phase 1 必须增加以下 State Store 验收，不以只测成功路径代替：

- 第 21 节完整 allowed transition matrix，以及 11 个 Status 间所有未列出的 forbidden transition；派发前取消特例单独验证。
- Provider Terminal Evidence 出现后禁止 running-stage regression；四个 terminal absorbing，迟到事件不能改变终态。
- 四个 Dispatch State 的全部允许/禁止转换；dispatching crash → uncertain；uncertain 不允许 replay，迟到 ACK 不改变该确定性状态。
- cancel-before-dispatch 仅允许 dispatch_pending + not_dispatched；与派发原子竞争失败时 Claim 保持。
- 相同 request key + same hash 返回原 Execution；不同 hash 返回 EXECUTION_REQUEST_KEY_CONFLICT；并发重复提交仅创建一次且不重复派发。
- 同 agent 第二个 unresolved Execution 被拒绝；unknown 继续阻止新 Execution；原请求幂等重试仍返回原记录。
- 不同 Agent 不绕过相同 canonical Workspace Claim；上述检查不改变已冻结的原子 Claim Release 测试。

---

## Phase 2 — Windows Runtime Launcher

只实现：

```text
Create Job
Create Pipes
CreateProcessW
Job-at-creation
stdio
TerminateJob
Query ActiveProcesses
Process Start Token
```

先使用测试 Child Process，不接 Codex。

必须优先通过：

```text
Host 崩溃
        ↓
Child Process Tree 消失
```

测试。

---

## Phase 3 — Codex App Server Client

实现：

```text
JSONL
initialize
initialized
request/response
notifications
thread/start
thread/resume
thread/read（metadata；legacy includeTurns）
thread/turns/list
thread/items/list
turn/start
turn/interrupt
backgroundTerminals/list
backgroundTerminals/clean
```

使用真实固定版本 Codex 做 Contract Test。新 managed Thread 必须显式 ephemeral=false / historyMode=paginated 并验证返回模式。验证真实 terminal Turn 后，R1 的 metadata/turns/items 读取、R1 Job-level termination evidence，以及独立 R2 对同一持久化 Thread 的只读恢复；精确比较 threadId、turnId、terminal status 相容性与 Final Result。保留明确 legacy 的 R1/R2 includeTurns 兼容测试，不作为 production fallback。使用 Fake Server 验证目标 Turn 不在首页、exact turnId 不存在、终态 Evidence 冲突、items 多页完整性及第 40.4 节所有失败条件。

---

## Phase 4 — Minimal Execution Vertical Slice

完成：

```text
Acquire Workspace Claim
        ↓
Create Runtime R1
        ↓
initialize
        ↓
thread/start
        ↓
turn/start
        ↓
turn/completed
        ↓
background cleanup
        ↓
finalize_and_release_execution
```

先只实现一个完整成功路径。

---

## Phase 5 — Cancellation

实现：

```text
cancel-before-dispatch

turn/interrupt

cancel ACK race

cancel timeout

provider terminal race
```

并通过第 32 节竞态矩阵。

---

## Phase 6 — Crash Recovery

实现：

```text
Runtime Recovery
Named Job Recovery
Cross-Runtime Recovery
unknown Execution
Claim Recovery
```

逐项完成第 31、33、34 节故障测试。

---

## Phase 7 — Product Integration

只有第 51 节 Runtime Foundation Gate 通过后，才接：

```text
AgentTaskManager UI
MCP Agent Tool
Execution 展示
Cancel 按钮
历史状态
```

UI 不参与任何：

```text
Safe Release
Runtime Evidence
Claim Ownership
```

判断。

---

# 51. Runtime Foundation Gate

正式进入上层 Agent 功能开发前，底层 Runtime 必须证明：

```text
1.
CreateProcessW 成功的 Codex
从未在 Job 外运行

2.
Host 被强制终止后
Codex 及普通后代进程全部退出

3.
Job Handle 未被 Child 继承

4.
stdio JSON-RPC 正常双向通信

5.
Runtime 重启可以依据 Job-level evidence
判断旧 Runtime

6.
无法取得可靠 Evidence 时
Execution → unknown
Workspace Claim 保留

7.
terminal Execution
与
Workspace Claim 删除
不存在事务崩溃窗口
```

只有以上 Gate 通过：

```text
Runtime Foundation = READY
```

才继续构建上层 Agent 产品能力。

---

# 52. 实施原则

本阶段优先级固定为：

```text
Correctness
>
Recoverability
>
Observability
>
Convenience
```

不得为了：

```text
更容易恢复
更快释放 Workspace
更简单实现
更少状态
```

削弱：

```text
Job-level termination evidence

Runtime ownership

Execution state monotonicity

Atomic Claim Release

unknown fail-closed
```

五项核心保证。

Technical Design 状态：**READY FOR IMPLEMENTATION**，表示可以依第 50 节顺序启动底层开发；不表示 Runtime Foundation Gate 已通过，也不代表固定 Codex 版本已兼容验证。

当前 Runtime Foundation Gate：**NOT_RUN**。固定 Codex 版本和真实协议 Contract Test 是 Phase 3 的强制验证前置项，尚未完成；不阻塞 Phase 1 State Store 与 Phase 2 Windows Launcher。当前没有需要先改变正文可靠性契约的未决设计项。若出现 Material Contract Difference，则受影响阶段标记 BLOCKED，等待明确决策后才可继续。

Foundation Gate 除上述七项外必须包含：第 31～34 节故障矩阵、固定版本完整初始化、Server Request 拒绝响应、全分页 empty evidence、Runtime 归属校验、R1→R2/NULL trigger 拒绝及历史 Evidence 缺失测试。每项记录命令、版本、结果和证据；未运行标 NOT_RUN，环境不可用标 UNAVAILABLE，不得记为 PASS。Gate 全部通过前不开发上层 Agent UI；本轮文档修订不授权生产代码、Cargo.toml 修改或 Git commit。
