# Verification

> **CURRENT STATUS: COMPLETED**。`CB5-005 PASS with Continue supported / Usage unsupported for initial release / Crash contract frozen`。Attempt 4/5 Host evidence、最终合同与 schema DCR 已通过独立只读 FULL_SCOPE review。

## 真实 Probe

- Attempt 1：`NON_EQUIVALENT_INITIALIZE_PROBE`。原始 evidence 保留，不再作为 CB5-005 Contract Gate authority。
- Attempt 2：`NON_EQUIVALENT_INITIALIZE_PROBE`。独立路径和全部 raw evidence 保留，不再作为 CB5-005 Contract Gate authority。
- Attempt 3：`EXACT_INITIALIZE_NON_HOST_ENVIRONMENT_PROBE`。两份冻结 CB5-004 wire hash 匹配，initialize shape exact；但由 Codex Agent shell 的 `process.env` 启动，不是已证明的 SerenaDesktop command Host 用户环境，因此仅为 diagnostic history、不是最终 Contract authority。
- Attempt 3 按门禁停止：没有 prompt、usage、S1、R2、`session/load` 或 crash window 调用；没有 `session/resume`、fallback 或第二次真实重试。
- Attempt 3 raw wire SHA256 `74a6439ff305ea06e127086feae518cb5af1cab8c685bd9acc28b42abe493470`；详细文件 hash 在 `evidence/attempt-3/attempt-classification.json`。
- Attempt 4：Host `command_execute` 环境完整执行。Fresh session、R1/P1/P2、R1 reap、独立 R2、`session/load`、13 条 exact-S1 replay、P3、Usage 和四个 crash 窗口均有真实 evidence；Workspace 无 delta。
- Attempt 4 Continue 的 `INCONCLUSIVE` 仅由旧逐字符 `p3Matched` 判定造成；`resultRecovery=session+partial-result / partial` 仍是独立结论。
- Attempt 5：Host continuation-only Probe exit 0，`continuation.conclusion=PROVEN_SUPPORTED`；没有重跑 Usage/Crash。

## 静态 / unit

- PASS — Node `v24.19.0`。
- PASS — `node --check harness/lib.mjs`。
- PASS — `node --check harness/probe.mjs`。
- PASS — `node --test harness/probe.test.mjs`: current 24/24。
- PASS — initialize fixture 同时校验两份 CB5-004 wire 固定 SHA256，并直接 deep-compare harness 生成 params。
- PASS — watchdog completion self-test 证明 operation 完成后 timer 被释放，不启动 Provider。
- 初次 tests 6/7：唯一 method test 误把只读 bundle inspection 字符串当调用；收窄到 request call site 后 7/7。Usage correction test 后 final 8/8。

## Cleanup

- PowerShell 精确空目录删除被 Host policy 拒绝，未发生删除。
- PASS — task-local owned-temp cleanup helper：attempt 1 两个、attempt 2 五个空 Workspace 均验证 `%TEMP%` + 固定前缀 + empty manifest 后删除。
- Attempt 2 `taskkill /T` FAIL，direct child reap 未证明；harness session interrupt 后 exact PID 均 absent。此项只为诊断，不是 Job/Claim authority。
- Attempt 3 Provider PID 46000：`taskkill /T` 返回失败后 direct `SIGKILL` reap 成功，`process-evidence.json` 记录 `directChildReaped=true`；run 后 PID absent、final manifest `{}`、Workspace deleted、task temp residue 0。仍不构成 Windows Job/Claim authority。
- Attempt 3 result/summary/process evidence 在外层 harness interrupt 前已持久化。interrupt 原因是已完成路径遗留 20 分钟 watchdog timer；该 task-local defect随后修复，13/13 包含独立子进程退出回归测试。真实 Probe 未重跑。
- Attempt 3 failure result 保存了 final manifest `{}` 与 deletion proof，但当时未把已计算的 initial `{}` manifest写入失败结果；修复后的 harness 已对 continuation/crash 成功与失败路径统一持久化 before manifest。由于只允许一次 attempt-3，未重跑，不能把此修复冒充为该次 raw evidence。

## 最终静态门（review 前）

- PASS — authority 三文件 SHA256 与用户给定值一致。
- PASS — `git diff --check` exit 0；本任务文件均为 untracked，因此该命令只证明 tracked diff 无 whitespace error。
- PASS — `git diff --name-only b83418a... -- src src-tauri docs` 返回空，production/docs 权威正文零 diff。
- PASS — current syntax checks exit 0；current unit tests 24/24。
- PASS — `%TEMP%` 中 `cb5-005-20260928-*` residue count = 0。
- 当前工作区仅新增本任务目录；无 commit/push。

## Review repair round 1

- Round 0 independent `CHILD_AGENT FULL_SCOPE` 覆盖 46/46，冻结 hash 匹配；P0/P1=0，P2=3、P3=1，gate `BLOCKED`，见 `review-round0.md`。
- 已最小修复 Usage exact identity/scope/reset、crash exact-result 可达性、pipe/end-to-end timeout/handle cleanup和零事件文档规则。
- PASS — repair 后 Node syntax checks exit 0；unit tests 10/10。
- NOT_RUN — 真实 CodeBuddy Probe：修复不改变本轮已冻结的 Provider/backend 失败事实，且用户要求真实调用尽量少；不做第三次尝试。
- Round 1 独立重审覆盖 47/47，P0/P1/P3=0、P2=3，gate `BLOCKED`，见 `review-round1.md`。

## Review repair round 2

- 已移除基于数值走势的 Usage scope/reset 推断；没有 typed 语义一律保持 `unknown`。
- crash exact-result 现在要求 original RPC id 的 typed `result.stopReason`，明确拒绝 RPC error/空 result。
- watchdog 超时路径现在先保存 Runtime reap 诊断与 final Workspace manifest、执行 task-owned cleanup 并写独立 cleanup evidence，再 exit 124；仍不宣称 Job authority。
- PASS — repair 后 Node syntax checks exit 0；unit tests 10/10。
- NOT_RUN — 真实 CodeBuddy Probe：仍不做第三次 Provider/backend 尝试。
- Round 2 独立重审覆盖 48/48，P0/P1/P3=0、P2=1，gate `BLOCKED`，见 `review-round2.md`。

## Review repair round 3

- Usage 的 terminal coverage / late behavior 改为逐字段、exact-bound sample 判定；unbound 明确为 `unknown_unbound`，不再由其他字段的事件连带推断。
- PASS — repair 后 Node syntax checks exit 0；unit tests 11/11。
- NOT_RUN — 真实 CodeBuddy Probe：仍不做第三次 Provider/backend 尝试。
- 尚待：更新冻结身份并执行最终独立只读 `FULL_SCOPE` 重审。

## Historical Host repair round — exact CB5-004 initialize

- Host Gate finding：旧 harness 对 CB5-004 initialize shape 的声明不实；旧 review `PASSED` 随此前提失效而作废。
- 修复：initialize params 逐字段精确复用 CB5-004 Host PASS；新增 frozen wire hash + structural equality fixture；attempt-3 fresh prerequisite 失败即停止。
- PASS — attempt-3 exact initialize 真实 wire；FAIL — fresh `session/new -32603 / HTTP 500`；后续真实 Contract Probe `NOT_RUN_BY_GATE`。
- 当时的 attempt-3-only 结论：Continue/Usage `INCONCLUSIVE`，Crash `NOT_OBSERVED`；现已由 Host environment finding 取代，最终 Contract authority 等待 attempt 4。
- 当时尚待的新 freeze/review 已失效；不得在 attempt 4 前重新 freeze 或宣称最终 review 通过。

## Attempt 3 review repair round 1

- 独立 `CHILD_AGENT FULL_SCOPE` 覆盖 59/59、冻结 identity 匹配；P0/P2/P3=0，P1=1，gate `BLOCKED`。
- P1：capability 文档仍引用已降级的 attempt1/2 支撑 recovery method。
- 已修复：唯一 method 现在只由 pinned typed schema + exact attempt3 `loadSession=true` 支撑；旧 Probe 仅作 diagnostic history。
- NOT_RUN — 单测与真实 Provider：本修复只改 evidence provenance 文档，不改变 harness 或 raw evidence。
- 尚待：重新 freeze 与独立只读 FULL_SCOPE rereview。

## Historical Host environment repair — attempt 4 preparation

- Host verified：原 CB5-004 gold-band Probe 在当前 SerenaDesktop command Host 环境 fresh session PASS，Workspace delta 0。
- Host verified：numeric/string JSON-RPC id 最小 A/B 均 fresh session PASS；id 类型不是 attempt-3 差异。
- Attempt 3 重新分类为 `EXACT_INITIALIZE_NON_HOST_ENVIRONMENT_PROBE`；其 Agent-shell `process.env` 不能称为已证明的 Host standard-user environment。
- 新增 Host-only `probe-host-attempt-4`：要求 `SERENA_CB5_HOST_PROBE=1`，固定独立 `evidence/attempt-4`，存在即拒绝覆盖。
- 新增 allowlist-only `environment-provenance.json`：runner/platform/Node/LaunchSpec hashes，以及固定 CODEBUDDY/proxy 键的 present/absent；不保存值、PATH、完整 env 或 token。
- PASS — Node syntax checks。
- PASS — `node --test harness/probe.test.mjs`: 16 passed, 0 failed。
- PASS — 无门禁 Host mode 子进程 exit nonzero 且 evidence tree 不变。
- PASS — attempt4 path 独立且位于 task-local evidence，未指向 `src/`、`src-tauri/`、`docs/`。
- PASS — initialize 与两份冻结 CB5-004 Host wire structural exact equality。
- PASS — completed watchdog operation 释放 timer。
- HISTORICAL — 本轮结束时 `probe-host-attempt-4` 尚未执行；随后 Host 已完整执行并生成 `evidence/attempt-4/`。
- HISTORICAL — 当时未 final freeze / FULL_SCOPE review；后续 attempt-4 finding 又要求本次 attempt-5 semantic repair。

## Host attempt 4 evidence and attempt 5 semantic repair

- HOST PASS — attempt 4 fresh session、R1/P1/P2 terminal、R1-before-R2 reap、独立 R2 initialize、唯一 `session/load`、13 条 exact-S1 replay、P3 terminal、Usage 与四个 crash 窗口均完成。
- PASS — attempt 4 replay 无 wrong-session update，Workspace before/after manifest 一致；exact PromptResponse 未恢复，因此 result recovery 仍为 `partial`。
- Finding — attempt 4 的 Continue 只因 `p3.answer.trim() === marker|workspace` 逐字符判定而保持 `INCONCLUSIVE`；额外 markdown、反引号或说明文字会误判。
- Repair — 新增 gated `probe-host-attempt-5-continuation` 与独立 `evidence/attempt-5/`；只执行 R1/P1 → reap → R2/load/replay → P3，不运行 Usage/Crash。
- Repair — P3 prompt 不含 marker/CWD；answer 仅落 SHA256、UTF-8 bytes、`markerPresent`、`cwdPresent`，不保存正文。
- Repair — CWD containment 大小写无关，`/` 与 `\` 等价；Continue PASS 还要求 exact-S1 replay、无 wrong-session、Host 未重放 P1、P1/P3 `end_turn` 与 Workspace 零 delta。
- PASS — Node syntax checks。
- PASS — `node --test harness/probe.test.mjs`: 24 passed, 0 failed。
- PASS — tests 覆盖 markdown/backticks、wrong marker、wrong cwd、case/slash normalization、P3 fixture isolation、Host gate、attempt-5 独立路径、Continuation-only routing 与 watchdog release。
- HISTORICAL — 本段原始静态 repair 时尚未运行 attempt 5；随后 Host command_execute 已完成并生成独立 `evidence/attempt-5/`。

## Final Host contract freeze candidate

- HOST PASS — attempt 5 status `COMPLETED`、scope `CONTINUATION_ONLY`、conclusion `PROVEN_SUPPORTED`。
- PASS — R1 在 R2 前 direct-child reap；R2 `loadSession=true`；唯一 `session/load(S1,cwd,mcpServers=[])` 成功。
- PASS — replay 11 条且非空，全部 exact S1，无 wrong-session update；load response 未包含 sessionId，因此没有可比较的 mismatched response identity。
- PASS — P1/P3 terminal 均为 `end_turn`；P3 prompt 不含 marker/cwd；Host 未重放 P1；`markerPresent=true`、`cwdPresent=true`；answer 正文未保存。
- PASS — `sessionLineage=EXACT_S1_HISTORY_REPLAY_AND_MARKER`；`cwdLineage=EXACT_WORKSPACE_PATH_OBSERVED_IN_P3`；Workspace manifest 无 delta。
- PASS — Result recovery 独立保持 `session+partial-result / partial`，`exactTargetTerminalRecovered=false`。
- OBSERVED — attempt 4 有 9 条真实 `usage_update`，但 `exactPromptBound=false`，scope/reset/terminal/late 均不能冻结。SerenaDesktop public Usage 固定 `EXPLICITLY_UNSUPPORTED_FOR_INITIAL_RELEASE`，`tokenUsage=false`、unknown/null；不否认 Provider event 存在。
- OBSERVED — attempt 4 四个 Crash 窗口均完成 Session recovery；side-effect marker 保留；result 为 unknown/partial；四窗口均无 exact PromptResponse recovery。
- PASS — R2/PID/direct reap 与 original Runtime Windows Job termination、Interrupted/Claim release authority 明确分离。
- PASS — 当前 StateStore schema v13 inspection：`codebuddy_execution_state` 与通用 `executions` 已覆盖 sessionId、canonical Workspace/cwd、parent/child lineage、provider/conversation request identity 和 recovery lifecycle；DCR=`EXISTING_FIELDS_SUFFICIENT`，无需 migration。
- GATE — CB8-003 `UNBLOCKED_TO_IMPLEMENT`；CB9-001 `SKIPPED_UNSUPPORTED` for initial release；CB8-004 可在 CB8-003 后继续并复用本 Crash 边界。

## Final review repair round 0

- 独立只读 FULL_SCOPE review 完整覆盖 86/86，冻结 identity 匹配；P0=0、P1=1、P2=1、P3=0，gate `BLOCKED`。
- P1 修复：`conversation_request_id` 明确为每个 execution 在 create 事务中本地原子预留的 UUIDv7 Prompt identity；child 不继承 parent。Provider 返回 identity 仅进入 `provider_request_id` / source。
- P2 修复：Crash side-effect marker SHA256 更正为 raw attempt-4 pre-kill、after-recovery、final manifest 一致值 `a050db044fad75cc10989f0cc6d774e793b944abf200b603ed88ee8fba0fd685`。
- 核心结论未改变；修复后重新冻结并执行完整只读复审。

## Final review gate

- PASS — repair 后独立只读 FULL_SCOPE review 覆盖 86/86，冻结 identity 匹配；P0=0、P1=0、P2=0、P3=1，gate `PASSED`。
- CLOSED — 上轮 conversation identity provenance 与 marker SHA256 findings 均由 raw Store/evidence 复核闭合。
- P3 metadata-only — freeze 的 JSONL row count 尚为修复前 821，实际 825；随 completed-state 最终冻结更新，不影响全部 825 行可解析或合同结论。
- COMPLETE — task status=`completed`，completedAt=`2026-09-28`；未 commit/push，未进入 CB8-003。
