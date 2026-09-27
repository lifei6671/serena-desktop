# 当前状态：Stage B Host Gate PASS

唯一 usage-repair 已由父 Host 完成，合同见 [usage-host-freeze.md](usage-host-freeze.md)。原始 evidence 与旧 timeout 保留；以下 handoff 命令和 NOT_RUN 是历史记录，**不得再次执行 usage / usage-repair / resume / load**。本轮没有新 Host 命令。生产实现与 Crash 未验收，CB5-005 仍 in_progress。

## 历史 handoff（已消费，不可重跑）

# Stage B Usage — Host handoff

仅 Usage；Stage A 的 resume/load Host PASS 与源码保持冻结。本轮不重跑 continuation，不做 Crash，不改生产源码。真实 usage **NOT_RUN**。

## 唯一 Host 命令

父 Host 在仓库根目录的 ordinary Win32 路径、标准用户环境执行：

```powershell
& .\.trellis\tasks\09-27-cb5-005-continuation-usage-crash-contract\harness\target\debug\cb5-005-continuation.exe usage
```

R1 direct installed `C:\nvm4w\nodejs\node.exe` + `C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy` + `--acp`；initialize/new S1/P1/P2 terminal/reap。R2 fresh initialize/typed session/resume exact S1及同cwd/P3 terminal/reap。继承Host env，任何prompt都禁止tools及文件读写，permission只拒绝；无load fallback。

固定文件：`evidence/usage/usage.attempt-started.json`、`usage.result.json`、`usage-analysis.json`。sentinel create_new+fsync，无force/retry/output override。失败保留sentinel；结果缺失或只写出部分文件表示未知结果，不能重试。不要移除sentinel、复制/重编译到其他task路径以绕过。

PASS/exit0仅表示三turn的terminal、exact identity、零workspace delta、有界cleanup及采样完整，不表示token_usage=true。失败/不完整exit2，P3可能NOT_RUN。`usage-analysis.json`分observations与interpretation，public Usage默认unknown，token_usage=false；即使显式breakdown出现，也须Host冻结其跨turn/restart语义后才能提升能力。

## 采样语义

- 每条样本包含runtime ordinal、wire sequence、source、phaseAtReceipt和phase。phase为new/P1/P2/resume/P3/late。当前phase只说明接收窗口；有correlation才归因exact_conversation，PromptResponse按exact_rpc_id，无correlation为window_only。旧P1 correlation在P2窗口出现仍标late/P1。
- usage_update通过官方typed SessionNotification/UsageUpdate和exact S1后投影所有非敏感number/bool/null。session_info_update、PromptResponse result/_meta仅投影usage/token/context/cost相关字段。扩展数值不声明已由SDK验证语义。
- 已知键保留可读field path，未知扩展键用SHA256路径；字符串只保留存在/类型，数组仅保留结构/元素数，对象只保留结构和安全叶子。敏感key及子树一律redacted，不落任意值。currency等字符串也不保存。
- used/size对应官方schema的context occupancy/window gauge；观测表保留下降、归零和size变化。latestSnapshot与lastPositiveSnapshot同时保留，零值不自动替换最后正值。禁止delta/长度/cost反推token。
- token breakdown只识别显式allowlist（inputTokens/prompt_tokens、outputTokens/completion_tokens、cachedRead/WriteTokens及cache_read/cache_creation_input_tokens、cachedTokens、totalTokens等）。模糊myTokenCount保留数值观察但不算breakdown；secret-looking字段即使带token字样也拒绝。
- terminal前最后usage、每turn late usage、R1末与R2首快照均显式记录。250ms late窗口有界，窗口外未观察到的消息不能声称不存在。cost字段保留其frame/phase/correlation来源；官方cost schema是session cumulative提示，extension实际归属仍须Host判断。

答案仅SHA256/UTF-8 bytes，缺少correlation的answer chunk计数说明摘要不完整；不会因此猜测正文。所有prompt、agent文本/thought、完整env、stderr、credential不落盘。manifest包括隐藏项/文件hash/长度，reparse/symlink拒绝；异常目录不递归删除。owned child reap不等于Windows Job/tree containment。

## 边界与验证

R1总timeout240s，R2总timeout120s；单child kill/wait≤5s，stderr join/abort≤2s。复用Stage A约4MiB总wire/1MiB单帧/每方向2048帧边界。数字投影每runtime最多8192结构/叶节点、深度24；超限失败而非静默截断。manifest≤4096项/32MiB。只运行Windows验证，不运行WSL、不声称Linux PASS。

```powershell
$env:CARGO_HOME=(Resolve-Path .trellis/tasks/09-27-cb5-005-continuation-usage-crash-contract/.cargo-cache).Path
& C:/Users/lifei/.cargo/bin/cargo.exe --config 'source.crates-io.replace-with="task-official"' --config 'source.task-official.registry="sparse+http://127.0.0.1:18742/"' test --offline --locked --manifest-path .trellis/tasks/09-27-cb5-005-continuation-usage-crash-contract/harness/Cargo.toml usage_
& C:/Users/lifei/.cargo/bin/cargo.exe --config 'source.crates-io.replace-with="task-official"' --config 'source.task-official.registry="sparse+http://127.0.0.1:18742/"' build --offline --locked --manifest-path .trellis/tasks/09-27-cb5-005-continuation-usage-crash-contract/harness/Cargo.toml
& C:/Users/lifei/.cargo/bin/cargo.exe fmt --manifest-path .trellis/tasks/09-27-cb5-005-continuation-usage-crash-contract/harness/Cargo.toml -- --check
```

fake只运行`Python312/python.exe -B harness/fake_usage_peer.py`，不启动CodeBuddy。Stage A 16个tests被filter跳过，其Host证据不重跑。缓存离线复用，不添加依赖。

## Host orchestration timeout 后的唯一 fresh repair attempt

原 `usage` 已被 Host CommandRun 的默认30s外层timeout中止：`command-26872-1790479449915847-74` / `COMMAND_TIMEOUT`。`usage.host-timeout.json` 是基础设施证据，不是Provider Usage结果。原sentinel与所有旧证据永久保留，原attempt不重跑、不覆盖。

唯一新命令（仓库根目录）：

```powershell
& .\.trellis\tasks\09-27-cb5-005-continuation-usage-crash-contract\harness\target\debug\cb5-005-continuation.exe usage-repair
```

**父 Host 必须在 CommandRun/command layer 显式设置外层 timeout ≥300000ms，推荐420000ms以覆盖原scenario的240s+120s及cleanup余量。** 这是Host命令层设置，不是exe参数；不能继续使用默认30s。harness内部Usage协议逻辑、240s/120s预算与分析保持不变。

启动gate要求原 `usage.attempt-started.json` 是普通文件、`usage.result.json` 不存在、`usage.host-timeout.json` 的 `classification` 精确为 `HOST_COMMAND_TIMEOUT`。任何缺失、解析错误、路径链接或已有repair证据均fail-closed，启动前不创建Provider child。

gate通过后，以create_new+fsync创建 `usage-repair.attempt-started.json`，复用同一scenario创建全新temp root/new Session，不读取或恢复原未知Session。只输出 `usage-repair.result.json` 和 `usage-repair-analysis.json`，scenario label均为 `usage-repair`。原 `usage` mode与旧证据不变。无force/output/retry参数，不存在第三次repair模式；repair再次中断也必须停Host Gate，不删除sentinel或新增后缀重试。

repair真实CLI仍NOT_RUN。该修复只恢复Host执行窗口，不改变Provider Usage判定，也不进入Crash。
