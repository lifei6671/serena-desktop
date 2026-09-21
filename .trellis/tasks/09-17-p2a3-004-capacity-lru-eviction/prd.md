# P2A3-004 Capacity Accounting 与 LRU Eviction

## Goal

实现 Workspace Capability Runtime Manager 的 per-Provider 容量驱动零 in-flight LRU 驱逐。

## Requirements

- Per `providerId`，按 Descriptor 的 `max_instances` 计算 `starting + ready + stopping` 的已分配容量；不得跨 Provider 计数。
- 容量已满时，只能选择 `in_flight == 0`、没有待完成 acquire、且处于 `Ready` 的同 Provider Slot；按单调使用序号选择最久未使用者。
- 在 Manager 的同步管理边界内完成 victim 选择、将其设为 `Stopping`、移出可 acquire 状态并把唯一 Runtime handle 交给 stop 流程；Provider `stop` 不得持有该全局管理锁。
- 仅当 Provider 返回 `Stopped` 证据后，才释放 victim 容量并为原请求启动 replacement。stop failure 必须返还同一 handle，使 victim 恢复 `Ready` 并继续占用容量，且不得启动 replacement。
- 容量满且没有可驱逐候选时，返回既有 `WORKSPACE_CAPABILITY_BUSY`，不产生 stop/start 副作用。
- 不实现 idle timeout 或后台 sweeper，不接入 Remove/Host shutdown，不提交 Git。

## Acceptance Criteria

- [ ] idle LRU victim 被停止后，replacement 启动且容量仍受限。
- [ ] busy LRU 候选被跳过；全 busy 时返回 Busy 且没有 stop/start 副作用。
- [ ] stop failure 后原 Slot 保留可用 handle 和容量，replacement 不启动。
- [ ] `Stopping` Slot 的 acquire 返回 Busy，且并发 acquire 不能在 victim 选中后重新进入。
- [ ] LRU 顺序在测试中由单调使用序号确定。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
