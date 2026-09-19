# P0-008 Codex Usage Wire Contract Evidence

## Goal

固定 pinned codex-cli 0.153.4 app-server 的真实 Usage wire、ordering、cumulative、terminal 与 checkpoint 证据，不修改生产运行时。

## Requirements

- 仅针对 discovery 等价路径定位到的真实 vendor `codex.exe`，固定绝对路径、版本字符串、SHA-256、大小和 mtime；版本非 `codex-cli 0.153.4` 即停止为 `DESIGN_BLOCKER`。
- 通过真实 `codex app-server` stdio JSONL 执行 initialize/initialized、至少两次 fresh turn、同 Thread continue、terminal/late（完成后至少监听 3 秒）、真实同步 checkpoint 探查，以及成本允许时 restart/resume。
- 保存按严格收发顺序的最小脱敏原始帧，保留 method、id、完整 JSON shape、数值和 Thread/Turn 关联字段。不得保存凭据或真实用户文本。
- 最终合同须把 vendor 实测或自描述事实与 Serena Phase 4 计划校验分开；不得自行相加或推导 `total_tokens`，不得将单次现象表述为跨版本保证。
- 全部任务产物仅位于本 Task 的 `research/`；不得修改 production/runtime/parser/store/UI、Cargo、schema，也不得 commit/push。

## Acceptance Criteria

- [ ] `research/binary-identity.txt` 固定 pinned binary identity，且 version 和 file SHA-256 分列。
- [ ] `research/wire-samples.jsonl` 包含每个 probe 的单调序号、时间、方向、method/id、脱敏 payload。
- [ ] `research/verification.md` 逐项回答 P0-008 的 14 个冻结问题，并把未知结果标为 UNKNOWN/UNPROVEN/NOT_RUN。
- [ ] `research/probe-matrix.md` 对 fresh、continue、terminal、late、checkpoint、restart 给出 PASS/FAIL/NOT_RUN 与证据位置。
- [ ] 结论为 PASS、DCR_REQUIRED 或 BLOCKED 之一，且没有越界生产修改。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
