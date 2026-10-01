# 当前状态：Stage A/B/C Host Gate 均 PASS

Stage A Continue Host PASS；Stage B Usage Host PASS（supported but completeness-aware）；Stage C Crash Contract Freeze Host PASS。CB5-005 作为 Contract Freeze 任务可以完成。Stage C 冻结结论见 [crash-host-freeze.md](crash-host-freeze.md)，历史 runbook 见 [crash-README.md](crash-README.md)，schema proposal 见 [codebuddy-private-state-dcr.md](codebuddy-private-state-dcr.md)。Stage A/B/C raw evidence 均冻结，禁止重跑 resume/load/usage/usage-repair/crash-before/crash-after 或任何真实 CodeBuddy CLI。

结论边界不变：Crash result recovery 最多 `partial`；R2 load/result recovery 不能证明 R1 Runtime termination，不能授权 Claim release；生产 Runtime/Windows Job/StateStore 尚未实现。本任务完成只表示协议证据与 Contract Freeze 收口，不是生产实现验收。

以下Stage A handoff为历史记录，命令已消费，不可再次执行。

# CB5-005 Stage A — Host handoff

仅 continuation：resume/load 独立探针。该段记录执行前 handoff；其 Host 命令已经消费，当前 Stage A 已 PASS。本任务没有修改生产代码。

## 已消费的 Host 命令（历史记录，禁止重跑）

父 Host 已在 Windows 标准用户环境、仓库 ordinary Win32 cwd 下完成执行。binary 直接启动 `C:\nvm4w\nodejs\node.exe` + `C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy` + `--acp`，继承 Host 环境，不读取/保存完整 env 或 credential。

历史执行顺序为先 resume、再独立 load；没有使用 `&&`、自动 fallback 或重试。以下命令仅用于标识已消费的执行。

```powershell
& .\.trellis\tasks\09-27-cb5-005-continuation-usage-crash-contract\harness\target\debug\cb5-005-continuation.exe resume
```

```powershell
& .\.trellis\tasks\09-27-cb5-005-continuation-usage-crash-contract\harness\target\debug\cb5-005-continuation.exe load
```

binary 固定写入编译时本 task 的 `evidence/continuation/`。每项只创建一次 `METHOD.attempt-started.json`（create_new + sync_all），随后写 `METHOD.result.json`；无 output/force/retry 参数。不要删除 sentinel 或移动/重新编译副本来重放。sentinel 存在但 result 缺失代表执行结果未知，必须停止，不能重试。

## 结果解释

- PASS：R1 exact terminal + reap，R2 exact S1/cwd typed recovery、P2 exact terminal、当前 conversationRequestId 的答案 trim 后与 token 完全相等；两阶段 manifest 无差异，cleanup/空 root 删除成功。
- UNSUPPORTED：本 recovery method 返回 JSON-RPC -32601，child 已回收。不切换方法。
- PARTIAL：包括 ACK 成功但无法回忆 token、错误身份、未归因 live answer、异常 terminal/timeout、manifest 或 cleanup 失败。
- exit 0 仅 PASS；其余 exit 2。真实证据由 Host 审核，fake tests 不等于 Host PASS。

使用 `agent-client-protocol =2.2.0` typed ResumeSessionRequest / LoadSessionRequest；当前锁定 schema 1.9.1：load 序列化 `mcpServers:[]`，resume 省略空 `mcpServers`。证据记录实际字段存在性，不补造一致 shape。response `sessionIdPresent=false` 合法；请求 exact S1、响应若有回显必须匹配，early notifications session 也必须匹配。

R1 new 前后通知标记 `session_new`；R2 请求至 P2 发出前标记 `recovery`，记录所有 update 和 history 子集的 type/count/order。recovery response 后 250ms 接收窗口，不代表无期限 replay completion 保证；P2 无当前 correlation 的正文不算答案且使结果 PARTIAL。terminal 后 250ms `late` 单独记录。finalAnswerLength 为 UTF-8 bytes，正文不落盘；catalog 字符串保留 hash/长度，其他字段只保留白名单结构。

每 runtime 120s，child kill/wait ≤5s、stderr join/abort ≤2s；两次 runtime 合计约254s加本地文件检查。frame ≤1 MiB，每方向≤2048 frames，总 wire 内存约4 MiB；manifest ≤4096项/32 MiB，超限 fail closed。全量 manifest 包含隐藏项，路径 hash、文件长度/hash。异常目录保留供 Host 检查，不做递归删除。

P1/P2 都要求只读且不使用工具，permission 只会 typed RejectOnce，不授予执行权限；Provider 在其进程内部的工具行为以 manifest 为证据，本探针不是 OS sandbox。owned child reap 不证明 Windows Job/tree containment，不涉及 Claim。

## 可重现的 Windows fake 验证（不调用 Provider）

仓库根目录运行；缓存从既有 CB5-004 官方 crate 缓存复制到本 task。source replacement 仅指向该已有缓存，`--offline` 不访问本地代理。

```powershell
$env:CARGO_HOME=(Resolve-Path .trellis/tasks/09-27-cb5-005-continuation-usage-crash-contract/.cargo-cache).Path
& C:/Users/lifei/.cargo/bin/cargo.exe --config 'source.crates-io.replace-with="task-official"' --config 'source.task-official.registry="sparse+http://127.0.0.1:18742/"' test --offline --locked --manifest-path .trellis/tasks/09-27-cb5-005-continuation-usage-crash-contract/harness/Cargo.toml
& C:/Users/lifei/.cargo/bin/cargo.exe --config 'source.crates-io.replace-with="task-official"' --config 'source.task-official.registry="sparse+http://127.0.0.1:18742/"' build --offline --locked --manifest-path .trellis/tasks/09-27-cb5-005-continuation-usage-crash-contract/harness/Cargo.toml
& C:/Users/lifei/.cargo/bin/cargo.exe fmt --manifest-path .trellis/tasks/09-27-cb5-005-continuation-usage-crash-contract/harness/Cargo.toml -- --check
```

fake tests 固定调用已安装 `C:\Users\lifei\AppData\Local\Programs\Python\Python312\python.exe -B harness/fake_peer.py`，仅 fixture 常量 token；随机真实 memory token 从不写入 fake 日志。Windows 专用验证，不声称 Linux validation；未使用 WSL。
