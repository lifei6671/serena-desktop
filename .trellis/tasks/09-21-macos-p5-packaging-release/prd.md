# Phase 5：macOS 打包与发布

## Goal

为 Apple Silicon `arm64` 建立可公开下载的 macOS Release：生成 ad-hoc 签名 DMG、macOS CI/Release 链路和可重复的产物验证，同时保持 Windows NSIS 契约不变。

## Requirements

- 依赖：Phase 4 macOS 桌面集成已完成并归档。
- 使用 macOS 平台配置输出 Apple Silicon `arm64` `.app` 和 `.dmg`，最低系统版本目标为 macOS 12.0；首版不构建、不发布 x86_64 或 Universal 产物。
- 首版使用 Tauri ad-hoc 签名并保留 Hardened Runtime；不要求 Developer ID、公证或 staple，也不得将产物描述为 Apple 已验证。
- GitHub Actions 增加 macOS arm64 CI/Release Gate，并保持 tag-only、版本一致、`--locked` 和禁止 soft-fail 契约。
- Release 拆分 Windows x64 NSIS、macOS arm64 DMG 和统一发布阶段；任一平台 Gate 失败均不得发布对应产物。
- 新增 macOS release verifier，检查 DMG 唯一性、版本、arm64 架构、文件大小、SHA-256、bundle identifier、DMG 内容和 ad-hoc 签名完整性。
- `.app` 及 bundle 内 Mach-O / 嵌套代码必须通过 `codesign --verify --deep --strict`；不把 `spctl` 的 Apple 已验证结论作为 ad-hoc 首版的验收条件。
- Release Summary/Release Notes 输出 macOS 产物文件名、大小和 SHA-256。
- 安装说明必须准确披露未经过 Apple Developer ID 公证，并提供正式的 Gatekeeper 手工放行步骤：首次尝试打开后进入“系统设置 → 隐私与安全性 → 仍要打开”并再次确认。
- 替换或删除应用只能影响应用本体，不删除用户配置、Workspace Registry、Agent State、OAuth State 或日志。

## Acceptance Criteria

- [ ] GitHub Release 稳定产出且只产出一个 macOS arm64 ad-hoc 签名 DMG。
- [ ] DMG 只包含 Serena Desktop.app 和 Applications 安装入口，bundle identifier 保持 `io.github.lifei6671.serena-desktop`，版本与 Release Tag 一致。
- [ ] DMG 内应用及嵌套 Mach-O 通过 ad-hoc `codesign` 完整性验证，主程序确认包含 arm64 且不承诺 Intel 支持。
- [ ] macOS Release verifier 输出 filename、size、SHA-256，并在结构、版本、架构、签名或内容异常时 fail closed。
- [ ] 从浏览器下载正式候选 DMG 后，Gatekeeper 手工允许流程通过真机验证并写入 README/Release Notes。
- [ ] Windows NSIS 构建、验证和发布契约不回归。
- [ ] 替换或删除 `.app` 后用户配置、任务数据库、Workspace Registry 和日志保留；重新安装可继续读取原数据。

## Deferred

- Developer ID Application 签名。
- Apple Notary Service、公证与 staple。
- Mac App Store。
- Intel x86_64 / Universal Binary。

以上能力在取得 Apple Developer Program 账户或明确扩展 CPU 支持范围后单独立项，不阻塞首版社区发行。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
