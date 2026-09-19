# P2D-009 设计

## Authority

Remote DTO 只携带 `workspaceId`、`query` 与可选 `maxFiles`。dispatcher 解析 request 的 `workspaceId`，经 `WorkspaceResolver` 创建 Lease，并把 Lease 交给 `WorkspaceCapabilityManager::call_tool`；Manager 只做 provider-agnostic slot orchestration，CodeGraph Adapter/Provider 负责 readiness、runtime 和 public output compatibility。不会从 UI、session 或 legacy global state 补全身份。

## Public compatibility

registry advertise 与 transport route 同步恢复 `codegraph_explore`。Provider 内部错误由 CodeGraph Adapter/Broker compatibility boundary 映射为稳定 public code；runtime handle、PID、root 与 raw stderr 不越过该边界。Remote Source Write 仍不公开。

## Integration gate shape

使用 existing fake providers、barrier、channel 与 permit counter 构造可重复 A/B trace。测试分别断言 Lease canonical root、slot identity、call provenance 与 stop ownership。并发证明不使用 sleep：同 workspace 证明 single-flight，不同 workspace 在两 slot 容量下并行；满载时第三 workspace 得到 `CODEGRAPH_BUSY`。readiness query 不触发 init/sync/index。

## Lifecycle

Manager 继续按统一 `(providerId, workspaceId, generation)` Slot 管理容量与 LRU。仅 idle + zero in-flight Slot 可被驱逐或 idle stop；remove 先关 acquire/claim 并只收敛目标 Workspace；crash 只污染所属 Slot。shutdown 通过 Manager/Provider stop，CodeGraph child 延续 `CODEGRAPH_NO_DAEMON=1`。
