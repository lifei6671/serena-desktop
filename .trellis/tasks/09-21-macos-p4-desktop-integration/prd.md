# Phase 4：macOS 桌面集成

## Goal

完成 macOS 打开命令、Dock/菜单栏/退出生命周期、通知、声音、LAN 权限、文件系统和平台化 UI 文案。

## Requirements

- 依赖：Phase 3 CLI 发现和进程树所有权契约已完成并归档。
- 完成 `/usr/bin/open`、Dock reopen、红色关闭、隐藏、`Cmd+Q`、菜单栏退出、single-instance 唤醒和 LaunchAgent 生命周期。
- `Cmd+Q` 与菜单栏退出均等待既有 shutdown 流程，不绕过受管进程回收。
- 验证通知权限、通知点击激活和真实声音提示；若无法实现声音，则仅在 macOS 隐藏开关。
- 增加本地网络用途说明，验证 LAN 权限与常见 Workspace 位置；首版不启用 App Sandbox。
- UI 与文案遵守 `docs/ui/DESIGN.md`，完成 Windows 专属术语和快捷键提示的平台化。

## Acceptance Criteria

- [ ] Finder、Dock、菜单栏、LaunchAgent、single-instance 和退出语义通过真机验收。
- [ ] 通知、声音和 LAN 权限的允许、拒绝及恢复路径可验证。
- [ ] 桌面、文稿、下载、iCloud Drive 和外置卷 Workspace 能被后续工具使用。
- [ ] macOS UI 文案和快捷键正确，现有信息密度与色彩系统不回归。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
