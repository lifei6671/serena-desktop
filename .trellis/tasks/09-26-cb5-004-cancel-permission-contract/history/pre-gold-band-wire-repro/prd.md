# CB5-004 Cancel / Permission Contract

## Goal

在独立临时 Workspace 中冻结真实 CodeBuddy ACP session/cancel 与 session/request_permission 的 wire、时序、副作用和 terminal convergence。只做 task-local contract probe，不修改生产 Provider。

## Boundaries

所有真实场景仅在 harness 创建的临时目录中运行。不得访问真实项目。权限场景只测试拒绝路径，不自动批准或持久化任何允许规则。Cancel intent、permission deny、Provider terminal、Runtime termination 必须分别记录，不能互相替代。

## Real scenarios

Cancel-before：启动可稳定保持 active 的 Prompt，在确认已进入运行态且 manifest 尚无文件变化时发送 session/cancel；记录 exact sessionId、cancel wire、updates、terminal/timeout 和最终 manifest。

Cancel-after：Prompt 先在 temp root 创建 deterministic marker，再进入长等待；harness 以 marker 实际出现和 hash 正确作为 cancel trigger。记录 cancel 后 terminal 是否到达以及是否仍产生其它副作用。Cancel 不等于回滚。

Permission-deny：以普通权限模式和 task-local process-scoped settings 尽量稳定触发一个只作用于 temp root 的权限请求。捕获真实 RequestPermissionRequest 和实际 option 集合；使用官方 SDK typed kind/id 选择拒绝项，不解析显示文案。拒绝后记录 updates、terminal 和 manifest。拒绝本身不是 terminal evidence。

## Failure semantics

session/cancel 后若 bounded timeout 内没有 exact prompt terminal，则清理 harness-owned Runtime/Child并记录 termination evidence，不伪造 Provider cancelled。任何可能有副作用的未知结果都不自动重放。Permission deny 后没有 terminal时同样只记录真实 termination/reconciliation事实。

## Deterministic tests

覆盖 identity mismatch、malformed permission response、duplicate/late permission response、cancel terminal、cancel no-terminal timeout、late updates、cleanup failure bounded。

## Evidence

cancellation.jsonl、cancel-before-result.json、cancel-after-result.json、permission.jsonl、permission-deny-result.json、permission-options.json、process-evidence.json、verification.md、review hashes。

## Acceptance

真实 canCancel wire 与 terminal/timeout语义有一手证据；after-side-effect证明 cancel不代表回滚；真实 permission option/deny与deny后收敛有一手证据。产品 src/src-tauri 0 变化，whole-tree diff-check PASS。