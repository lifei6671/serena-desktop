开始实施 CB7-005 — Fresh Execution Finalization / Atomic Claim Release。当前 branch=feat/codebuddy，基线提交 18ddddf（CB7-004 已完成并提交），工作区 clean。新建 task：.trellis/tasks/09-27-cb7-005-codebuddy-fresh-execute-finalization/。最终独立只读 FULL_SCOPE review；不要 commit/push。

权威：implementation-task-breakdown CB7-005；technical-design §15.1、§21/21.1、§23、§31；CB6-002/004/005；CB7-002 PreparedFreshSession、CB7-003 PromptCompletion、CB7-004 Activity。

目标：完成首个生产级 CodeBuddy Fresh Execute vertical slice：AgentProvider.execute -> managed Runtime -> fresh session -> acceptance/dispatch -> prompt/activity -> exact provider terminal/result -> terminate entire Job -> approved runtime termination evidence -> provider-neutral atomic terminal + Workspace Claim release。全部 Gate PASS 后 Windows canExecute=true；CB7-004 activity也可在完整execute链上advertise true。不要实现Cancel/Continue/Usage。

重要现状/必须闭合的两个生命周期缺口：
1. CB7-003 当前顺序是 private MarkSent -> acceptance -> physical prompt，但 generic Execution 仍 dispatch_pending/not_dispatched。完整链必须冻结：`MarkSent durable -> accepted() -> generic Dispatching durable -> physical session/prompt write/flush -> generic Dispatched -> Running`。accepted发生后、physical prompt之前必须持久化Dispatching。若无法精确证明prompt物理flush，必须扩展现有 GuardedWrite/Requests 用 SDK-generated exact request id/method 的窄 flush observation；禁止用timer/response到达推断。
2. generic `ReleaseBasis::RuntimeTerminated` 当前只允许 Reconciling->Interrupted。CB7-005需要 Provider-neutral 扩展：已持久化 exact provider terminal 的 Finalizing Execution，也可在原Runtime termination evidence complete后，用其真实 terminal status（Completed/Failed/Cancelled/Interrupted）原子 finalize+release。严禁 `if provider == codebuddy { release }`。

一、Public Provider execute / capabilities：
- Windows CodeBuddy `AgentProvider.execute` 接入完整fresh lifecycle；非Windows保持capability unsupported/fail-closed。
- execute必须只使用当前 registered provider 的 resolved LaunchSpec；health/admission仍由TaskManager/Registry现有机制控制，不在execute自行fallback。
- 最终 Gate通过后 Windows capabilities：canExecute=true、activity=true、canRecover=true；canCancel=false、canContinue=false、tokenUsage=false。非Windows Fresh/Activity/Recovery按实际Gate保持false（不要声称未实现平台能力）。
- missing CLI/health unavailable时registered descriptor/capabilities事实仍可查询，但availableForNewExecution必须false；found+enabled+health available+canExecute才可新执行。

二、Dispatch/Prompt exact send boundary：
- CB7-002 prepare仍要求 frozen Execution workspace/provider/R1 identity。
- CB7-003 MarkSent前所有preflight/OCC不变。
- MarkSent提交后调用 acceptance.accepted()；随后必须 `Transition::Dispatch{to: Dispatching,runtime_id: exact R1}`，成功后才允许SDK request被物理写入。
- 给 `Requests`/`GuardedWrite` 增加最窄的 prompt-flush observation：SDK仍生成JSON-RPC id；只对exact `session/prompt` outgoing frame注册/通知一个有界 one-shot/observation；GuardedWrite在整frame真实pipe write + inner.flush成功后才发出flushed信号。不能让观察者完成SDK waiter，不能自己造id。
- exact prompt flush后写 `DispatchState::Dispatched`；随后 `Transition::Running`（或用现有state允许的等价顺序），必须在Provider terminal前完成。若 terminal异常早到，状态处理仍fail-closed，不能跳过dispatch evidence。
- MarkSent/accepted后但physical flush失败/EOF/timeout/caller drop：dispatching -> uncertain（若已经进入dispatching）；private Sent -> Uncertain；绝不retry prompt。然后终止Runtime并按runtime evidence收敛，不能假装Provider terminal。
- 刷出prompt后response失败/terminal不可靠同样是unknown side effect：Dispatched/Running -> reconcile/unknown，终止Runtime；无terminal则最终只能Interrupted/Unknown result completeness（取得termination evidence后才释放）。

三、Provider terminal + result durable staging：
- CB7-003 PromptCompletion ProviderRunResult只是内存结果；设计§15.1要求 `persist result / provider terminal` 先于 Runtime termination。
- 在generic state machine增加**provider-neutral** staged terminal-result事务。优先设计一个 typed mutation/Transition，例如 `ProviderTerminalResult {runtime_id,status,result,completeness}`，或等价单事务API：
  * 验证 execution revision、runtime ownership/provider、terminal status；
  * provider terminal evidence与 `final_result_json/result_completeness` 同一事务持久化；
  * 状态进入Finalizing；
  * 不释放Claim，不写release evidence；
  * 重复exact同值可按现有OCC策略幂等/冲突明确；不同terminal/result冲突fail-closed。
- 不要给CodeBuddy私表加result字段，不改schema若现有Execution列足够。
- ProviderRunResult result JSON不得含session/conversation/runtime safety evidence；持久化值必须exact等于CB7-003安全public result。
- crash window测试必须证明：provider terminal/result staged后、Job尚活时，Claim仍存在且release_evidence未complete。

四、Provider-neutral RuntimeTerminated finalization：
- 扩展 generic `finalize_and_release_execution` / state transition，不新增provider special-case。
- ReleaseBasis::RuntimeTerminated允许且仅允许：
  A. 现有 recovery：Reconciling -> Interrupted（保持现状）；
  B. normal terminal：Finalizing + persisted provider terminal evidence bound to same original runtime + finalization.terminal exact等于persisted provider terminal status。
- B必须要求 approved `terminated_runtime(tx, original, row.provider)` evidence；result/completeness必须与前一步staged值一致（若你设计finalization不再传result，也必须由transaction读取staged值，不能让caller替换）。
- Runtime provider ownership继续用generic `require_runtime_provider`；Provider ID只验证ownership，不能授权release。
- atomic transaction仍同时写 terminal status、runtime_termination_evidence*, release_evidence_state=complete/kind=runtime_terminated/json、Claim delete；任何故障整体rollback。
- Codex SameRuntimeCleanup路径逐字语义不变；既有 RuntimeTermination recovery tests不退化。

五、Runtime termination / evidence：
- Provider exact terminal staged后，消费 `PromptCompletion.session.shutdown()` 终止整个 Job，不kill PID。
- shutdown返回值不是release authority；必须重新读取 durable Runtime row，只有CB6-005 approved evidence `job_active_processes_zero` / `managed_job_destroyed` + complete + exact R1/provider/session policy才进入finalization。
- 如果shutdown返回Err但durable evidence已complete，可按Store authority继续；如果evidence不完整/unknown，绝不release。
- evidence不足：Execution收敛 `unknown`（或符合现有state graph的reconciling->unknown），Claim retained，final_result/provider terminal staged可保留用于recovery；不要清空真实terminal。
- terminal前失败但Runtime termination evidence complete：没有可靠Provider terminal，走generic reconcile RuntimeTerminated -> Interrupted，result Unknown/Partial按已有安全语义；不要伪造Completed/Failed/Cancelled。

六、Startup crash window：
- 重点覆盖 `provider terminal/result staged -> host crash/worker drop -> Runtime仍需recovery`。
- CodeBuddy startup_reconcile在发现 Finalizing + exact staged provider terminal/private terminal + original R1 termination evidence后，应走同一 generic RuntimeTerminated normal-terminal finalization，保留 staged terminal/result，而不是无条件降级 Interrupted。
- 若provider terminal/result不完整或private identity冲突，则保持现有 Interrupted/Unknown fail-closed语义；不能根据final_result_json单独信任terminal。
- recovery release仍只依赖approved Runtime evidence，不因provider=codebuddy直接释放。

七、Fresh execute failure convergence：
- pre-accept失败：sink不accepted；如果从未建立可证明runtime attempt，保持现有pending/cancel-before-dispatch语义；一旦有R1/side-effect attempt，cleanup并根据termination evidence安全收敛，禁止自动replay。
- post-accept/pre-flush：Dispatching->Uncertain + private Uncertain；termination evidence complete后Interrupted safe release，否则Unknown+Claim。
- post-flush/no-terminal：Dispatched/Running -> Reconcile；termination evidence complete后Interrupted safe release，否则Unknown+Claim。
- exact terminal：stage terminal/result -> shutdown/evidence -> atomic真实terminal+release。
- Store OCC/persistence injection failure：不能半释放Claim，不能丢失Runtime authority。

八、Real isolated Windows vertical-slice test（必须）：
- 不重跑真实CodeBuddy/CB5 probe。使用native fake ACP child，但必须是真实 `CreateProcessW + Job-at-creation + pipes + SQLite + isolated temp workspace`，不是纯内存mock。
- fake child在收到exact `session/prompt` 后真实在其 cwd 创建 `output.txt`，bytes固定例如 `CB7_005_WRITE\n`，不改其它用户文件；然后发exact-correlated activity + PromptResponse end_turn。
- 通过真实 `CodeBuddyProvider.execute`（最好再覆盖 TaskManager registry/admission入口）跑完整链；断言：
  * accepted receipt在prompt前；dispatching在physical prompt前；dispatched/running在flush后；
  * exact workspace只有预期output delta；
  * provider terminal/result staged时Claim仍存在、Job未证明empty；
  * execute结束后Runtime terminated evidence complete；
  * Execution=completed、provider_terminal_status=completed、result_completeness=complete、final_result_json exact safe payload；
  * release_evidence_state=complete、kind=runtime_terminated、runtime id/evidence_at exact；
  * Workspace Claim已删除；
  * Runtime state terminated；
  * Activity只安全category，不影响结果。
- read-only vertical slice也建议至少一个，以证明零delta。

九、Crash/failure matrix tests至少：
- terminal-before-termination crash window：staged result+terminal + Claim retained -> startup reconcile terminates Job/evidence -> exact terminal/result preserved + atomic release。
- termination evidence persistence failure / Job open/query/policy/timeout failure => unknown + Claim retained。
- finalization transaction fault at result/release/Claim delete boundaries => rollback，Claim retained；retry with same durable evidence converges exactly once。
- staged result mismatch / terminal mismatch / runtime provider mismatch => finalization rejected, Claim retained。
- no terminal + complete Runtime evidence => Interrupted + Unknown/Partial，不Completed。
- main PID disappears但descendant alive：仍等待Job zero（回归CB6-005）。
- provider disabled after acceptance不终止running lifecycle；missing/unavailable只影响new admission，不破坏current owned execute/recovery。
- activity slow/failure不改变finalization。
- Codex same_runtime_cleanup/result/Usage tests保持原样。

十、Capabilities/Product：
- Gate全部PASS后 Windows `can_execute=true`, `activity=true`, `can_recover=true`; cancel/continue/token=false。
- 更新Provider catalog/projection snapshots；found available+enabled => availableForNewExecution true；missing CLI/unavailable或disabled => false。
- AgentProvider.execute绕过admission时仍必须核对Execution provider/R1/workspace/current lifecycle，不能拿错Execution。
- 非Windows不因compile存在接口就advertise execute/activity。

严格禁止：
- provider==codebuddy Claim release shortcut；
- SameRuntimeCleanup伪装CodeBuddy runtime evidence；
- prompt terminal直接release；
- PID作为termination evidence；
- 自动retry session/new/session/prompt；
- Cancel、Continue、Usage实现；
- schema migration（除非发现不可避免MCD，先停并报告）；
- real CodeBuddy probe。

验证：focused generic finalization/state-machine tests；CodeBuddy native vertical slices/failure matrix；full CodeBuddy（如Windows并行fixture有已知时序抖动，必须保留并行失败证据并做serial deterministic回归，不能静默跳过）；TaskManager/Product provider admission/catalog；telemetry；Codex same-runtime/Usage regressions；fmt/check；clippy（usage_tests.rs:987 baseline精确记录）；git diff/scope；freeze + independent read-only FULL_SCOPE review。

Acceptance：Fresh Start vertical slice和atomic release全部Gate通过，才设置canExecute/activity true并task completed。任何termination evidence不确定必须unknown+Claim retained。不要commit/push。
