# DCR — CodeBuddy private recovery state（bounded proposal）

状态：schema input / ownership proposal，待后续生产任务批准实现；本卡不实施 migration，不建立表，不修改公共 Port。Stage C Host NOT_RUN，以下不得作为真实 crash 已通过的证据。

## 当前事实与是否需要下一版本

只读当前 `src-tauri/src/agent/store.rs:327` 起的 migrate：接受版本0..=12，version=12直接检查FK，低版本追加SCHEMA_V12并设置user_version=12（约421行）。`schema_v12.sql` 中 executions 已有 provider、runtime_instance_id、workspace_id、canonical_workspace_root、workspace_generation、revision、final_result_json/result_completeness及独立release/termination evidence。`schema_v9.sql` 已有公共 execution_usage、Codex-private codex_execution_usage_state/epochs；当前 schema 没有本DCR的CodeBuddy recovery identity表。

因此生产采用本proposal需要 **next schema version，当前基线下候选v13**；正式实施时重新检查版本，不能覆盖已被其他任务占用的版本号。不改v12、不在本卡新增SQL/migrate分支，不查询或写用户实际DB。

## 最小父记录 proposal：codebuddy_execution_state

一条Execution对应一次模型Prompt；恢复检查不是新Execution模型调用。所有枚举仅Provider-private，不添加公共Product identity字段。

| 字段 | 必要性 / 可空与语义 |
|---|---|
| execution_id | 必需，PK且FK executions(id)，ON DELETE RESTRICT；必须验证execution.provider=codebuddy |
| runtime_instance_id | dispatch后必需；允许dispatch前null，FK runtime_instances(id) RESTRICT。原R1 ownership snapshot，必须与generic immutable Execution/runtime binding一致；R2 recovery不能覆盖它 |
| acp_protocol_version | initialize成功后必需，未协商为null/unknown；不默认填1 |
| session_id | new/recovery成功且验证身份后必需；先前缺失null。同Session可跨多个Execution，不设单列unique |
| conversation_request_id | 发Prompt前durable，Prepared时必需；用于精确目标归属；不得从文本或时间窗口补造 |
| provider_request_id | 若Provider requestId与conversation_request_id能在已验契约中强制同值，可不重复持久化；否则需独立nullable列并保存绑定来源。当前bundle的rootRequestId路径不支持无条件把两键等同 |
| prompt_rpc_id | 可空，实际发送/分配后记录；仅在原runtime connection内相关，不是跨Runtime history身份；必须保留JSON-RPC string/number类型，不假设新连接复用可识别旧Prompt |
| prompt_state | 必需，prepared / sent / uncertain / terminal_observed；准备不代表已发送，发送后host crash不能补成terminal。状态转换由OCC保护；业务Execution终态仍由generic authority决定 |
| terminal_stop_reason | nullable，只有exact live PromptResponse或以后独立批准的equivalent terminal证据才写；history assistant正文不得填写end_turn |
| terminal_observed_at | nullable，与terminal身份/来源一致写入；时间本身不构成terminal证据 |
| recovery_method / recovery_state | nullable method，仅本proposal的session/load用于result inspection；state=not_attempted / inspecting / partial / unknown / material_difference；不把Continue session/resume混成fallback，不引入recovered_completed |
| recovery_runtime_instance_id | nullable，FK runtime_instances(id) RESTRICT；R2来源快照，与R1 ownership分离，不是R1 termination证明 |
| recovery_started_at / recovery_finished_at | nullable，实际时间；仅诊断/重入控制，不因超时推断Provider terminal或R1消失 |
| revision / created_at / updated_at | 必需；revision非负单调，Provider-private OCC；时间不能替代revision或evidence |

结果hash/length/completeness：generic final_result_json/result_completeness已有最终结果authority，不再复制完整正文或新的完整性authority。若生产需要持久化recovered text诊断，按既有结果保留策略保存partial provenance；可选private `recovered_answer_sha256`/`recovered_answer_bytes` 用于检查，而非完成证据。旧live expected hash若未durable，必须为unknown；本harness内存保留oracle不代表真实Host crash后仍可比较。不得仅凭hash相同填complete。

不增加canonical_workspace_root/cwd作为独立authority。Execution已有冻结workspace identity/generation，外部cwd由其投影并复核，恢复也使用该同一authority。Harness fresh cwd仅为task证据，不是生产schema复制方案。

## 唯一性、ownership、OCC与写入顺序

- `execution_id`主键约束一条private父状态。建议 `(session_id, conversation_request_id)` 在两者非null时unique，防止同一target Prompt被不同Execution认领；若允许不同provider account namespace共享session ID，正式实施必须将已验证namespace绑定纳入约束，不能靠猜测session全局唯一。
- `prompt_rpc_id`不设全局unique。其相关范围为原runtime/connection；同Session的多个Execution拥有不同conversation/request target。
- FK存在不等于ownership验证：每次写检查generic execution.provider、immutable R1 binding、Runtime.provider一致；R2只进入独立recovery provenance，不改generic绑定。
- 在发送Prompt字节前，事务内durable prepared identity。发出但未确认的crash窗口保持uncertain；禁止自动重发原Prompt。实际RPC ID若只能SDK发送时观察，则字段可空，不补造发送前证据。
- 更新带expected private revision与generic execution revision/provider/runtime predicate，受影响行数非1则停止并重读，不覆盖新状态。terminal/partial结果与generic lifecycle的联动须在既有IMMEDIATE事务authority内完成；本DCR不新增另一套release事务。
- R1 Job evidence缺失：unknown + Claim retained。另行取得R1可靠termination evidence后，即使result partial/unknown，也可按§23 generic runtime-termination recovery安全收敛interrupted。private状态、R2成功、PID/reap、hash均不产生Claim release permission。

## Usage durable ledger另表 proposal

Stage B仅冻结supported but completeness-aware。若生产要求crash-safe messageId去重，需单独CodeBuddy-private child rows，例如 `codebuddy_execution_usage_calls`，FK execution_id→private父记录 ON DELETE RESTRICT，复合唯一 `(execution_id, provider_request_id, provider_message_id)`；同ID帧以OCC替换最新有效值，不能累加重复帧。不同ID才形成model-call集合。缺失ID的观测不得编造key进入complete ledger。

最小child输入：explicit input/output/total/cache-read/cache-write nullable非负安全整数、source runtime、source kind/path、first/last observed timestamps、row revision、有效性/覆盖状态；不保存任意raw _meta、cost文本、credential、prompt/thought。未冻结reasoning保持null。非法/回退数字不覆盖有效值，父完整性降partial/unknown。唯一集合coverage、prompt_end边界及发送失败须有独立provenance，单靠ledger存在不填complete。

公共execution_usage作为聚合投影，ledger替换与聚合revision必须同事务避免crash double-count。不塞入 `codex_execution_usage_state`，不使用Session cumulative baseline subtraction。独立评审此ledger的覆盖与retention后才能实施；本卡不凭空添加完整usage evidence。

## Migration、retention与Port边界

- v12→next只新增private结构；历史Execution不回填session/request/message/terminal。缺失保持null/unknown，未曾采集的R1 Job evidence不能由migration创造。
- private父/child和diagnostic摘要遵循Execution retention；FK RESTRICT禁止先删除Execution绕过审计。已批准的清理事务按child→private parent→Execution顺序；有Claim或未解决recovery/ledger归属时保留，不自行设置TTL或清理Job evidence。
- public Product/Provider Port只接收既有规范化result/completeness/usage，不暴露ACP session、request、message、RPC或private revision。Provider-private State仅Provider内部访问，不把Codex字段当通用容器。
- 待生产批准条件：实际Stage C Host证据、identity namespace/ownership约束确认、SDK RPC-ID可持久时点、OCC/事务边界、retention及缺失历史回归测试。DCR不是migration授权或Production PASS。
