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
