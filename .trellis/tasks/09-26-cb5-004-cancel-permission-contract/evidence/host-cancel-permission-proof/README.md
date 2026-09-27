# CB5-004 Host 真实合同 PASS（当前有效状态）

Fresh Session prerequisite、cancel-before、cancel-after、permission-deny 均有 Host 真实 wire 证据，**CB5-004 contract-test PASS**。CB5-003 Fresh Execute PASS 保持；CB5-005 未开始，其 CB5-004 依赖已满足，是否进入后续任务由 Host 决定。

- before：exact prompt sequence 6，correlated activity 且 manifest 为空后 cancel 29，exact prompt terminal 30 / `cancelled`，最终 delta=[]。
- after：真实 catalog 广告 auto，typed set_mode request 6 / ACK 10（通知穿插），prompt 11；磁盘 marker 27 bytes、SHA256 `613841732b16579f9417ffa2ffeb2a86dab63957b0c8381976403c6ee4960301` 后 cancel 43，terminal 47 / `cancelled`。marker 保留且为唯一 delta；cancel 不回滚。
- permission：default Always Ask；真实 request 91 / RPC id 0，exact session/tool identity，typed RejectOnce 选择广告 optionId `reject`，response 92；无 session/cancel，exact prompt terminal 96 / `cancelled`，deny 前后 manifest 均为空。deny 本身不是 terminal，本次 terminal 由 Provider 独立返回。
- 三个 fresh temp cwd 独立，direct child cleanup/reap、streams close 与 workspace delete 均成功。进程 cleanup exitCode=1 是 terminal 之后 owned-child termination 记录，不替代 ACP terminal，也不证明 Windows Job-at-creation/tree containment。

[验收](acceptance.json)、[合并 cancellation](cancellation.jsonl)、[permission wire](permission.jsonl)、[进程证据](process-evidence.json) 已逐项核对 exact identity/sequence/manifest/hash，而非仅采用 runner PASS。permission options 是 CodeBuddy 2.158.0 本次观察，不能跨版本写死白名单；生产始终按 typed kind + advertised ID 决策。`canCancel` 还需 CB8-001 implementation PASS 才可 advertise。Cancel/deny 不授权 Claim release；Phase 6/7 Runtime/Claim 冻结边界不变。

本次收口仅核对现有 evidence 与文档；真实调用 0，harness/产品源码无修改，无commit。旧 raw evidence/hash/ready review-target 保留。独立 review 与最终 Host Gate 由父会话收口；不进入 CB5-005。

---

## 历史 harness-ready 报告（superseded，命令不得再次执行）

# Host cancel / permission proof harness

状态：HARNESS_READY；本实施 Agent 未运行任何真实 CodeBuddy 命令。Fresh session/new 的前置条件采用 Host 已证明 PASS 事实；先前 nested HTTP500/EOF/EPERM 仅为保留历史。三个合同场景目前均 NOT_RUN，必须由 Host 单次执行并依据新证据判定。

新增 binary 使用 official SDK 2.2.0 external streams。Initialize 单独用 SDK 公开 `UntypedMessage` 发送 Host 指定 exact shape，再 typed 解析 InitializeResponse（SDK InitializeRequest 默认会多序列化 fs/terminal/auth）；new/mode/prompt/cancel/permission 均使用官方 typed API。Fake peer 检查实际收到的 initialize 参数精确相等，early config_option_update 在 new response 前穿插。

固定真实 child：`C:\nvm4w\nodejs\node.exe` + `C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy` + `--acp`。没有建会话前 mode/tools/settings。每场景独立普通 Win32 temp cwd，隐藏项完整 manifest，symlink/reparse fail closed；证据只写 manifest size/SHA256，无文件内容、prompt、工具参数或 stderr 正文。

## Host 命令

编译产物已存在。以下三个命令分别调用一次；失败、EOF、timeout 或已有 sentinel 都不得重试。runner 的原子 create-new + fsync sentinel 在任何启动前写入；无 force 开关。不要直接绕过 runner 调用 binary。

```powershell
Set-Location 'E:\wx_lifeilin\github.com\lifei6671\serena-desktop'
& 'C:\Users\lifei\AppData\Local\Programs\Python\Python312\python.exe' 'E:\wx_lifeilin\github.com\lifei6671\serena-desktop\.trellis\tasks\09-26-cb5-004-cancel-permission-contract\run_host_cancel_permission.py' before
```

```powershell
Set-Location 'E:\wx_lifeilin\github.com\lifei6671\serena-desktop'
& 'C:\Users\lifei\AppData\Local\Programs\Python\Python312\python.exe' 'E:\wx_lifeilin\github.com\lifei6671\serena-desktop\.trellis\tasks\09-26-cb5-004-cancel-permission-contract\run_host_cancel_permission.py' after
```

```powershell
Set-Location 'E:\wx_lifeilin\github.com\lifei6671\serena-desktop'
& 'C:\Users\lifei\AppData\Local\Programs\Python\Python312\python.exe' 'E:\wx_lifeilin\github.com\lifei6671\serena-desktop\.trellis\tasks\09-26-cb5-004-cancel-permission-contract\run_host_cancel_permission.py' permission
```

结果在本目录，逐场景保存 result、sanitized JSONL、process evidence 与持久 identity。permission-options 从真实 request wire 摘取；缺少拒绝选项仍保留真实目录。原 task/evidence 文件一律不覆盖。

before 仅 exact prompt 已写入、correlated activity、零 manifest delta 时 cancel；最终零 delta 才 PASS。after 仅实际 catalog 广告 auto 才 typed set_mode，ACK 后 prompt；磁盘 marker exact bytes+SHA256 才 cancel。permission 仅 advertised typed RejectOnce，按 active session + 已观察 toolCallId 校验，one-shot responder；无拒绝选项则 fail closed/PARTIAL。

没有 exact prompt terminal 时不生成 terminal；post-action 20s timeout 走 owned direct child bounded cleanup，整体150s。runner 外层180s期限，仅异常时 taskkill /T /F（10s）并 wait（5s）。不声称 Windows Job-at-creation 或任意后代进程 containment 已证明。取消不回滚，marker 保存至证据捕获后删除整个 temp root。Cancel/deny 自身不授权 Claim release。

Host 完成后需要把两个 cancellation JSONL 汇总为 cancellation.jsonl，把三个 process evidence 汇总为 process-evidence.json，并对三项真实验收分别判定。停止 Host Gate，不进入 CB5-005。
