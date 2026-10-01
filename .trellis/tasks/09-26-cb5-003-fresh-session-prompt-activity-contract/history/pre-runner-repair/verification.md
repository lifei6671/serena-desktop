# CB5-003 verification — BLOCKED，UUIDv7 格式修复完成，等待 Host Gate

本轮已修复 Host 指出的 conversationRequestId 格式错误，并实际运行完整 harness A/B。两场景各遇 session/new HTTP 500 后，用新进程和新临时目录重试一次；四次 initialize 均 PASS，四次 session/new 均返回错误，未取得 sessionId，未发送 prompt。**canExecute contract evidence BLOCKED，本轮不修改产品 capability。**

500 是本轮四次请求的观察结果，不冻结为永久或稳定服务端阻塞。Host 独立成功调用 session/new 的事实不替代本 harness 的实际失败证据。无 authenticate、token/登录文件访问、browser 操作或 `--agent cli`。

## UUIDv7 修复

- task-local uuid dependency 启用 `v7`；生成方式 `uuid::Uuid::now_v7().simple().to_string()`。
- 值必须恰好 32 个 `[0-9a-f]` 字符，无连字符，解析后 version=7；使用官方 `PromptRequest.meta(Map)` 写入唯一准确键 `codebuddy.ai/conversationRequestId`。
- A 的 prompt 不包含 `_meta`；B 的值由 harness 生成，Provider-private，不从 prompt/用户输入推导。
- deterministic test 验证上述长度、字符、无连字符、UUID version、metadata 原值及 A 不发送 metadata。
- B attempt1 生成 `01a0dde0d3e4713eaa49dfde2bddb2d2`；attempt2 生成 `01a0dde0e09976d2912c1b6ef9c2452c`。两者均因 session/new 失败而未发送；**不得声称 CodeBuddy 已接受 UUIDv7 prompt**。
- 只更新本 task prd/design；主 technical design / task breakdown 未修改。

## 实际场景与重试

| 场景 | attempt | 临时 cwd basename | PID | initialize | session/new | delta / cleanup / 删除 |
|---|---|---|---|---|---|---|
| Read A | 1 | cb5-003-Zu8CUA | 4760 | PASS，number 1 | -32603 / HTTP500 | 0 / PASS / PASS |
| Read A | 2 | cb5-003-FbcxwJ | 45188 | PASS，number 1 | -32603 / HTTP500 | 0 / PASS / PASS |
| Write B | 1 | cb5-003-eAKgnr | 48504 | PASS，number 1 | -32603 / HTTP500 | 0 / PASS / PASS |
| Write B | 2 | cb5-003-bCNJ6N | 3928 | PASS，number 1 | -32603 / HTTP500 | 0 / PASS / PASS |

所有 cwd 位于 `C:\Users\lifei\AppData\Local\Temp`，由本卡创建，canonical absolute path 同时作为 Child cwd 与 session/new.cwd。每次均为新进程、新临时目录。四个 `*-attempt-*.json` 独立保存，最终 `read-only-result.json` / `isolated-write-result.json` 对应各自 attempt2。

每次 exact error JSON：`{"code":-32603,"message":"Internal error","data":{"details":"Request failed with status code 500"}}`。`fresh-session.jsonl` 共16帧，每次4帧 initialize request/response + session/new request/error；保存 scenario、attempt、局部 sequence 与 globalSequence，以及 exact rawLine。四次均未返回 sessionId；SDK/raw terminal 均不存在，prompt RPC id 为 null。

Read-only input.txt 实际27字节，before/after SHA256 均为 `26acb2f57881ef83f8dcac111e4f1fbc30fef0c3c115b0700bf6882a11558db2`。写场景目录仍空，没有 output.txt。0 delta 只证明本次失败尝试未改变目录，不能当成 read prompt 或 isolated-write acceptance PASS。Manifest 递归包含隐藏文件/目录，hash 源自实际字节。

## 关联 / Activity / terminal 证据

- conversationRequestId required / optional / ignored / echoed / correlation-useful 全部 NOT_PROVEN，真实 prompt 未发送。
- reporter 分开保存 B PromptResponse.result._meta、session/update.params._meta、session/update.params.update._meta；另外收集已脱敏的 public providerData correlation 字段，避免把 terminal echo 推断成所有 notification 已关联。
- metadata/providerData 仅保留公开 conversationRequestId/requestId/sessionId/toolCallId 关联字段和层次，私有任意数据不落盘。当前四次 wire 没有需要脱敏重建的帧。
- 每次实际0条 session/update，是未达到 prompt 的观察结果；Activity richness、顺序与 CodeBuddy prompt terminal 仍未证明。
- safe error prompt terminal = NOT_PROVEN。session/new 错误不是 prompt terminal。
- fake peer 的 update-before-terminal 与 terminal stopReason JSON preservation 只证明 harness，不替代真实契约。

## 进程与 SDK

canonical executable=`C:\nvm4w\nodejs\node.exe`，argv[0]=`C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy`，参数 `--acp`；仅 write 使用 `--permission-mode auto`。没有 `--agent cli` 或真实项目 cwd。

官方 `agent-client-protocol=2.2.0` / schema1.9.1；harness 自己 spawn 并持有 Child/stdin/stdout/stderr，SDK 仅收到 external ByteStreams。fs.readTextFile=false、fs.writeTextFile=false、terminal=false。意外 session/request_permission 仍 fail closed，不实现权限策略。

唯一协议 gate 是 initialize success + protocolVersion==1。版本/hash/capability/child exit code 不参与。Raw initialize 的 delegateToolsSupport/mainAgentSupport/multitaskSupport 与 typed DTO 差异分别保存；SDK typed 补 audio=false 等默认值，不替代 raw wire。

每次 SDK 流关闭后最多2s宽限，然后 start_kill + 最多5s wait，stderr join最多1s + abort/join最多1s。四次均 directChildReaped=true、stderrJoined=true、errors=[]、exitCode=1；全部临时目录在 manifest 捕获后删除。没有 Job-at-creation 或进程树 containment/Claim release 证明。未保存 stderr 文本、完整环境或凭据。

## 验证

cwd=`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`（普通路径）；`TASK=.trellis/tasks/09-26-cb5-003-fresh-session-prompt-activity-contract`。

| 命令 / 检查 | 本轮结果 |
|---|---|
| `CARGO_HOME=TASK/.cargo-cache cargo --config 'source.crates-io.replace-with="task-official"' --config 'source.task-official.registry="sparse+http://127.0.0.1:18742/"' test --offline --manifest-path TASK/harness/Cargo.toml` | PASS，exit0，7 unit + 7 integration = 14/14 |
| `cargo fmt --manifest-path TASK/harness/Cargo.toml -- --check` | PASS，exit0 |
| `python TASK/run_probe.py` | BLOCKED，exit1；4次真实CLI调用，包含每场景唯一一次HTTP500重试 |
| `python TASK/run_probe.py --summarize` | exit1，保留BLOCKED；仅刷新attempt标识与echo NOT_PROVEN summary，不启动CLI |
| 产品 src/src-tauri baseline | PASS，319文件 delta0；父会话 evidence/scope-verification.json |
| 全仓 `git diff --check` | PASS，exit0；父会话 scope evidence |
| HEAD / tracked status | `5179248cfa30096df912c6567aee2e80e3bf4352`；trackedChanges=[] |

Tests 包含 missing/empty/malformed sessionId、prompt EOF/timeout、update-before-terminal/terminal preservation、manifest unexpected hidden write、UUIDv7 A/B serialization、公开metadata位置/脱敏，以及 wait/kill failure bounded cleanup。测试全部实际执行，没有以静态检查替代。

## 归档与范围

`history/pre-login/` 保留首次认证错误阶段；`history/pre-uuidv7/` 保留修复前30份source/evidence/review/docs并带逐文件SHA256 manifest，明确历史状态。原 baseline.json 未改。旧500记录是当时请求结果，不是永久服务故障结论。

本轮变更只在task-local Cargo UUID feature、UUID生成/测试、公开关联保留、限定重试/evidence reporter、prd/design/verification 与证据。src/src-tauri、产品Cargo、主设计文档、root .gitignore、.zed及其它task未修改；未提交Git，未进入CB5-004。独立 review/review-target由父会话完成。已达到每场景一次重试上限，停止并等待 Host Gate。
