# Stage C Crash Host Freeze

Stage C Contract Freeze Host Gate **PASS**。Stage A/B/C Host Gate 均 PASS，CB5-005 可以完成。本文件只读冻结父 Host 已落盘 evidence；没有重新运行 binary、harness、测试或任何真实 CodeBuddy CLI。

## 1. Host execution 与 raw evidence

父 Host 报告以下真实 CommandRun；外层执行元数据不在 result JSON 内，因此按 Host 报告记录：

- before：`command-26872-1790483578322192-95`，completed / exit0。
- after：`command-26872-1790483608562452-96`，completed / exit0。

两组 `attempt-started`、`prompt-identity`、`result` 共六份 raw evidence 原字节保留；SHA256 与机器可读断言见 [crash-host-freeze-checks.json](crash-host-freeze-checks.json)。sentinel 已消费，禁止删除、覆盖或重跑。

## 2. Observed contract

`crash-before-terminal.result.json`：

- `status=PASS`，`r2Status=INSPECTED`，顶层 `resultCompleteness=partial`。
- R1 `beforeWindowObserved=true`，在 exact prompt terminal 前命中目标窗口；R1 projection 为 `unknown`，R2 load 后 projection 为 `partial`。
- `manifestComplete=true`、`workspaceDelta=[]`、`workspaceDeleted=true`。

`crash-after-terminal.result.json`：

- `status=PASS`，`r2Status=INSPECTED`，顶层 `resultCompleteness=partial`。
- R1 `terminalExactConversation=true`、`terminalStopReason=end_turn`；R2 `messageIdSetMatchesLive=true`、`answerMatchesLive=true`，R1/R2 均 `materialContractDifference=false`。
- `manifestComplete=true`、`workspaceDelta=[]`、`workspaceDeleted=true`。

两项都满足本任务定义的协议身份、结果检查、owned child cleanup 与 Workspace 无差异 gate，因此 Stage C Contract Freeze Host Gate PASS。

## 3. Fail-closed 边界

- 两项 `resultCompleteness` 都只是 `partial`；`businessCompletedRecoverable=false`、`productionExecutionResultPersisted=false`。不得升级为完整业务结果恢复或 completed Execution 证明。
- 两项 `windowsJobAtCreationProven=false`、`runtimeTerminationEvidenceProven=false`、`claimReleasePermitted=false`。R2 load/result recovery 是 Result Recovery inspection，不能证明 R1 Runtime termination，也不能授权 Claim release。
- owned direct child 已 cleanup/reap 不等于 Windows Job/tree containment；缺失 R1 termination evidence 时生产判定继续 fail closed。
- 本卡只冻结协议证据与 provider-private schema DCR。生产 Runtime、Windows Job、StateStore、migration 均未实现；没有生产 Crash Gate PASS。
- 不允许重跑 resume/load/usage/usage-repair/crash-before/crash-after，不允许以删除 evidence、移动 binary、repair/retry/force 或其他真实 CodeBuddy CLI 绕过一次性证据约束。

## 4. Task conclusion

Stage A Continue、Stage B Usage、Stage C Crash 的 Host Gate 均 PASS。CB5-005 的 Contract Freeze 交付目标已完成；完成状态不扩大为生产实现、Claim 生命周期或 Runtime termination 验收。
