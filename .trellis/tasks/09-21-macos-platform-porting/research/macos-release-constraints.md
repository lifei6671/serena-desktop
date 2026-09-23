# macOS 首版发布约束调研

## 已确认决策

- 首版分发渠道为 GitHub Release + DMG，不进入 Mac App Store。
- 首版仅支持 Apple Silicon `arm64`，最低系统版本目标为 macOS 12.0。
- 当前没有 Apple Developer Program 账户，首版采用 Tauri ad-hoc 签名，不执行 Developer ID 签名、公证或 staple。
- 发布说明必须披露 Gatekeeper 未验证状态和用户手动允许步骤，不得声称产物已通过 Apple 验证。

## 官方依据

- Apple Developer ID：Developer ID 证书用于 Mac App Store 之外的可信分发，证书由 Apple Developer Program 或 Enterprise Program 成员取得。
  - https://developer.apple.com/developer-id/
  - https://developer.apple.com/help/glossary/developer-id-certificate/
- Apple 公证：标准公证要求有效 Developer ID 签名、Hardened Runtime 和安全时间戳。
  - https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
- Tauri macOS 签名：可将 `signingIdentity` 配置为 `-` 进行 ad-hoc 签名，但用户仍需在“隐私与安全性”中手动允许应用。
  - https://v2.tauri.app/distribute/sign/macos/
- Tauri macOS 配置：`bundle.macOS.minimumSystemVersion` 会写入 `LSMinimumSystemVersion` 并设置 `MACOSX_DEPLOYMENT_TARGET`；Apple Silicon-only 目标采用 12.0 候选值，最终仍需最低版本运行验证。
  - https://v2.tauri.app/reference/config/

## 实施影响

- Phase 5 仍需验证 DMG 结构、架构、摘要、bundle identifier、ad-hoc 签名完整性和 GitHub Release Gate。
- Phase 5 不把 Developer ID、公证和 staple 作为首版退出条件；这些能力应在取得开发者账户后单独立项。
- Phase 6 必须覆盖从浏览器下载后的真实 Gatekeeper 流程，并验证文档中的手动允许步骤。
