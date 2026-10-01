# CB10-001 Automated Full Regression Matrix

## Goal

在 CB10-002 人工验收前执行完整自动化 Gate，确认 Phase 1–9 引入的 Multi-Agent / CodeBuddy 变化没有新增机械回归。

## Authority

- `docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md` CB10-001。
- `docs/technical-design-multi-agent-provider-codebuddy-v0.1.md` §31、§32。
- 用户给出的 A–H 验证矩阵与失败分类规则。
- Phase 1–9 已提交历史；baseline HEAD `927f7b90eaa3e0a3c3f393132ca54100077e2a7b`。
- Host review 批准在同一 Work 内最小修复 CB6-005 provider-scoped Claim selection ordering。

## Acceptance

- 新增回归为 0。
- 每条命令记录 exit code、实际测试数、pass/fail/ignored/0-test、分类与 owner。
- §31 的 19 条安全不变量逐条映射到实际执行证据。
- 所有非 PASS 均保留最小原始证据并归类。
- 测试结束后只保留本任务 task-local evidence；无 secrets、大二进制、临时 DB 或临时 workspace。

## Repair boundary

- `workspace_claims` 始终是 startup recovery 扫描 authority，必须先加载 Execution 再判断 Provider。
- 悬空 Claim 对 generic、Codex scoped、CodeBuddy scoped 均 fail closed。
- 合法其他 Provider Claim 在任何 mutation 前只读 skip；自身 Claim 继续复用 generic classification。

## Non-goals

- 不为通过 Gate 修改能力边界、设计、schema 或 migration。
- 不修改 Recovery 状态机、Runtime、Claim release、schema/migration、CodeBuddy contract、Usage 或前端生产代码。
- 不执行真实 destructive provider 调用。
- 不进入 CB10-002，不 commit，不 push。

## Known expected behavior

CodeBuddy Usage 首版 `SKIPPED_UNSUPPORTED` / `tokenUsage=false` 是预期，不属于回归。
