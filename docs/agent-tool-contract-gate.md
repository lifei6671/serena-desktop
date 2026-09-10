# Agent External Tool Contract Gate

源码、当前运行的 Local Broker、Cloudflare 已同步目录、ChatGPT 当前可见目录是四个不同的事实。不能用本机测试替代外部验收。

## 1. Local Registry — 真实 MCP

从仓库根目录执行现有官方 Serena 集成测试，使用隔离目录和临时 loopback 端口启动实际 Broker。测试不替换当前桌面应用或占用其配置端口。

```powershell
$env:SERENA_TEST_EXE = "$env:USERPROFILE/.local/bin/serena.exe"
New-Item -ItemType Directory -Force docs/tasks/evidence/TASK-009/external-tool-contract | Out-Null
$env:SERENA_AGENT_CONTRACT_EVIDENCE = (Resolve-Path docs/tasks/evidence/TASK-009/external-tool-contract).Path
cargo test --manifest-path src-tauri/Cargo.toml official_serena_agent_schema_round_trip -- --ignored --exact mcp::integration_tests::official_serena_agent_schema_round_trip
```

要求官方 Serena 1.7.0。测试执行两个新 MCP session 的 initialize/tools/list：Agent disabled 时无 agent，enabled 时只增加一个 agent。保留原始 ListToolsResult JSON、descriptor hash、Broker 发布日志及一次 observe(waitMs=0) 的 MCP 返回值。该观察使用测试库中的 durable Execution，不启动 Codex。

检查：

- action 仍只有 start / continue / resume_pending / observe / cancel / list。
- inputSchema 是 flat object，required 只有 action，additionalProperties=false；outputSchema 为手写 Stable Core Envelope，根 object + success/failure oneOf，以 ok const 区分。
- 属性精确包含 action、agentId、executionId、requestKey、prompt、workspaceId、limit、knownRevision、waitMs、includeResult。
- waitMs 缺省 20000、最大 25000；includeResult 缺省 false。
- description 包含本地执行器分工、bounded observe、控制回执和原请求重试指导。
- description 必须同时包含六种 action 的精确参数签名：start(agentId, requestKey, prompt, workspaceId)、continue(executionId, requestKey, prompt)、resume_pending(executionId)、observe(executionId, knownRevision?, waitMs?, includeResult?)、cancel(executionId)、list(agentId?, workspaceId?, limit?)；只传当前 action 对应的字段，不携带其他 action 的参数。flat schema 和 required=[action] 不变。
- annotations：readOnlyHint=false、destructiveHint=true、idempotentHint=false、openWorldHint=true。
- observe 的 control 包含 requestAccepted、providerInvoked、dispatchCertainty、nextAction；text JSON 与 structuredContent 保持一致。
- terminal summary 有结果时提示 review_result；includeResult=true 成功返回结果后，data.nextAction/control.nextAction 均为 null，重复读取保持如此。dispatch_pending + not_dispatched 的 progress.phase 为 pending；AGENT_DISABLED 也返回 false/false/not_dispatched/null 的 control。

openWorldHint 是可能与外部实体交互的提示，不是网络功能验收。当前 Product Contract 允许联网、使用外部 Codex Provider；既有 workspace-write/network 真实验证 MCD 必须单独解决，不能凭注解标 PASS。

`resume_pending` 仅用于已经 durable 创建、可靠证明尚未跨越 Provider side-effect boundary、未建立 Runtime attempt 的 pending Execution。Host crash、Provider/backend 不可用、binary discovery/resolution failure（均在 Runtime 创建前）都可能产生此状态，并允许 explicit resume 或 cancel-before-dispatch。

必须同时满足 `dispatch_pending + not_dispatched + runtime_instance_id=NULL + provider_terminal_status=NULL`、原 Execution 拥有 Workspace Claim、无 persisted Runtime attempt。拒绝 dispatching/dispatched/uncertain、已绑定 Runtime、已有 Runtime attempt、已有 Provider terminal、running/finalizing/reconciling/unknown、completed/failed/cancelled/interrupted，以及 Claim missing/mismatch。

只接受 exact `executionId`；不创建 Execution、不生成 requestKey、不 replay uncertain Provider request、不重新绑定旧 Runtime、不夺取其他 Claim。继续复用原首次 Provider pipeline；并发 duplicate resume 不得产生第二个 Runtime/Thread/Turn。

## 2. Contract Fingerprint

Agent Tool Contract SHA-256 覆盖 name、description、inputSchema、annotations、outputSchema。递归按 JSON object key 排序，保留 array 顺序，使用紧凑 JSON UTF-8 的 SHA-256 小写十六进制值。enabled、端口、进程、Workspace、时间及执行状态不进入 hash。

Broker 成功启动及 tools/list 发布时记录：

```text
agent contract startup agentEnabled=... agent tool contract sha256=... properties=... annotations=...
agent contract published agentEnabled=true agent tool contract sha256=... properties=... annotations=...
```

Agent disabled 的 tools/list 记录 agent absent。新诊断只包含 descriptor 的身份和字段名，不包含 prompt 值、arguments、Token 或 credential。

在实际桌面环境运行新构建后，先确认**该进程**日志中的 hash 与验收 descriptor 一致。测试进程的 hash 不能证明原先运行的桌面 Broker 已被更新。

参数签名属于 description，因此本次修复会改变 fingerprint。旧版外部调用已通过不代表新 descriptor 已同步，应对当前运行版本重新获取 hash，不沿用旧 evidence 的值。

## 3. Cloudflare Sync

本地 Gate 通过后，由用户在实际 Cloudflare MCP Portal 同步工具目录。核对接入的是预期 Broker，并保存同步结果及可获得的 descriptor。若能导出 descriptor，按上述规则计算 hash 与 Local Broker 对比。

不能访问或操作 Portal 时，记录 `PENDING_EXTERNAL_ACCEPTANCE`。不修改 Tunnel、端口或传输配置来掩盖目录差异。

## 4. ChatGPT Visible Contract

刷新 ChatGPT 的工具目录，人工确认实际可见 Schema 包含 knownRevision、waitMs、includeResult，并用已知 executionId 调用：

```json
{"action":"observe","executionId":"existing-execution-id","waitMs":0}
```

检查真实返回的 control。也可用不完整 start 验证无副作用的 invalid-input 回执：

```json
{"action":"start"}
```

应返回 AGENT_INVALID_ARGUMENT，control 为 false / false / not_dispatched / correct_input。不要为了验收而提交未授权的真实工程任务。

只有用户实际确认可见 Schema 和调用成功后，才能记录 ChatGPT External Tool Contract = PASS；此前为 `PENDING_EXTERNAL_ACCEPTANCE`。Cloudflare 同步成功也不能自动代表 ChatGPT 缓存刷新成功。

## Evidence 与完成边界

每层分别记录 PASS / FAIL / NOT_RUN / PENDING_EXTERNAL_ACCEPTANCE / UNAVAILABLE。保留 descriptor、hash、进程/端口来源以及调用 evidence；不得保存凭据。测试输出在已有 git-ignored docs/tasks/evidence 下，不会自动进入提交。

Control Receipt 只投影 persisted facts：dispatching/uncertain 的 providerInvoked 为 null；terminal 状态本身不证明派发。cancel-before-dispatch 仍是 false。control=null 表示接受情况证据不可用，不得当成拒绝受理。发生 transport ambiguity 时，只原样重试原 action 和全部参数；start/continue 保留原 requestKey，其他 action 不新增 requestKey。不得自动 replay、创建新 key 或夺取 Claim。

## Stable Core Output Contract

outputSchema 不从内部 Envelope 自动生成。三个共享 $defs 仅复用 Execution、Control 和 NextAction，避免重复展开，不改变 flat inputSchema。Success 必需 ok=true/data/control；Failure 必需 ok=false/error/control。顶层、error、control、progress、availableActions、nextAction 拒绝未知字段。Execution 保持开放，兼容 prompt、时间、身份细节和动态 finalResult；List 包含 executions 数组。

control 允许 null（例如 list、证据读取不可用），providerInvoked 允许 boolean/null。NextAction 公开 action、可选 executionId/waitMs/includeResult，沿用实际 JSON，不建立第二套业务校验。Execution 公开核心身份、状态、semantic revision、结果可用性、progress、nextAction、availableActions、providerTerminalStatus、resultCompleteness、attention。

Output Contract 进入诊断 hash；该 hash 不参与 StateStore、requestKey canonical identity 或 Runtime 安全判断。旧 evidence/hash 不代表新增 outputSchema 已同步。Cloudflare/ChatGPT 刷新后需重新验证 outputSchema；本地 PASS 不等于外部提示已消失。

Product tests 使用仓库 npm install 已提供的 AJV（支持 const、oneOf 和本地引用）验证实际 operation 返回值，Node/npm dependencies 是这组测试的前置条件。没有新增生产或测试依赖。

Fresh start 必须携带调用方期望 workspaceId；新建前在同一事务检查当前冻结 ActiveWorkspace snapshot 的 ID，不匹配返回 AGENT_WORKSPACE_CHANGED，control=false/false/not_dispatched/activate_workspace。相同 key 的重试使用原 snapshot，workspaceId 仍须匹配原 Execution；不同 ID 属于 request-key conflict。

Execution outputSchema 同时声明 prompt、canonicalWorkspaceRoot、threadId/turnId（可为 null）、interrupt 标志和时间字段。unchanged/finalResult 可缺省；finalResult 为开放 JSON Value。

## Startup / restart regression boundary

- `restart_tests::rt01`–`rt06` 使用同一文件数据库关闭旧 Service/Store 后重新初始化，验证 unbound Runtime attempt fail-closed、clean pending、原样 start retry、跨 dispatch 状态、finalizing evidence/result 和 list/history/observe 可见性。
- `startup_guard_product_and_provider_agree_on_persisted_runtime_attempt` 比较 Startup、dispatch guard、Product 与 Provider failure 的 pre-runtime 资格；已有 attempt 不能进入 explicit resume。
- `completed_and_failed_terminal_ack_after_rpc_deadline` 在 exact terminal 已 durable、结果恢复已开始后延迟原 start ACK 超过 RPC deadline。仅 sealed same-runtime、`dispatched` 和 terminal evidence 可使该 ACK 的 deadline 不再阻断 finalization；ACK Turn 身份仍须匹配，其他 RPC deadline 保持不变。
- Finalizing 跨 Runtime recovery 的现有 V0.4 业务终态为 `interrupted`，Provider `completed/failed/interrupted` 和恢复结果保持原身份；要求恢复为 Provider 对应业务终态属于 Material Contract Difference，本轮未修改该规则。
- 本地 Fake App Server / reopen regression 不等于真实 Codex、桌面进程强杀重启或外部 ChatGPT transport 验收。
