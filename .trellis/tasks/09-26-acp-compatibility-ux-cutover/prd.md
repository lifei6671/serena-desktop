# ACP Compatibility UX Cutover

## Goal

移除旧的 CodeBuddy 产品版本白名单/unsupported-version UX 语义，统一切换为 ACP protocol/base-capability compatibility。产品版本、FileVersion、commit、binary hash 只做诊断和复现，不参与是否允许连接。

## Requirements

- 前端不再识别或展示 `CODEBUDDY_VERSION_UNSUPPORTED`。
- 新稳定 consumer seam 使用 exact `CODEBUDDY_ACP_INCOMPATIBLE`。
- 命中时提示：`<displayName> 的 ACP 协议或必需能力与当前 SerenaDesktop 不兼容。请升级 CodeBuddy 或 SerenaDesktop 后重新检测。`
- 不把 provider version 拼进阻断文案；version 继续作为普通元数据显示。
- generic unavailable / 任意新产品版本 / 任意 binary hash 不得触发 ACP incompatibility 文案。
- 不根据 providerId、version、health、errorMessage 猜兼容性。
- 不提供绕过 ACP protocol gate 的 override。
- Provider Cards、启停、Role Routing、pending Claim、sidebar/detail 继续回归。
- 不修改 Rust backend、Remote MCP、Runtime、Provider admission。

## Acceptance

- [ ] exact CODEBUDDY_ACP_INCOMPATIBLE 显示 ACP incompatibility 文案。
- [ ] CODEBUDDY_VERSION_UNSUPPORTED 不再有专用 UI 语义。
- [ ] 产品版本变化本身不显示阻断。
- [ ] generic unavailable 不显示 ACP incompatibility。
- [ ] no providerId special case / no override。
- [ ] focused/full frontend tests, build, lint, diff-check PASS。
- [ ] Rust/Remote 0 变化。