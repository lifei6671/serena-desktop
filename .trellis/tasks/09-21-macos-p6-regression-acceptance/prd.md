# Phase 6：双平台回归与人工验收

## Goal

建立 Windows/macOS 双平台自动化 Gate、macOS 12+ Apple Silicon 真机验收矩阵和用户文档，完成“用户可以从 GitHub Release 下载、手工通过 Gatekeeper 后正常使用”的首版发布总验收。

## Requirements

- 依赖：Phase 5 macOS 打包与发布已完成并归档。
- Windows 执行完整质量与 NSIS Gate；macOS arm64 执行完整质量、DMG、ad-hoc 签名和 release verifier Gate。
- 首版只验收 Apple Silicon `arm64`；不得把未测试的 Intel/Rosetta 能力写入 Release 说明。
- macOS 12.0 或可信等价环境必须通过最低版本验收；若失败，只能提高最低版本并同步 Tauri 配置、README 和 Release 要求。
- 完成从浏览器/GitHub Release 下载 DMG、挂载、拖入 Applications、首次 Gatekeeper 手工允许、后续正常启动的真实安装链路。
- 完成桌面生命周期、依赖发现、Serena、Codex Agent、Remote Access、通知/声音/LAN 权限、文件系统、升级和数据保留真机矩阵。
- Codex 产品验收使用当前共享的 JSON Schema 必要子集兼容契约；version/hash 是 evidence，不再使用历史精确 allowlist 判断新版本能否执行。
- README 和状态页文档覆盖 macOS Apple Silicon 下载、DMG 安装、Gatekeeper 手工允许、开发依赖、平台能力差异和未公证说明。

## Acceptance Criteria

- [ ] Windows 与 macOS arm64 自动化 Gate 全部通过，无 `continue-on-error` 或等价软失败。
- [ ] macOS Release 只包含经过 verifier 验证的 arm64 DMG；Windows Release 继续包含既有 x64 NSIS。
- [ ] 从 GitHub Release/浏览器下载正式候选后，Gatekeeper 预期提示、手工“仍要打开”和后续重复启动流程完成真机记录。
- [ ] macOS 12.0+ arm64 真机验收矩阵完整记录并通过；若实际最低版本提高，所有配置与文档同步。
- [ ] Codex Agent start、正常完成、cancel、continue、应用崩溃与重启恢复通过真实产品路径；schema probe 通过不能替代 Runtime/Provider E2E。
- [ ] Serena 安装/启停/索引、CodeGraph、Workspace、Quick Tunnel、ngrok、自建 HTTPS、仅 MCP、系统通知、提示音、LAN 权限和文件选择达到首版功能范围。
- [ ] 升级、删除应用和重新安装均保留用户配置、Workspace Registry、Agent State、OAuth State 和日志。
- [ ] README/Release Notes 明确首版使用 ad-hoc 签名、未经过 Apple Developer ID 公证，并准确记录 Gatekeeper 手工允许步骤。
- [ ] 已知的 macOS Process Group 恢复边界与 Windows Job Object 差异、CLI 找不到、LAN 权限拒绝和残留进程排查入口已记录。
- [ ] Windows 现有行为、Runtime Safety Contract、质量 Gate 和 NSIS 发布链路不回归。

## Notes

- Developer ID、公证、staple、Mac App Store 和 Intel Mac 支持均不属于首版 Acceptance。
- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
