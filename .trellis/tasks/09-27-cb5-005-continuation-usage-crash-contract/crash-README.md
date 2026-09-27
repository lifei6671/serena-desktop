# Stage C — Crash Host Gate freeze

Stage A/B/C Host Gate 均 **PASS**，CB5-005 Contract Freeze 可以完成。Stage C 两个真实 mode 已由父 Host 各执行一次并消费；禁止重跑 Stage A/B 的 resume/load/usage/usage-repair，也禁止重跑 crash-before/crash-after 或任何真实 CodeBuddy CLI。冻结结论见 [crash-host-freeze.md](crash-host-freeze.md)。

本次 PASS 只冻结已观测的 Crash Result Recovery 合同：两项 `resultCompleteness` 均为 `partial`。R2 load/result recovery 不能证明 R1 Runtime termination，不能授权 Claim release；生产 Runtime/Windows Job/StateStore 尚未实现。

## 已消费的父 Host 执行（历史记录，禁止重跑）

父 Host 在仓库 ordinary Win32 cwd、Windows 标准用户环境依次完成两个独立 fresh temp Workspace/new Session 场景：

- before：CommandRun `command-26872-1790483578322192-95`，completed / exit0。
- after：CommandRun `command-26872-1790483608562452-96`，completed / exit0。

以下命令仅用于标识已经消费的执行，不是待执行步骤：

```powershell
& .\.trellis\tasks\09-27-cb5-005-continuation-usage-crash-contract\harness\target\debug\cb5-005-continuation.exe crash-before-terminal
```

```powershell
& .\.trellis\tasks\09-27-cb5-005-continuation-usage-crash-contract\harness\target\debug\cb5-005-continuation.exe crash-after-terminal
```

执行时每 mode 使用 R1 总预算120s、R2 总预算120s，两个 child 各 kill/wait≤5s、stderr join/abort≤2s；异步合计上界254s，另有本地 manifest/证据 IO。CommandRun 的 timeout 是父 Host 参数，不是 exe 参数；sentinel 已消费，不允许重跑或追加 repair。

direct installed launcher固定 `C:\nvm4w\nodejs\node.exe` + `C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy` + `--acp`，继承Host env，不保存完整env/stderr/credentials。不加multitask、不配置auto、不发cancel、不请求工具。permission只RejectOnce，否则拒绝响应。

## 流程与固定证据

before：R1 initialize/new，write-ahead identity，发read-only/no-tool Prompt；只在当前requestId correlated activity且尚无exact prompt terminal时停止task-owned direct child。terminal先到→NOT_OBSERVED，不重试。after：等待exact RPC + exact conversationRequestId + end_turn，立即kill/reap，再从内存生成live answer摘要；不写任何生产StateStore，诊断hash不等于真实Host crash后还能取得oracle。

R2只initialize/typed session/load exact S1/same cwd，历史早帧及response后250ms采样。**没有P2，无load→resume或resume→load fallback**。这是Result Recovery inspection，不是普通Continue（Continue仍session/resume）。250ms之外的late/无限历史完整性不作保证。

固定 `evidence/crash/` 下每mode三文件：`<mode>.attempt-started.json`、`<mode>.prompt-identity.json`、`<mode>.result.json`。sentinel和identity create_new+fsync；任何既有产物拒绝新attempt。不要移动binary/换编译根目录/删除证据绕过。缺result意味着未知，不自动重试。

identity文件只保存session及harness生成的conversation/request identity、prepared状态；SDK自动分配的prompt RPC ID在发出后由最终摘要记录，发送前为null。history只保存type/order、requestId equality/hash、messageId equality/hash、answer SHA256/UTF-8 length。全部prompt、answer/thought/raw meta只存在有界内存/pipe。

replay文本按messageId首次出现顺序分组，相同message同文本片段去重，不同片段按序拼接。这只是诊断重组规则，不能证明模型call/result覆盖；如果重复文本片段本来合法，该规则可能保守地产生hash不匹配。after要求messageId集合及answer hash/length与live一致，否则PARTIAL。

## 结果解释与安全边界

- **PASS / exit0**：目标窗口被观测，协议/身份检查通过，R2历史可绑定（after还要求答案/IDs一致），两runtime cleanup及workspaceDelta=[]；`resultCompleteness`依然最多partial。它构成 Stage C Contract Freeze Host Gate PASS，**不是 business completed 或生产 Crash Runtime 验收**。
- **PARTIAL / exit2**：窗口超时、协议/身份/load失败、历史缺失或文本/ID不匹配、tool activity、workspace/cleanup失败，或出现Material Contract Difference。无可绑定history→resultCompleteness=unknown。
- **NOT_OBSERVED / exit2**：before场景terminal先到，未命中窗口。跳过R2，不重试；不代表Provider故障。
- session/load不预设能恢复旧PromptResponse。缺exact旧terminal时，即使answer exact match也仅partial。若未来收到明确target terminal候选，仅记录Material Contract Difference，不升级completed。
- 始终 `windowsJobAtCreationProven=false`、`runtimeTerminationEvidenceProven=false`、`claimReleasePermitted=false`。本harness不证明Windows Job/tree containment。生产R1 Job evidence缺失→unknown + Claim retained；另行有R1 termination evidence时可按§23收敛interrupted。本卡不授权释放Claim。

复用约4MiB总wire、1MiB/frame、每方向2048 frames；manifest含hidden，≤4096项/32MiB，symlink/reparse fail-closed。只删除已检查的空root，异常内容保留Host检查，无递归删除。read-only是Prompt/permission约束，不是OS sandbox；workspace delta必须0。

## 既有 Fake 验证（不会启动真实 Provider，无需重跑）

以下命令是 Stage C harness 独立 review 前已完成的历史验证记录；本次 Host freeze 没有重新执行。结果见 [crash-verification.md](crash-verification.md)。

```powershell
$env:CARGO_HOME=(Resolve-Path .trellis/tasks/09-27-cb5-005-continuation-usage-crash-contract/.cargo-cache).Path
& C:/Users/lifei/.cargo/bin/cargo.exe --config 'source.crates-io.replace-with="task-official"' --config 'source.task-official.registry="sparse+http://127.0.0.1:18742/"' test --offline --locked --manifest-path .trellis/tasks/09-27-cb5-005-continuation-usage-crash-contract/harness/Cargo.toml
& C:/Users/lifei/.cargo/bin/cargo.exe --config 'source.crates-io.replace-with="task-official"' --config 'source.task-official.registry="sparse+http://127.0.0.1:18742/"' build --offline --locked --manifest-path .trellis/tasks/09-27-cb5-005-continuation-usage-crash-contract/harness/Cargo.toml
& C:/Users/lifei/.cargo/bin/cargo.exe fmt --manifest-path .trellis/tasks/09-27-cb5-005-continuation-usage-crash-contract/harness/Cargo.toml -- --check
```

新fixture只调用Python312 `-B harness/fake_crash_peer.py`；既有34项fake回归只验证共享基础设施/main dispatch兼容，不是重跑Stage A/B真实mode。Windows-only，无WSL/Linux PASS声明。DCR见 [codebuddy-private-state-dcr.md](codebuddy-private-state-dcr.md)。
