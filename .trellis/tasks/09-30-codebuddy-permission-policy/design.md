# 设计

PermissionContext 持有来自已核验 execution row 的 WorkspaceLease 与 mode，不查询 Registry。保存 typed ToolCallUpdateFields，仅 initial 注册 id，partial update 合并，terminal 删除；count/bytes 有界，raw input 不投影。

独立 permission_policy 仅给 AutoAllowOnce/RejectOnce。dispatcher 验证所有 option IDs，全程按 typed kind 选唯一 AllowOnce；不能 allow 时选唯一 RejectOnce，否则 PermissionOptions。pending 保存真实拒绝标志，只有 reject flush 发送 PermissionDenied，原 terminal authority 保持。

路径复用 WorkspacePathResolver canonical nearest-existing 与平台 component identity。命令仅有限语法与 allowlist。这是体验审批层，不能限制已批准命令实际访问，长期需要 OS Sandbox Layer。
