# CB5-002 verification — PASS，等待 Host Gate

当前独立 CodeBuddy CLI 的真实 initialize 已通过官方 Rust SDK external-managed pipes。未进入 CB5-003，没有产品源码或产品依赖改动。

协议 contract probe 为 PASS；完整交付检查为 PARTIAL：全仓 `git diff --check` exit=2，唯一错误为 Host 冻结的 `docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md:794` trailing whitespace。该文件 SHA256 仍为本轮输入的 `dca62bdc54aaab7e418335beea53f189bc74243d08fb865cf298e696eadebb46`，故属本轮开始前已有内容；不越出 task-local 范围修改。主会话复核产品315文件零变化、Git状态保留，见 `evidence/scope-verification-resume-final.json`。不能将全仓检查报告为 PASS。

## 真实结果

- 只读确认 `@tencent-ai/codebuddy-code@2.158.0`，Node `v24.19.0`；package/shim/script/node 的 hash 在 `evidence/cli-identity.json`，仅用于诊断。
- canonical argv：`C:\nvm4w\nodejs\node.exe C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy --acp`。
- 临时 cwd，stdin/stdout/stderr 全部 pipe；环境无 IDE override；20s initialize timeout。
- 官方 `agent-client-protocol=2.2.0` / schema1.9.1，defaultFeatures=false；harness `tokio::process::Child` 持有 process authority。SDK 只取得 `ByteStreams::new(ChildStdin.compat_write(), ChildStdout.compat())`。
- initialize 846ms 成功；request / response 的 `protocolVersion` 均为 JSON number、值1。`protocolCompatible=true`。
- 先关闭 SDK/流；Node 在2s宽限内未自行退出，harness 执行终止并 wait：child exit1、directChildReaped=true、stderrJoined=true、cleanup.succeeded=true、errors=[]；总耗时2878ms。exit1 是独立 cleanup 结果，不反向否定 initialize 成功。
- Toolhelp 快照 + 进程句柄观察141次，记录 Node 与它的启动环境探测 shell/console 后代共12个；所有已观察后代均在 observer cleanup 前退出，未由 observer 额外杀进程。所有已记录临时 cwd 已移除，最终 direct PID 查询均不存在。

## Capability / authentication surface

Raw initialize 实际返回：

| 字段 | 实际值 |
|---|---|
| promptCapabilities.image / embeddedContext | true / true |
| mcpCapabilities.http / sse | true / true |
| loadSession | true |
| delegateToolsSupport | true |
| mainAgentSupport | false |
| multitaskSupport | true |

原始 response 未声明 audio；SDK typed projection 默认 audio=false。SDK 丢弃上述三个非标准 capability extension，所以 `response`/raw wire 与 `sdkResponse` 分别保存，不能用 typed DTO 替代原始 surface。

公开 authMethods：`iOA`（Login with iOA）、`external`（Login with Google/Github）、`internal`（Login with WeChat）、`selfhosted`（Login with Enterprise Domain）。只保存 id/name，剔除描述/元数据，不记录 token/credential，未调用 authenticate。

唯一 gate：`initializeSucceeded && negotiatedProtocolVersion == 1`。产品版本/hash、capability、进程 exit code 均不参与 gate。没有启用 canExecute/canContinue/canCancel/canRecover/activity/tokenUsage 产品能力；loadSession 广告不等于 recovery method 证据，Fresh Execute 留给 CB5-003。

## 验证

Windows 正常工作区 cwd：`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`。下列 TASK 表示当前 task 目录；Cargo cwd 为 `TASK/harness`。

| 命令 / 场景 | 结果 |
|---|---|
| `python TASK/cargo_bootstrap.py test --offline` | PASS：5 unit + 6 integration，11/11，0 failed |
| `python TASK/test_cargo_bootstrap.py` | PASS：2/2 cleanup 注入失败路径 |
| `cargo fmt --manifest-path TASK/harness/Cargo.toml -- --check` | PASS |
| `python TASK/run_real_probe.py` | PASS：真实 SDK initialize + 已观察进程树退出证据 |
| harness `fixture ... eof-before/eof-during` | PASS：稳定 incoming_transport_closed，无 hang |
| harness `fixture ... mismatch` | PASS：number999 能 deserialize，但 protocolCompatible=false |
| harness `fixture ... missing-capability` | PASS：raw capabilities={}，SDK loadSession=false，protocolCompatible=true |
| harness `fixture ... hang` | PASS：initialize timeout；harness terminate/wait |
| lifecycle failure regression / watchdog | PASS：wait错误后仍清理stderr与Child；已知peer handle有界终止并确认退出 |

保留此前 review round1 lifecycle 修复。没有 Linux 验证，不把 Windows 本机结果当 Linux 证据。独立 review 与最终产品 baseline/hash/git 状态核对由主会话执行，本文不替代 Host Gate。

## Evidence 与历史

- `evidence/initialize.jsonl`：当前真实 CLI 与5个 fake 场景的有序 sanitized wire；`rawLine` 保留原始未改帧，脱敏重建帧标记 redacted=true。
- `evidence/real-cli.json`：actual raw response、SDK typed response、argv/env/cwd/PID/initialize/cleanup。
- `evidence/real-cli-process-tree.json`、`process-evidence.json`、`direct-process-check.json`：本次进程观察与退出。
- `evidence/protocol-gate.json`、`sdk-details.json`、`sdk-feature-tree.log`、Cargo.lock：兼容规则与官方 API/依赖证据。
- `history/ide-attempt-superseded/HISTORY.json`：恢复前50个文件的不可变副本与 hashes；旧 IDE real-default/real-auto 保留原位及历史副本。IDE wrapper EOF 属于被替代目标的历史失败，不是当前独立 CLI gate。
- Cargo 初始 Schannel 与 localhost proxy 下载失败保留历史；目前离线缓存运行，无 SDK API fallback。

## 限制

Toolhelp sampling 证明本次已观察进程均已退出，不是原子进程 containment，也不证明从未产生短寿命未观察后代。没有 Windows Job-at-creation 证据或实现，Phase 6 仍负责生产 Job。未调用 session/new、prompt、authenticate；没有安装/升级/登录、真实 workspace prompt/write、产品 Provider/Runtime/MCP 改动或 Git commit。

本卡停止，等待 Host Gate。
