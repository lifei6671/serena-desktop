# macOS 平台适配

## Goal

按照 docs/macos-porting-checklist.md 完成 Phase 0 决策，并协调 Phase 1 至 Phase 6 的可独立验收子任务，在不降低 Windows 版本质量的前提下交付功能完整、可安装、可验收的 macOS 版本。首版使用 ad-hoc 签名 DMG，Developer ID 签名和公证延期。

## Requirements

- 以 `docs/macos-porting-checklist.md` 为权威实施清单。
- 保留 React/Tauri 产品层及 MCP/Agent 业务契约，将平台相关 Runtime、进程生命周期、CLI 发现、桌面集成和发布流程收敛为明确的平台实现。
- 不降低已经验收的 Windows 版本质量、错误码语义和发布 Gate。
- macOS Runtime 使用平台真实可验证的进程证据；不能证明原 Runtime 身份时保持 fail-closed，不模拟 Windows Named Job 证据。
- Codex 兼容性准入以目标平台/架构预检和 SerenaDesktop 实际依赖的 App Server JSON Schema 必要子集为准；Codex version、binary SHA-256 与完整 schema SHA-256 继续作为身份、诊断和回归证据，但不作为精确白名单准入条件。正式 Runtime 的 initialize、Provider lifecycle 和 termination evidence 仍必须独立通过。
- 首版 GitHub Release 使用 ad-hoc 签名 DMG，允许 Gatekeeper 显示未验证提示；安装文档必须准确说明用户需要在“隐私与安全性”中手动允许。
- 首版不要求 Developer ID 签名、公证或 staple，且不得将产物描述为 Apple 已验证；相关正式分发能力延期至取得 Apple Developer Program 账户后的独立任务。
- 实施前完成 Phase 0，记录功能范围、CPU 架构、最低系统版本、分发渠道和签名责任。
- Phase 1 至 Phase 6 拆为可独立规划、实施和验收的 Trellis 子任务，按清单依赖顺序推进。

## Phase 0 Decisions

- 首版功能范围：与 Windows 功能对齐，完整包含 Codex Agent、任务恢复、Remote Access 和系统通知；允许按 Phase 分阶段实现，但发布前必须全部通过验收。
- 分发渠道：GitHub Release + DMG，不进入 Mac App Store。
- CPU 架构与产物策略：首版仅支持 Apple Silicon `arm64`，发布独立的 `aarch64-apple-darwin` DMG；Intel `x86_64` 和 Universal Binary 不在首版范围。
- 最低 macOS 版本：首版目标为 macOS 12.0；必须在该版本或等价可信环境完成验收，若依赖或真机结果不支持，只能提高版本并记录证据。
- 签名责任：首版由发布流水线生成 ad-hoc 签名产物；当前无 Apple Developer Program 账户，Developer ID 签名、公证和 staple 明确延期。
- Codex 兼容性：Windows/macOS 共用必要 JSON Schema 子集兼容判定；version/hash 只作为 evidence，不阻止 schema-compatible 的后续版本。历史 `0.153.4` 精确 hash 继续保留为已验证证据，不再定义全局准入白名单。

## Acceptance Criteria

- [x] Phase 0 的功能范围、CPU 架构、最低系统版本、分发渠道和签名责任已明确记录。
- [x] Phase 1 至 Phase 6 均建立可独立验收的 Trellis 子任务，并记录依赖和退出条件。
- [x] 当前父任务与 `docs/macos-porting-checklist.md` 已统一 ad-hoc DMG 分发契约和 Codex schema-subset 兼容契约；历史归档任务保留当时证据，不作为当前准入 Authority。
- [ ] macOS 构建、Runtime、CLI/进程树、桌面集成、发布和真机验收均满足经 Phase 0 修订后的清单退出条件。
- [ ] 首版 GitHub Release 产出 ad-hoc 签名 DMG，安装说明披露 Gatekeeper 未验证状态和手动允许步骤。
- [ ] 首版发布流程不要求 Developer ID、公证或 staple，也不声称 Apple 已验证。
- [ ] Windows 现有行为、测试和发布 Gate 不回归。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
