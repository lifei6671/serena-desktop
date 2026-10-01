# Design

复用 ManagedClient / Shared / GuardedRead / GuardedWrite / Dispatcher 与原 Prompt owner。owner 校验 durable exact identity 后预注册窄单槽 permission context；active Prompt wire 与 tool_call 身份从同一 Shared 通道绑定，不造第二套 ACP parser。

Dispatcher 用官方 SDK typed request conversion/response，从实际 advertised 唯一 RejectOnce 选择 ID。非法输入关闭原 transport，交 owner cleanup。reply exact id 与物理 flush 复用 SDK responder/GuardedWrite；安全事件仅物理完成后交回 owner 有界队列。

context 在 terminal/failure/shutdown/drop 清理，不能跨 Runtime 复用。response deadline 和后续 terminal wait 有界；cancel deadline 独立。所有 SQLite/telemetry await 留在 owner 既有机制，不阻塞 Dispatcher。

沿用 exact terminal staging -> whole Job shutdown -> approved Runtime evidence -> atomic finalization；无可靠 terminal 走 reconciling/interrupted；证据缺失 Unknown + retained Claim。不新建 terminal/release authority。

先核对 pinned SDK 与 frozen wire；material mismatch 不降级自由 JSON。生产不投影 raw permission payload。

权限决策通知 `PermissionDenied` 保持 CodeBuddy 私有，仅原 owner 持有 identity。owner bounded publication 执行 exact Runtime/Session/Execution/Prompt Store 事务，同时保存固定诊断和显式 `provider.permission_denied` Activity current/history/revision；不再发普通 Provider Activity 立即覆盖拒绝。通用 `AgentActivityEvent` 与 telemetry projector 不改契约。Dispatcher 不执行这些 await；terminal/EOF/timeout 收尾保留有界交付。

Activity 使用既有 Provider/none pair 与持久化 summary 列，无 migration、新 phase 或 capability。`resolve_summary_code` 只允许此合法 pair 的明确拒绝摘要；Product 对未知/不一致存储代码仍 fail closed，finalizing/reconciling 优先。普通新 Activity 恢复既有摘要，历史保留拒绝。前端只增加封闭 summary 的固定中文标签，不依赖 Provider ID 或 diagnostic 文本。

同一 Shared 为 notification 分配单调序号，物理 deny flush 保存当前 cutoff；owner 等待已提交 publication 完成、清除旧队列、过滤 cutoff 及更早 Activity，防止迟到旧 ToolCall 覆盖拒绝。正文 collector 不受过滤影响；cutoff 之后新 Activity 正常处理。没有跨 Runtime registry 或额外后台任务。
