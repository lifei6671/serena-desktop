# Phase 3：CLI 发现与进程树

## Goal

完成 Finder/LaunchAgent 环境下的 Codex、Git、uv、Serena 发现与安装，并统一受管进程组生命周期。

## Requirements

- 依赖：Phase 2B Runtime 恢复与 State Store 集成已完成并归档。
- 在 Finder 和 LaunchAgent 环境中按固定顺序发现 Codex、Git、uv、Serena，不假设继承 shell dotfiles 的 `$PATH`。
- 验证常规文件、Unix execute bit、CPU 架构和版本兼容性，并返回可区分且不泄露敏感环境的诊断。
- 支持直接安装的 Codex 及 npm Darwin vendor binary，不通过 npm shell shim 启动 Runtime。
- 冻结 macOS uv 获取、架构选择和摘要验证，移除 `winget`、`LOCALAPPDATA`、`uv.exe` 依赖。
- Serena、Workspace Runtime 和 cloudflared 使用独立 process group，并只回收应用拥有的进程树。

## Acceptance Criteria

- [ ] Terminal、Finder 和登录项启动均能稳定发现依赖或给出可操作诊断。
- [ ] 覆盖含空格、中文和外置卷的安装及 Workspace 路径。
- [ ] 超时、取消、应用退出和启动失败均完整回收受管进程树。
- [ ] 停止操作不会影响用户在 Terminal 中自行启动的相关进程。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
