# Phase 5：macOS 打包与发布

## Goal

为 arm64 创建 macOS 平台配置、ad-hoc 签名 DMG、GitHub Actions 发布链路和产物验证，不改变 Windows NSIS 契约。

## Requirements

- 依赖：Phase 4 macOS 桌面集成已完成并归档。
- 使用 macOS 平台配置输出 Apple Silicon `arm64` `.app` 和 `.dmg`，最低系统版本为 macOS 12.0，不改变 Windows `nsis` 配置。
- 首版使用 Tauri ad-hoc 签名，保留 Hardened Runtime；不要求 Developer ID、公证或 staple，也不声称 Apple 已验证。
- GitHub Actions 增加 macOS arm64 CI/Release Gate，并保持 tag-only、版本一致、`--locked` 和禁止 soft-fail 契约。
- 新增 macOS 产物验证，检查唯一性、版本、架构、大小、SHA-256、bundle identifier、DMG 内容和 ad-hoc 签名完整性。
- 安装说明准确披露 Gatekeeper 未验证状态及手动允许步骤。

## Acceptance Criteria

- [ ] GitHub Release 稳定产出并验证唯一的 arm64 ad-hoc 签名 DMG。
- [ ] DMG 只包含 Serena Desktop.app 和 Applications 安装入口，bundle identifier 保持不变。
- [ ] 浏览器下载后的 Gatekeeper 手动允许流程通过真机验证并写入文档。
- [ ] Windows NSIS 构建、验证和发布契约不回归。
- [ ] 替换或删除 `.app` 不删除用户配置、任务数据库和日志。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
