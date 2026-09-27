# Design

在 CodeBuddy 私有边界创建单 Prompt typed mapper；只拥有 execution/session/conversation/request identity 的只读快照和有界 tool category map，输出既有 AgentActivityEvent。无 raw 公共字段、无 private Store mutation。忽略非白名单 typed variants；所有 mapping 在 exact identity 校验后发生。

与 collector 共享每批 frame，保留原有 result assembly 与 terminal transaction。publish 必须是可丢弃的附属工作：不延迟 request polling/timeout/cancellation，不在 terminal 之后继续发出 pending Activity；使用最小有界方案，不新增长驻后台任务。terminal/failure/caller drop 释放 mapper 和未完成 publish。

SDK 2.2.0 / schema 1.9.1 v1 保留 meta；ToolKind Other 有 serde(other)，未知 kind 安全为 Tool。DefaultOnError 会把非法 meta 转为 None，必须拒绝；不得将非法结构化身份默认为当前 identity。

所有公共 contracts、provider capabilities、Claim/Usage/terminal 写入保持原样。Windows 原生 fake peer 验证；无 project Docker runner，Linux UNAVAILABLE。
