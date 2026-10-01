# 执行与验证

1. 读取 authority、launcher、private store、generic Claim/finalization、Provider/TaskManager 构造与恢复路径。
2. 实现 typed runtime persistence + sealed evidence + Win32 Job recovery；维护 recovery matrix。
3. 接入 CodeBuddy scoped startup reconcile 与 recovery authority；保留 generic outcomes、disabled/discovery 行为。
4. 补充 focused native Windows fake Job/process tree、failure matrix 与真实 TaskManager tests；不运行真实 CodeBuddy/CB5 probe。
5. 执行 focused CodeBuddy runtime/recovery/provider/TaskManager tests、Codex Windows recovery regressions、fmt/check/clippy。命令以项目已有 evidence 为准。Linux 如需要仅用项目 Docker runner，禁止 WSL。
6. Gate evidence 满足后 canRecover=true，其余五项保持 false；重新验证受影响断言。
7. 记录完整 diff inventory/hash、验证结果与局限；独立只读 full review；必要修复后再验证/重审。

交付不 commit/push、不归档推进下一任务。所有新增函数与核心逻辑附中文注释。
