# Material Contract Difference — 历史记录，RESOLVED_BY_HOST_DESIGN

以下为上一 Execution 发现的原始差异，保留事实和当时停止决定。Host 后续明确批准：继续使用官方 SDK 2.2.0，仅在 GuardedRead/Shared::incoming 已校验 envelope 的 exact pending `session/new` response 上捕获 models；单 Runtime 单未消费槽，以 typed sessionId 二次校验后消费，fail/shutdown 清空。此设计取代下文“不实施 raw capture”的历史停止边界，不升级 SDK、不自建请求 id/waiter/NDJSON client。

生产准备入口只读 Execution canonical workspace，默认无 desired mode；仅显式 policy 才发送本次目录 advertise 的 typed set。Generic Runtime 原有绑定路径会同时写 Dispatching，因此增加 CodeBuddy 专属 preparation OCC binding 入口，校验 Claim/provider/reservation/R1，保持 not_dispatched。Private R1/protocol/session 继续使用既有 typed mutation；不写 thread/turn，不改 schema。

Runtime 使用独立 ownership task 覆盖 launch await 的取消窗口，避免 cleanup 在 CreateProcess 尚未完成时误发 destroyed 证据。成功返回后 PreparedFreshSession 拥有 Runtime；drop 同步整 Job cleanup 并调度 CB6-005 evidence，显式 shutdown 等待 evidence。无法确认时保留 unknown/Claim，prepared rows 和 runtime attempt 均可供 startup 识别。现有 generic Dispatch 的后续衔接属于 CB7-003，不在本卡提前推进。

锁定 agent-client-protocol=2.2.0，Cargo.lock 解析 schema=1.9.1。schema::v1::NewSessionResponse 仅有 session_id、modes、config_options、meta，没有 models 或 flatten 扩展字段。serde 忽略未知 models，SDK request 成功并不能证明目录完整捕获。

现有 CB5-003 evidence/fresh-session.jsonl 的 read/write sequence=5 session/new 成功 response 均包含顶层 models（availableModels/currentModelId）、modes、configOptions。fixture 数量只用于证明本次回放内容，不作为产品常量。测试走当前 ManagedClient -> SDK typed NewSessionRequest -> fake peer response -> typed response；断言 exact session/modes/configOptions 保留，但 models 消失。

这是设计 §15.1 的 returned models/modes/configOptions 内存捕获与 pinned typed response 的差异。不能假定顶层 models 与 configOptions 中 model 永远等价，不能从一个目录推造另一个目录。v13 无 durable catalog 字段，不能偷塞 public/private identity 字段。

不实施 SDK 升级、自造 NDJSON、response wrapper/raw capture 或忽略 models 的兼容策略；这些需要先解决已报告契约差异。本轮未创建 PreparedFreshSession、未接生产 acceptance，保留原有 Runtime cleanup/recovery 和 capability。

排除误报：CB5-004 host_cancel_permission.rs 的 catalog sanitizer 主动移除 name；其精简 fixture 无法直接作为完整 typed catalog 输入，不证明 Host 实际缺少 name。CB5-003 success fixture 保留 names，复现测试使用该 fixture。

证据冲突：CB5-003 verification.md 仍描述较早 HTTP500 失败阶段，但当前 success wire 与 runner-result.json=PASS、read-attempt-1.json.protocolSucceeded=true 一致。以当前用户指定的 Host 真实 evidence 为准，不修改旧文件、不重跑 probe。
