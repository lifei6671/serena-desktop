# 设计

保留 ACP client/protocol/fresh/continued/prompt/execute/result recovery。平台 adapter 提供 launcher request、runtime 生命周期和持久化 containment。复用 Codex Darwin 进程身份与 process-group 机制，不复制业务 pipeline；Windows Job launcher 不改行为。macOS evidence 仅在旧 runtime 的身份与绑定校验后生成。discovery 不启动进程，受管 catalog 不创建 Execution/Claim。

## 实施后决策

共享 Codex core 增加不写 Codex Store 的 external owner 入口；CodeBuddy 自己负责 durable row 和 sealed proof。失败 ownership 保留到 Workspace quarantine，避免临时 catalog 在清理失败时丢失 child。跨 Host SIGTERM 后 ESRCH 仅进入有界观察，不再升级信号。

Spec review：仓库目前只有 frontend spec，本次不改 UI。平台契约与真实验证边界记录在 `docs/codebuddy-macos-validation.md`，不扩充无关 frontend 规范。
