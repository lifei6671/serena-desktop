# Continuation implementation matrix

| 合同 | 生产实现 | 证据 |
|---|---|---|
| generic admission/child authority 不变 | `task_manager.rs` 未改；CodeBuddy 仅实现 `validate_continuation` | TaskManager 31/31；disabled/health/capability tests |
| source exact private eligibility | `provider.rs` → `CodeBuddyStore::continuation_source` | missing private/session 为 Ineligible；native source 为 Eligible |
| exact child/source lineage | `read_codebuddy_continuation_lineage` 原子比较 provider、parent、agent、task role、Workspace id/root/generation、mode、profile 与 core terminal eligibility | parent/cwd/generation drift 均在 Runtime 前失败，wire 请求数为 0 |
| Fresh/Continue prepare 分离 | `fresh::prepare_owned` 与 `continued::prepare_owned` 独立；只共享 `start_owned` 和 prepare 后 lifecycle | Fresh 全套回归包含在 CodeBuddy 135/135 |
| new child Runtime/private identity | Continue 调用 `start_owned` 创建 child private UUIDv7 与独立 persisted Runtime | success test 断言 R1 != R2、conversation id 不同、parent_execution_id=source |
| 唯一 typed load | `LoadSessionRequest::new(S1, projected_cwd)`；SDK 默认 `mcpServers=[]` | client typed test与 native request log；序列仅 initialize/load/prompt |
| exact replay/history | load 前 `register_route(S1)`；response 可选 sessionId exact；replay 非空、全属 S1、且至少含一个 typed non-empty Text user/agent/thought正文 | wrong session、mismatch、missing、catalog-only、`{}`、空文本、纯空白、malformed matrix 全部 fail closed |
| durable identity/recovery | load 前 `ExactSession(S1)`；验证后且 durable `Sent` 之后、acceptance 之前写既有 v13 `inspecting → partial` | store transition test；acceptance sink 观察 recovery=partial |
| child identity不继承 | child UUIDv7 新建；provider request 仅 exact child wire observation；prompt RPC 由 child connection 自己观察 | native R1 持有非空 provider request而R2为空；typed store source 持有非空 provider/RPC identity而child均为空；conversation不同 |
| acceptance ordering | lineage/load/history完成 → prompt preflight → MarkSent → continuation recovery partial → accepted → Dispatching → physical prompt | acceptance sink 断言 Sent/partial、Runtime running、Claim=1、prompt文件尚不存在 |
| terminal/cancel/permission/activity | prepare 后继续进入既有 `prompt::run`、execute finalization/recovery、Job convergence | continued Cancel/Permission/Activity native regression；CodeBuddy 135/135 |
| Runtime/Claim authority | load只写 private recovery；Claim仅由原有 Runtime Job termination evidence释放 | acceptance时 Claim仍存在；结束后 child Runtime terminated/evidence complete |
| public capabilities | Windows `can_continue=true`；非 Windows `cfg!(windows)=false`；`token_usage=false` | provider/catalog snapshots；实际 CodeBuddy Product gate覆盖 exact/missing/no-session/disabled/unavailable；Product 135/135 |
| schema | 只复用 v13 字段，无 migration/schema修改 | `git diff --name-only` 对 schema/migration为空 |

Result Recovery 仍冻结为 `partial`；本卡只证明 Session continuation，不声称 recovered exact terminal/result。
