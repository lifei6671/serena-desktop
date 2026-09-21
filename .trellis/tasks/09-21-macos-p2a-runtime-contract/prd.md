# Phase 2A：Runtime 与终止契约

## Goal

设计并实现 macOS Codex launcher、process group/session、启动令牌、取消终止和终止证据契约。

## Requirements

- 依赖：Phase 1 可编译基线已完成并归档。
- 使用固定 executable + argv 启动 Codex，不经过 shell command string；在 exec 前建立独立 process group/session。
- 保持 stdin/stdout/stderr 的最小继承集合，并验证 argv、cwd、空字节和超长参数。
- 定义绑定 PID/PGID 与启动时间或等价内核事实的 macOS 启动令牌，防止 PID 复用误认。
- 实现“中断请求、宽限等待、process group 终止”，并验证直接 child 和受管 process group 均退出。
- 终止证据绑定 Runtime ID、PID/PGID、启动令牌和观测时间；无法证明时保持 `unknown` 和 Workspace Claim。

## Acceptance Criteria

- [ ] macOS launcher、启动令牌、取消和完整进程组终止具有自动化测试。
- [ ] 启动期和运行期取消均不遗留受管 child/grandchild。
- [ ] PID 复用或证据不完整不能通过 Runtime 身份验证。
- [ ] 未确认终止不会提前释放 Workspace Claim。
- [ ] Windows Runtime 行为和错误码不回归。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
