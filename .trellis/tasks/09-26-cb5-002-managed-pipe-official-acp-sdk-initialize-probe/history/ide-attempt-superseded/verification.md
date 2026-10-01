# CB5-002 verification — BLOCKED

真实 CodeBuddy ACP initialize **未证明成功**；不进入 CB5-003，不等待产品版本 allowlist，不启用任何 Provider capability。

## 已验证

- 官方 `agent-client-protocol = 2.2.0`（default features=false），schema 1.9.1。`ByteStreams::new` 接收 harness 已创建的 ChildStdin/ChildStdout，SDK 不持有 Child，不调用 AcpAgent spawn helper。
- Windows 本机 task-local Rust crate：5 unit + 6 external-process integration tests PASS，0 failed。
- 外部 fake peer：EOF-before / EOF-during 返回稳定 `incoming_transport_closed`；numeric 999 成功 deserialize，protocol gate 拒绝；缺 optional capability 且 numeric 1 连接 PASS，loadSession=false；正常关闭后退出0；不响应 peer 超时后 harness terminate 并 wait。
- 最终 direct PID 检查均不存在；所有已记录临时 cwd 已移除；下载代理监听端口已关闭。
- `cargo fmt --check` PASS。产品 baseline / git 状态保留、最终独立 review 由主会话验证，本文不提前宣告通过。

## 真实 probe 一手结果

规范 EXE + `resources/app/out/cli.js --acp`：initialize 161ms 返回 SDK `Incoming transport closed`，Child exit0，wait 已完成。
增加 `--permission-mode auto`：initialize 158ms 同样 EOF，Child exit0，wait 已完成。
两次 stdout 均输出 `To read from stdin, append '-' (e.g. 'echo Hello World | buddycn -')`，非 ACP JSON；stderr 明确提示参数未知、转交 Electron/Chromium。SDK 随后发出的 JSON-RPC parse-error 帧保留真实 `id:null,error.code:-32700`，没有伪造 method/params 字段。

这些只是失败的实际 invocation 候选，不是可用 ACP argv。未收到真实 initialize response，因此真实 negotiatedProtocolVersion/capability 均 unknown。安装只读检索覆盖 3467 个 JS/JSON/CMD，未发现两个 CLI flag；找到 genie/out extension 和 out/codebuddy/main.js 中的窗口消息 ACP bridge，尚未建立 external stdio 入口。不能扩大结论为所有 CodeBuddy 安装均不支持 ACP。

## 门禁与证据

`protocolCompatible = initializeSucceeded && negotiatedProtocolVersion == 1`。支持版本来自冻结设计 stable ACP v1。请求 protocolVersion JSON 类型 number、值1；mismatch fixture response 为 number、值999。ProductVersion/FileVersion/commit/binary hash 都不参与门禁；capability 不参与门禁。

真实 canExecute/canContinue/canCancel/canRecover/activity/tokenUsage 均 unknown / not enabled，仅 fake 缺能力 fixture 证明 loadSession=false 不影响连接。未调用 session/new、prompt、authenticate；未修改 src/src-tauri 或产品 Cargo。

- `evidence/initialize.jsonl`：传输 tee 顺序、原请求 rawLine、公开字段响应、非 JSON 诊断；有删除的错误 data 标记 redacted=true。
- `evidence/sdk-details.json` / `sdk-feature-tree.log` / task-local Cargo.lock：准确依赖与 API 证明。
- `evidence/process-evidence.json` / `direct-process-check.json`：argv/env delta/cwd/PID/timeout/exit/cleanup。
- `evidence/protocol-gate.json`：真实与 fixture 门禁矩阵。
- `evidence/cargo-test-final.log`：5+6 tests，全部执行通过。

## 命令与环境

仓库 cwd：`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`。以下 `TASK` 表示本 task 目录。

- `cargo search agent-client-protocol --limit 3`：FAIL，Schannel SEC_E_NO_CREDENTIALS，下载环境问题。
- `python TASK/cargo_bootstrap.py fetch`：初次 localhost 请求受宿主 proxy 影响被 bounded 终止；关闭仅 Cargo 子进程的 HTTP proxy 后 retry PASS，未禁用上游 TLS 验证。
- `python TASK/cargo_bootstrap.py test --offline`：PASS，实际 Cargo cwd 为 `TASK/harness`；CARGO_HOME 仅 task `.cargo-cache`。
- `cargo fmt --manifest-path TASK/harness/Cargo.toml -- --check`：PASS。
- `TASK/harness/target/debug/cb5-002-initialize-probe.exe fixture TASK/evidence/<case>.json <case>`：5场景已运行。
- 同 executable `real TASK/evidence/real-default.json` / `real TASK/evidence/real-auto.json auto`：命令 exit0 表示证据与 cleanup 完成，**不表示真实 ACP Gate PASS**。

没有 Linux 验证，也没有把 Windows 结果当作 Linux 证据。下载缓存与 target 被 task-local .gitignore 排除，不能作为独立审查源码。

## 明确限制

真实 ACP entry 未建立，任务 Gate BLOCKED；SDK external streams 支持已由编译与外部进程 fixtures 证实，不允许 NDJSON fallback。
只证明 harness 直接 Child 已有界 wait/reap。CLI 候选会转交桌面 Electron，未记录其短寿命后代树，**后代无 orphan 尚未证明**；不将 direct-child 检查夸大为进程树清理或 Windows Job-at-creation 证明。Phase 6 Job 未实施。

保持本卡等待 Host Gate；当前实际 ACP invocation 尚未建立，不能完成真实 initialize acceptance。

## 独立 review repair round 1

两项 lifecycle P1 已修复：Rust cleanup 不再因 kill/wait 错误提前返回，始终有界处理 stderr abort/join；cleanup.errors/succeeded 明确保存异常，失败不伪造 wait/reap，CLI 保存证据后返回非零。新增真实 Child + 注入首个 wait error 的回归，验证仍回收并 join。

集成测试 watchdog 先获取自有 fake peer PID 对应的 Windows handle，再尝试 taskkill /T /F；树命令未生效时直接 TerminateProcess 已知 peer，WaitForSingleObject 有界确认退出，同时有界回收 harness。新增 watchdog 抢先终止 harness 的实际 Windows 回归 PASS。此证据仅覆盖受控 fake peer，不能延伸为真实 Electron 后代证明。

Python 下载辅助脚本 taskkill 失败/超时仍进入 direct kill+wait，分别收集错误；2 个注入 OS 拒绝/超时的单测 PASS。最终 Rust 11/11 + Python 2/2 PASS；fmt PASS。真实两组 SDK probe 重新运行仍 EOF/BLOCKED，所有记录 cleanup.succeeded=true/errors=[]。
