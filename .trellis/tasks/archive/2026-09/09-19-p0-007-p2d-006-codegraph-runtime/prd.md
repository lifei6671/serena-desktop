# P0-007 与 P2D-006 CodeGraph RuntimeSlot

## Goal

在隔离临时工作区取得 CodeGraph 多进程隔离证据，并以该证据冻结策略和实现 RuntimeSlot 集成。

## Requirements

- 在两个全新临时 Workspace A/B 与隔离的用户配置目录中，实际验证 CodeGraph 1.6.0 的 `init --yes`、`status --json`、`serve --mcp --path` 多进程契约；不得写入真实业务 Workspace、索引或 Agent 配置。
- A/B 必须各自完成索引、Root identity 校验、独立真实 server 子进程启动、`tools/list` 含 `codegraph_explore`、并发 marker 查询，以及 stop/crash 后的相互隔离；记录启动/停止延迟及真实 server 子进程 steady RSS。
- 仅当 P0-007 全部通过时，依据实测证据冻结 CodeGraph runtimePolicy；无同进程并发证据时 `perSlotConcurrency=1`，`maxInstances` 与 `idleTimeout` 不得猜测。
- 将 CodeGraph 从 `stateless_command` 升级为 `workspace_scoped_process`，复用通用 `WorkspaceCapabilityManager` 的 RuntimeSlot、single-flight、容量、LRU、idle sweep 与 opaque-stop 合约；禁止第二套 manager 或旧 Global Binding。
- `start(lease)` 和每次 call 仅信任 `WorkspaceResolver -> WorkspaceLease` 与 `status --json` 的 Root/readiness；不得以 `.codegraph/codegraph.db` 推断可用，且 query/acquire 绝不隐式 init/sync/index。
- Remote 恢复与否必须服从权威设计文件的 P2D gate；若本任务未授权恢复，`codegraph_explore` 继续不 advertise、不 route。无论状态如何，禁止 ActiveWorkspace、Desktop selection、legacy session/global Binding 参与路由。
- 保留现有未提交 P2C、P2D Batch A/B 工作；不进入 P2D-007/008/009，不 commit/push。

## Acceptance Criteria

- [ ] P0-007 evidence 证明 A/B 独立 root、真实不同 server identity、并发 marker 隔离、stop/crash isolation、restart identity 变化及资源/时延数据；失败立即停止实现。
- [ ] runtimePolicy 的 `maxInstances`、`idleTimeout`、`perSlotConcurrency` 都有本次证据与明确理由。
- [ ] CodeGraph Provider、registry、manager routing 与 MCP schema/call（若 gate 允许）均只通过 Lease/RuntimeSlot，并覆盖异常与容量语义。
- [ ] 针对同 Workspace single-flight、A/B slot identity、capacity/LRU/BUSY、idle、crash recovery、readiness failure/no implicit prepare 的测试通过。
- [ ] 运行最小相关 Rust 测试、`cargo check --locked`、owned `rustfmt`、`git diff --check`；Linux 验证仅使用项目 Docker runner，若不可用如实 NOT_RUN。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
