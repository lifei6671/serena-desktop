# P0-007 与 P2D-006 设计

## 决策闸门

P0-007 是实现的硬前置。只在隔离临时根目录与隔离 HOME/USERPROFILE/APPDATA/LOCALAPPDATA/XDG 环境运行真实 CodeGraph CLI。任何多进程、root 绑定、MCP query、crash isolation 或资源采集失败均记录 `DESIGN_BLOCKER` 并停止，不改生产代码。

## 运行时边界

CodeGraph 由 `WorkspaceCapabilityProvider` 创建和解释 opaque `CapabilityRuntimeHandle`。Handle 固定 `(providerId, workspaceId, generation, canonicalRoot)`；Manager 仅以其通用 RuntimeSlot 生命周期、容量和 in-flight guard 管理它，不检查 CodeGraph PID、port 或 root，也不 downcast。

`start(lease)` 先运行并解析 P2D-004 的 `status --json`，校验 complete/readiness 与 canonical root。成功后以官方 `codegraph serve --mcp --path <lease.canonical_root>` 创建绑定该 root 的 MCP transport/client。`call` 对 lease/runtime identity 二次校验，再由 Provider 调 `codegraph_explore`，保持既有 query/maxFiles 公共语义。

## 策略冻结

仅在 P0-007 显示 A/B 可同时存活且资源合理时设置 `maxInstances=2`。没有任意同一 server 并发安全证据，首版固定 `perSlotConcurrency=1`。`idleTimeout` 必须从启动、停止、RSS 结果推导，偏保守并写入 evidence；若不能给出可复核理由，视为 `DESIGN_BLOCKER`。

容量满时，Manager 仅驱逐安全 idle 且 `in_flight=0` 的 LRU Slot；若全 Slot 为 starting/stopping/in-flight，稳定返回 `CODEGRAPH_BUSY`，绝不 kill/retarget 其它 root。transport/crash 只污染所属 Slot，后续同 Workspace 可新建该 Slot，绝不借用其它 Workspace。

## Remote 决定

技术设计将 `codegraph_explore` 置于 Phase 2D Adapter 完成后的显式 workspace 路由 gate。实施前再次定位该 gate：若明确 P2D-006 可恢复，则 schema/advertise/call 同步走 `workspaceId -> Resolver -> Lease -> Manager`；若文本指定 P2D-009 后，保持 disabled 并测试拒绝路径。
