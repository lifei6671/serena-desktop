# Phase 1B：macOS App Bundle 基线设计

## 1. 根因与边界

当前 `src-tauri/tauri.conf.json` 将 `bundle.targets` 固定为 `["nsis"]`。在 arm64 macOS 上执行 `npm run tauri build` 时，Tauri 成功编译 release 裸 Mach-O，但没有生成 `.app`，最终只报告 `src-tauri/target/release/serena-desktop`。用户从 Finder 点击这个裸可执行文件时，macOS 按命令行程序启动；它既不具备 App Bundle 的 LaunchServices 身份，也不会使用 bundle 中的 `icon.icns`。

现有 `src-tauri/icons/icon.icns` 是有效的 1024×1024 macOS icon 资源。问题位于 bundle target 选择，不修改 Rust `main()`、窗口创建逻辑或图标内容。

## 2. 配置方案

新增 `src-tauri/tauri.macos.conf.json`：

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "bundle": {
    "targets": ["app"],
    "macOS": {
      "minimumSystemVersion": "12.0",
      "signingIdentity": "-"
    }
  }
}
```

Tauri 会把平台专属配置与基础 `tauri.conf.json` 合并。基础配置继续拥有 Windows `nsis` target、WebView 安装策略、资源和通用 icon 列表；macOS 只覆盖 target 和 macOS 专属字段。因此原命令 `npm run tauri build` 在 Windows 仍构建 NSIS，在 macOS 则构建 `.app`。

本任务不将 target 改为 `all`，也不在基础配置同时列出 `nsis`、`app` 和 `dmg`，避免跨平台 bundler 接受无关目标。

## 3. 签名与启动语义

`signingIdentity: "-"` 明确请求本机 ad-hoc 签名，不读取 Apple Developer 证书或公证凭据。该签名只用于本地构建和测试，不代表 Gatekeeper 可接受的公开分发签名。

用户入口固定为：

```text
src-tauri/target/release/bundle/macos/Serena Desktop.app
```

`target/release/serena-desktop` 仍是 App Bundle 内部构建输入，不作为 Finder 用户入口。通过 `open -na ".../Serena Desktop.app"` 交给 LaunchServices 启动，验证它被识别为 GUI App Bundle；不修改 `main.rs`，因为其 `windows_subsystem` 属性仅影响 Windows，macOS GUI/命令行身份由 `.app` bundle 决定。

## 4. 自动化验证

新增一个 Node 测试读取基础配置和 macOS overlay，冻结以下契约：

- 基础 `bundle.targets` 精确为 `["nsis"]`；
- macOS `bundle.targets` 精确为 `["app"]`；
- macOS 最低版本和 ad-hoc identity 精确；
- 基础 icon 列表包含 `icons/icon.icns`，且源文件存在；
- macOS overlay 不复制 Windows bundle 配置，也不提前加入 `dmg`。

真实 bundle Gate 在当前 macOS 主机执行：

```bash
npm run tauri build
test -d "src-tauri/target/release/bundle/macos/Serena Desktop.app"
plutil -extract CFBundlePackageType raw "src-tauri/target/release/bundle/macos/Serena Desktop.app/Contents/Info.plist"
plutil -extract CFBundleIconFile raw "src-tauri/target/release/bundle/macos/Serena Desktop.app/Contents/Info.plist"
test -f "src-tauri/target/release/bundle/macos/Serena Desktop.app/Contents/Resources/icon.icns"
codesign --verify --deep --strict "src-tauri/target/release/bundle/macos/Serena Desktop.app"
codesign -dv --verbose=4 "src-tauri/target/release/bundle/macos/Serena Desktop.app"
open -na "src-tauri/target/release/bundle/macos/Serena Desktop.app"
```

`CFBundlePackageType` 必须为 `APPL`。`CFBundleIconFile` 可由 Tauri 写成 `icon.icns` 或等价的 `icon` 名称，验证脚本以实际 plist 值解析对应文件，不硬编码重复资源。`codesign -dv` 必须报告 ad-hoc 签名，不接受意外命中的本机 Developer ID。

## 5. 顺序与非目标

先完成并提交本任务，再开始 Phase 2B。这样后续 Runtime/Recovery 的人工真机测试都从真实 `.app` 启动，不再误用裸 Mach-O。

本任务不生成 DMG，不修改 CI/Release，不声明 Developer ID 或公证完成，也不更新最低 macOS 12.0 真机验收结论。这些仍由 Phase 5/6 负责。
