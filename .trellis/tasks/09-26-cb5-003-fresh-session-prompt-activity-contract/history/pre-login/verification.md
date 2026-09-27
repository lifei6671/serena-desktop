# CB5-003 verification — BLOCKED，等待 Host Gate

真实 CodeBuddy Code 的两个独立临时场景均成功 initialize，随后 `session/new` 返回认证错误。没有取得 sessionId，没有发送真实 prompt，没有真实 prompt terminal。**canExecute contract evidence BLOCKED；不得据此开启产品能力。** 本次不调用 authenticate，不重试认证，不进入 CB5-004。

## 一手结果

| 合同 | Read-only A | Isolated write B |
|---|---|---|
| initialize / protocolVersion | PASS，JSON number `1` | PASS，JSON number `1` |
| session/new | BLOCKED，error code number `-32000` | BLOCKED，error code number `-32000` |
| error.message / data.category | `Authentication required` / `auth` | `Authentication required` / `auth` |
| exact sessionId | 未返回，null | 未返回，null |
| session/prompt / RPC id | NOT_RUN，null | NOT_RUN，null |
| session/update | 实际捕获 0 条；未到 prompt | 实际捕获 0 条；未到 prompt |
| prompt terminal / stopReason | NOT_RUN，null | NOT_RUN，null |
| 完整递归 manifest delta | 0，input.txt 27 bytes 未改变 | 0，空目录未改变；output.txt 未生成 |
| 进程清理 / 临时目录删除 | PASS | PASS |

`fresh-session.jsonl` 共 8 条真实 ordered wire，每场景 4 条：initialize request/response、session/new request/error。`rawLine` 保留实际 NDJSON；此轮所有 8 帧无需脱敏重建。公开 error JSON 精确保留，SDK error 文本另外记录在 result 的 `error` 字段。不存在可比较的 typed session/new success DTO。

Read-only cwd 为 `C:\Users\lifei\AppData\Local\Temp\cb5-003-O1PZxR`；write cwd 为 `C:\Users\lifei\AppData\Local\Temp\cb5-003-ot8DA9`。两者均为本卡创建的独立 canonical absolute Windows 路径，子进程 cwd 与 session/new.cwd 相同。两目录已在证据捕获后删除。

Read-only seed 的实际 SHA256 为 `26acb2f57881ef83f8dcac111e4f1fbc30fef0c3c115b0700bf6882a11558db2`，before/after 相同。Write 未执行，不能把 0 delta 说成预期写入通过。Manifest 包含隐藏文件和目录，读取实际字节后计算 SHA256；不是从期望 marker 合成 hash。

## Process / SDK / 协议

- canonical executable=`C:\nvm4w\nodejs\node.exe`；argv[0]=`C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy`；共同参数 `--acp`；仅 isolated write 添加 `--permission-mode auto`。
- 官方 SDK `agent-client-protocol=2.2.0`，官方 schema `1.9.1`，default-features=false。Harness 自己 spawn 并持有 Child/stdin/stdout/stderr；SDK 只收到 Tee 包装的 external ByteStreams。
- initialize 明确发送 fs.readTextFile=false、fs.writeTextFile=false、terminal=false、auth.terminal=false。没有代理客户端文件系统或终端。
- initialize 唯一兼容规则为 success + protocolVersion==1。CLI version、hash、capability、child exit code 都不参与协议兼容判断。
- Raw initialize 有 delegateToolsSupport/mainAgentSupport/multitaskSupport extension，SDK typed 丢弃这些字段；typed 另外补 audio=false 等默认值，结果中分别保留 raw 与 typed。
- 两场景 SDK 流关闭后 2s 宽限；随后 start_kill + 最长 5s wait，stderr join 最长 1s + abort/join 最长 1s。实际均 directChildReaped=true、stderrJoined=true、errors=[]、exitCode=1；exitCode 不反向否定 initialize。
- 不保存环境或 stderr 文本；仅记录 stderr drain 字节数。没有 Windows Job-at-creation、进程树 containment 或 Claim release 证明；Phase 6 仍未执行。

## conversationRequestId / Activity / Terminal

官方 `PromptRequest.meta(Map)` 可表达 `_meta["codebuddy.ai/conversationRequestId"]`，无需 task-local raw adapter。A 无 metadata、B harness-generated UUID 的序列化由 deterministic Rust test 证明。真实 B 已生成 UUID，但 `session/new` 失败，**UUID 没有发送**。

真实 required / optional / ignored / echoed / correlation-useful 全为 NOT_PROVEN，不作采用建议。`conversation-request-id.json` 区分 generated 与 actually sent。

Fake peer 已证明 update-before-terminal 的 wire 次序、terminal stopReason/result extension 保留、malformed terminal raw 反证保存。它们只证明 harness 行为，不是 CodeBuddy Activity 或 terminal 契约。真实 safe error prompt terminal 为 NOT_PROVEN；当前 session/new 认证错误不是 prompt terminal。

## 验证命令

cwd=`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`；以下 `TASK=.trellis/tasks/09-26-cb5-003-fresh-session-prompt-activity-contract`。

| 命令 | 结果 |
|---|---|
| `CARGO_HOME=TASK/.cargo-cache cargo --config 'source.crates-io.replace-with="task-official"' --config 'source.task-official.registry="sparse+http://127.0.0.1:18742/"' test --offline --manifest-path TASK/harness/Cargo.toml` | PASS，exit 0，6 unit + 7 integration = 13/13 |
| `cargo fmt --manifest-path TASK/harness/Cargo.toml -- --check` | PASS，exit 0 |
| `python TASK/run_probe.py` | BLOCKED，exit 1；实际完成两次 CLI 启动与认证错误捕获 |
| `python TASK/run_probe.py --summarize` | exit 1，按未通过 acceptance 返回；仅整理既有证据，不重启 CLI |
| 产品 src/src-tauri hash baseline | PASS，319 文件 delta 0；见父会话 `evidence/scope-verification.json` |
| 全仓 `git diff --check` | PASS，exit 0；见父会话 scope evidence |
| HEAD / tracked status | HEAD 保持 `5179248cfa30096df912c6567aee2e80e3bf4352`；trackedChanges=[] |

Tests 覆盖 missing/empty/malformed sessionId、prompt EOF/timeout、update ordering、raw/typed terminal 差异、unexpected hidden write、A/B metadata serialization、cleanup wait/kill 错误后仍有界执行后续回收。没有用 fake peer 结果代替真实验收。

## 交付范围与限制

修改仅限本 task：`harness/Cargo.toml`、`Cargo.lock`、`src/main.rs`、`src/transport.rs`、`tests/contracts.rs`、`run_probe.py` 及 evidence/verification。CB5-002 缓存只读复制到本卡缓存，本卡 build/cache 为本地验证产物。未修改产品 Provider/Runtime/MCP、产品 Cargo 文件、真实用户项目；未提交 Git，保留 `.zed` 和其它 task。

独立 review、review-target hashes 由父会话完成。当前阻塞是 canonical CLI 在 session/new 要求认证；本卡无权限执行 authenticate。等待 Host Gate，不扩展或自动恢复任务。
