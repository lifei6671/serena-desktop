# P3-003 Activity Store Update 与 History Query

## Goal

让 `executions` 成为唯一的当前 Activity authority：语义变化原子地更新
current snapshot、递增 sequence、写入一条 history；同语义 heartbeat 仅单调刷新
`last_activity_at`。同时提供有界、内部使用的 history 查询。

## Requirements

- Activity Revision v2 固定为 SHA-256 JSON array：
  `["agent-activity-v2", executionId, activityPhase, toolCategory, summaryCode]`；
  仅这五项参与 hash。
- Provider Activity 在 identity/claim 校验、非终态和 fault injection 检查之后，依当前
  persisted lifecycle 计算 `ProgressPhase` 与 `derive_summary_code`，比较完整 semantic tuple。
- 同 tuple 是 heartbeat：只以单调方式更新 `last_activity_at`，不改 sequence、history、
  `executions.revision` 或 `updated_at`。不同 tuple 必须在同一 IMMEDIATE transaction
  更新 current、sequence 并 append 一条 history。
- `transition_execution` 的 lifecycle CAS 成功时，若 progress priority 造成 summary
  semantic tuple 变化，也必须在同一 transaction 更新 Activity current/history；其 lifecycle
  revision 仍按既有规则恰好递增一次。
- history 查询必须有内部 typed exclusive sequence cursor、ASC 排序、server-enforced 上限与
  永远带 LIMIT 的 SQL；不设计或公开 MCP wire。
- 原子失败、非法 Activity、identity mismatch 与 observability injection 不得改变 current、
  history 或 lifecycle revision；终态后 Provider Activity 仍 no-op。

## Acceptance Criteria

- [ ] 首个和连续 semantic Activity 的 sequence/history 精确对应，且 Activity-only 从不增加
      `executions.revision`。
- [ ] heartbeat（包括迟到 heartbeat）只单调前进 `last_activity_at`；不会写 history 或
      `updated_at`。
- [ ] Finalizing、Reconciling 及离开 override phase 的 lifecycle transition 正确产生或不产生
      Activity history，且同一 transaction 内无 crash window。
- [ ] 并发 Activity 与 lifecycle transition 无 revision conflict、无丢失/重复 history，最终
      snapshot 与 sequence 一致。
- [ ] 分页具有有界 limit、exclusive cursor、稳定 ASC、next cursor 和空页行为。
- [ ] 所有用户列出的 focused transaction、Telemetry、P3-001/P3-002 回归与静态检查通过，
      或明确报告环境限制。

## Out of Scope

- P3-004 Observe 的 `knownActivityRevision`、wake 及 Product DTO 行为；MCP schema、UI、
  Work Adapter、Usage、history prune、Provider command 执行、Claim/lifecycle 业务重构、commit、push。
