# Phase 1B：macOS App Bundle 基线

## Goal

为 macOS 增加平台专属 Tauri App Bundle 配置，使现有命令 `npm run tauri build` 生成带项目图标、可由 LaunchServices 启动且采用 ad-hoc 签名的 `Serena Desktop.app`，同时保持 Windows NSIS 配置和发布契约不变。

## Requirements

- 使用 `src-tauri/tauri.macos.conf.json` 覆盖 macOS bundle 设置，不修改全局 `tauri.conf.json` 中的 `targets: ["nsis"]`。
- macOS 默认 bundle target 只设为 `app`；DMG、CI 发布、Developer ID 和公证仍属于 Phase 5。
- App Bundle 继承现有 `icons/icon.icns`，不得复制或重新生成另一套图标资源。
- macOS 最低系统版本配置为已批准的候选值 `12.0`；本任务只验证配置写入 bundle，不声称已完成 macOS 12.0 真机验收。
- 在没有 Apple Developer Program 账户时使用 `signingIdentity: "-"` 进行 ad-hoc 签名，不引入证书、密钥或凭据配置。
- 为平台配置增加自动化契约测试，防止后续把 Windows 与 macOS bundle target 混入同一个全局列表。
- 验证原命令 `npm run tauri build` 产生 `.app`，而不是只留下 `target/release/serena-desktop` 裸 Mach-O。

## Acceptance Criteria

- [ ] 配置测试确认 Windows 基础配置仍为唯一 `nsis` target，macOS overlay 为唯一 `app` target。
- [ ] 配置测试确认 macOS 使用 `minimumSystemVersion: "12.0"` 和 `signingIdentity: "-"`，基础图标列表包含 `icons/icon.icns`。
- [ ] `npm run tauri build` 在当前 arm64 macOS 主机 exit 0，并生成 `src-tauri/target/release/bundle/macos/Serena Desktop.app`。
- [ ] App Bundle 的 `CFBundlePackageType` 为 `APPL`，`CFBundleIconFile` 指向 bundle 内实际存在的 `icon.icns`。
- [ ] `codesign --verify --deep --strict` 接受该 App Bundle，签名身份为 ad-hoc，不依赖开发者账户。
- [ ] 通过 `open -na` 由 LaunchServices 启动 `.app`，不把裸 Mach-O 当作用户入口。
- [ ] Windows NSIS 配置、workflow、installer verifier 和现有 Windows 源码无变化。

## Out of Scope

- DMG 布局与安装入口。
- Developer ID 签名、Notary Service、公证和 Gatekeeper 分发验收。
- GitHub Actions macOS runner 与 Release workflow。
- Finder 图标缓存刷新或更换视觉资产。
- Phase 2B StateStore、Claim 与 Startup Recovery。

## References

- Tauri 平台专属配置：<https://v2.tauri.app/reference/config/#platform-specific-configuration>
- Tauri macOS App Bundle：<https://v2.tauri.app/distribute/macos-application-bundle/>
