# CB5-004 verification

结果：**PARTIAL；真实 cancel/permission Acceptance 尚未证明，等待 Host Gate**。仅交付 task-local 合同 harness、确定性测试和真实阻塞证据。没有进入 CB5-005，没有产品改动或 Git commit。

## 真实执行

Canonical `C:\nvm4w\nodejs\node.exe` + `C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy --acp`，用户 Host Gate 确认 CLI 2.158.0。每次 fresh temp cwd，SDK agent-client-protocol 2.2.0/schema 1.9.1，仅使用 external streams；harness 持有 Child 和三条管道。

| 执行 | 次数 | 观察 |
|---|---:|---|
| before / after / permission 初次 | 3 | initialize v1 成功；session/new JSON-RPC -32603；初版白名单仅保存 code，不回填 HTTP 分类 |
| session/new-only 安全诊断 | 1 | initialize v1；session/new -32603，严格静态分类 HTTP_500；未发送 prompt |
| 三场景显式 fresh followup | 3 | 每项最多追加一次；全部 session/new HTTP_500；未发送 prompt |

共 7 个真实进程。追加前已确认初次没有 prompt wire，原报告保留在 `evidence/initial-scenarios/`。没有自动 retry、没有重放 cancel/prompt、没有基于未知副作用结果重放。父会话停止追加的消息到达时，三项 followup 的同步工具调用已经结束；此后没有真实调用。

7/7 临时 manifest before={}、after={}、delta=[]；7/7 Child reaped、stderr join、临时目录删除成功。这里的零 delta **不是** cancel-before PASS，因为没有 active prompt，也没有 cancel。`providerTerminalReceived=false`，session/new error 不是 prompt terminal。没有伪造 cancelled/failed/completed。

`cancellation.jsonl` 保留两次 before 和两次 after 的握手/失败帧；`permission.jsonl` 保留两次 permission 的握手/失败帧；`permission-options.json=[]` 明确表示没有收到真实 request/options。诊断 wire 在 `session-new-diagnostic.json`。`process-evidence.json` 包含全部 7 次，不以 harness exit0 表示场景成功。

## 尚未证明

- canCancel：未发送真实 session/cancel，terminal/timeout convergence 未证明。
- after-side-effect：未创建 marker，不能声称真实 cancel 不回滚已有副作用。
- permission：未收到真实 session/request_permission，process-scoped ask 规则效果、真实 options、typed deny 和后续 convergence 均未证明。
- 没有 Windows Job-at-creation、进程树 containment、Linux/Docker 或生产 Runtime 验证；只确认 owned direct Child cleanup。

## 实现合同

`CancelNotification::new(sessionId)` 经 SDK `send_notification`，是 notification，无独立 RPC response；关联当前 fresh session 中唯一 prompt RPC id、UUIDv7 simple conversationRequestId。UUID 在发送前以 File::sync_all durable。before 要求 wire prompt 已发送、同 session/update._meta correlation、零 manifest；after 只接受实际 marker exact bytes + SHA256 `613841732b16579f9417ffa2ffeb2a86dab63957b0c8381976403c6ee4960301`。取消不授权 Claim release。

permission 默认 `--permission-mode default`；process-scoped `--settings {"permissions":{"ask":["Write","Edit","Bash"]}}`，tools 限制为 Write。没有读取/修改用户 settings、token 或 credential。typed request 校验 active/session/已观察 toolCall identity；仅 typed RejectOnce，使用广告 optionId，responder 消耗所有权，重复/late fail closed。没有选永久允许。

cancel/deny 后 20 秒未收到 exact prompt terminal 即记录 no_terminal_timeout，再做 bounded direct Child kill/wait；总场景期限 150 秒，runner watchdog 175 秒。fake 使用更短期限。完整 manifest 包含隐藏文件、目录；Windows reparse point 和 symlink fail closed。落盘 wire 白名单不保留 prompt、command、source、label 或任意错误文本。没有保存 stderr 内容。

## 检查

cwd=`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`。下表 TASK=`.trellis/tasks/09-26-cb5-004-cancel-permission-contract`。

| 命令/检查 | 结果 |
|---|---|
| `$env:CARGO_HOME=(Resolve-Path TASK/.cargo-cache).Path; cargo --config 'source.crates-io.replace-with="task-official"' --config 'source.task-official.registry="sparse+http://127.0.0.1:18742/"' test --offline --manifest-path TASK/harness/Cargo.toml` | PASS exit0，9 unit +4 integration=13/13，0 ignored；在 reparse/最终 Rust 修改后运行 |
| `python TASK/test_run_probe.py` | PASS exit0，3/3；真实 bytes hash、禁止重放已发 prompt、拒绝复用 stale output |
| `cargo fmt --manifest-path TASK/harness/Cargo.toml -- --check` | PASS exit0 |
| `python TASK/run_probe.py` | PARTIAL exit1，3 场景均 session/new 阻塞 |
| `harness/target/debug/cb5-004-contract-probe.exe diagnostic TASK/evidence/session-new-diagnostic.json` | harness exit0；session/new HTTP500，诊断不是场景 PASS |
| `python TASK/run_probe.py --fresh-session-new-followup` | PARTIAL exit1；3 fresh 独立追加均 HTTP500；此选项不能重放已发 prompt 或第三次追加 |
| `python TASK/run_probe.py --summarize` | PARTIAL exit1；仅汇总已有证据，无 CLI 调用 |
| `git diff --check` / 父会话 `git diff --cached --check` | PASS exit0 |
| HEAD / tracked hash / 产品 | HEAD=`314687f9ec0ab8bb6115971cf1edc6e8ef116b2d`；父会话 scope-verification：755 tracked SHA256 delta0，产品 delta0 |

13 Rust tests 覆盖 cancel identity mismatch 不发送、permission session/tool mismatch、malformed option/response、重复/late responder、cancel terminal/timeout、late updates、wait/kill 故障有界、actual marker/hash 非 timer、hidden manifest、wire 脱敏，以及真实 SDK fake-peer typed deny 后 end_turn/timeout。Fake PASS 不升级任何真实 capability。

首次编译测试 fixture 缺 ToolCallUpdateFields 参数，修复后测试通过；这不是环境阻塞。依赖 cache 从 CB5-003 只读复制，sha2 0.10.9 及依赖从本机 cargo cache 复制到本 task；全程 offline build，未修改旧 task cache。Cargo.lock 为版本 authority。cache/target 属于生成缓存，排除 review 正文；全部自编源码、fake、tests、runner 纳入 hash。

真实阻塞记录产生后收尾增加了 reparse fail-closed、terminal metadata 白名单、runner fresh output guard 和 fake permission tests；未重新运行真实 CLI。它们不改变已保存 session/new 阻塞事实。最终 Rust/Python 检查针对最终实现。独立 review 由父会话安排，当前文档不自称 Review PASS。

## 交付范围

仅当前 task 的 `harness/`、`run_probe.py`、`test_run_probe.py`、`evidence/`、本报告与 review hashes。原有 .zed 和其它 task artifacts 保留；根 .gitignore 未动。两份主文档的版本 hash 见父会话 scope-baseline/review-context。停止于此，等待 Host Gate。
