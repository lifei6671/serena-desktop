# Phase 2B：State Store 与恢复集成

## Goal

在已冻结的 macOS Runtime 证据契约上完成数据库迁移、恢复观察、Claim 释放规则和端到端故障场景。

## Requirements

- 依赖：Phase 2A 的 macOS Runtime 身份和终止证据契约已冻结并归档。
- 新增 migration 表达 Runtime 平台、containment 类型和平台证据，同时保留 Windows Job 字段及历史语义。
- 按 containment 类型验证约束，增加 macOS 终止证据并更新 Claim 释放查询。
- 使用真实旧库 fixture 验证升级，migration 失败时旧库保持完整。
- 实现应用崩溃或强杀后的恢复观察；无法证明原 Runtime 身份时进入人工收口，不误杀无关进程。
- 人工解锁只使用已有 Local Human Authority 入口。

## Acceptance Criteria

- [ ] Windows 旧库 fixture 能完整升级，失败注入不产生部分迁移。
- [ ] 正常完成、运行期取消、Codex 崩溃、应用强杀重启和 PID 复用场景均有自动化覆盖。
- [ ] 终止证据不完整时状态保持 fail-closed，Claim 不自动释放。
- [ ] macOS 与 Windows 的恢复能力差异有明确文档和稳定状态表示。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
