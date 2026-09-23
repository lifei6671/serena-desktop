# Phase 5 CI 与 Release 设计

- `tauri.conf.json` 继续定义 Windows NSIS；macOS overlay 只覆盖 `app`/`dmg`、ad-hoc 签名、Hardened Runtime、12.0 最低系统版本和本地网络权限文案。
- CI 用两个独立平台 job 执行同一组前端和 Rust Gate；macOS job 先核验 `uname -m`。
- Release 用 `release-windows`、`release-macos`、`publish` 三个 job。两端各自注入 tag 版本、完成全套质量 Gate、构建及验证，然后上传唯一正式产物。`publish` 只在两个构建成功后下载这两个 workflow artifact 并单次创建 GitHub Release。
- Release workflow 顶层 `contents: read`；只有 `publish` job 获得 `contents: write`，构建 job 不持有发布写权限。
- macOS verifier 在 macOS 主机挂载 DMG，检查可见顶层内容、Info.plist、主程序 arm64 slice、ad-hoc 签名和 SHA-256；挂载后无论成功失败都 detach。输出单行稳定 JSON。
- 不改变业务逻辑、不引入 entitlement；不以本机验证代替 GitHub Release、Gatekeeper 或 Windows 实机验收。
