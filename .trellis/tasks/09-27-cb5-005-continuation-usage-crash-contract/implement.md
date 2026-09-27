# Stage A 执行计划

1. 复用已启动任务与原基线，保留用户并发 .gitignore 修改。
2. official SDK typed continuation binary、manifest 与安全证据。
3. Windows fake/unit tests、build、fmt check；不运行真实 CodeBuddy，不用 WSL，不声称 Linux validation。
4. README 唯一 Host 命令与 NOT_RUN ledger。
5. 冻结 delivery target 并独立只读 review，修复阻断项后重新验证；停 Host Gate。

任务创建和启动已有用户授权。只提交本 task 文件；此处提交指交付文件，不进行 Git commit/stage。
