# Phase 6：双平台回归与人工验收

## Goal

建立 Windows/macOS 双平台自动化 Gate、macOS 12+ arm64 真机验收矩阵和用户文档，完成首版发布总验收。

## Requirements

- 依赖：Phase 5 macOS 打包与发布已完成并归档。
- Windows 执行完整质量与 NSIS Gate；macOS arm64 执行完整质量、DMG 和 ad-hoc 签名 Gate。
- macOS 12.0 或可信等价环境必须通过最低版本验收；若失败，只能提高最低版本并记录证据。
- 完成全新安装、首次启动、桌面生命周期、依赖发现、Serena、Codex Agent、Remote Access、权限、升级和数据保留真机矩阵。
- README 和状态页文档覆盖下载、架构、DMG 安装、Gatekeeper 手动允许、开发依赖和平台能力差异。

## Acceptance Criteria

- [ ] Windows 与 macOS arm64 自动化 Gate 全部通过，无 `continue-on-error` 或等价软失败。
- [ ] macOS 12.0+ arm64 真机验收矩阵完整记录并通过。
- [ ] Codex Agent、任务恢复、Remote Access 和系统通知达到首版功能对齐目标。
- [ ] 升级、删除应用和重新安装均保留用户数据。
- [ ] 已知的 macOS Runtime 恢复边界和常见故障排查入口已记录。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
