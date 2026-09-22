# macOS 版本改造任务清单

> **For agentic workers:** 实施本清单前必须先完成 Phase 0 决策并创建 Trellis 任务；实施阶段使用 `superpowers:subagent-driven-development` 或 `superpowers:executing-plans`。

**目标：** 在不降低已验收 Windows 版本质量的前提下，交付可安装、可签名、可公证、可验收的 macOS 版 Serena Desktop。

**架构方向：** 保留 React/Tauri 产品层和 MCP/Agent 业务契约，将 Windows Job Object、进程启动、终止证据和可执行文件发现收敛为平台实现。macOS 使用可验证的 Unix process group/session 模型，但不伪造与 Windows Named Job 等价的跨重启证据。

**技术栈：** Tauri 2、Rust、React 19、TypeScript、SQLite/rusqlite、GitHub Actions、Apple Developer ID / Notary Service。

---

## 1. 当前基线

### 1.1 已验证事实

- 当前代码已在 arm64、macOS 26.5.2 主机通过 `cargo check --locked` 和完整 Rust 测试；最低 macOS 12.0 真机仍未验证。
- Phase 1 的 Rust 编译阻断已经解除；Phase 2B StateStore、Claim 与 Startup Recovery 仍未完成。
- 前端主体为 React/WebView，组件和布局可复用；平台文案和少量系统交互需要分支。
- Tauri 配置已包含 `icon.icns`，并已初始化 `MacosLauncher::LaunchAgent`。
- Quick Tunnel 已包含 macOS arm64/x86_64 的 cloudflared 固定版本与 SHA-256 映射。
- 基础 `src-tauri/tauri.conf.json` 仍是 Windows NSIS authority；macOS 已使用独立的 `src-tauri/tauri.macos.conf.json`，当前主机执行 `npm run tauri build` 会生成 `.app`。
- 当前 GitHub Actions、Release、安装器验证和卸载策略仍只覆盖 Windows/NSIS。

### 1.2 关键证据位置

- 平台模块边界：`src-tauri/src/agent/mod.rs:9`
- Codex Windows launcher：`src-tauri/src/agent/codex/windows_launcher.rs:1`
- Codex Windows Runtime：`src-tauri/src/agent/codex/runtime.rs:1`
- Windows Runtime schema：`src-tauri/src/agent/schema_v1.sql:1`
- Windows 终止证据判定：`src-tauri/src/agent/store/transactions.rs:815`
- Serena 非 Windows 终止：`src-tauri/src/serena.rs:1066`
- macOS cloudflared 资产：`src-tauri/src/remote/quick_tunnel.rs:41`
- 系统打开命令：`src-tauri/src/commands.rs:457`
- bundle 配置：`src-tauri/tauri.conf.json:32`
- macOS bundle overlay：`src-tauri/tauri.macos.conf.json:1`
- Windows CI：`.github/workflows/ci.yml:19`
- Windows Release：`.github/workflows/release.yml:15`

---

## 2. 工作分解与依赖

```text
[Phase 0 范围决策]
          ↓
[Phase 1 macOS 可编译基线]
          ↓
[Phase 2 Runtime/恢复契约]
          ↓
[Phase 3 依赖发现与进程生命周期]
          ↓
[Phase 4 macOS 桌面集成]
          ↓
[Phase 5 签名、公证与发布]
          ↓
[Phase 6 双平台回归与人工验收]
```

Phase 2 是关键路径。在 Runtime 身份、终止证据和崩溃恢复契约未确定前，不应直接改数据库约束或解除 Windows 条件编译。

---

## 3. Phase 0：范围与发布决策

**预估：** 0.5～1 个工程日

- [ ] 确定首版是否必须与 Windows 功能对齐，包含 Codex Agent、任务恢复、Remote Access 和系统通知。
- [ ] 确定分发渠道。建议首版使用 GitHub Release + Developer ID 签名/公证 DMG，不进入 Mac App Store。
- [ ] 确定架构范围：Apple Silicon、Intel，或者两者。
- [ ] 确定产物策略。建议首版分别发布 arm64/x86_64 DMG，避免 Universal Binary 掩盖架构专属 CLI 问题。
- [ ] 根据依赖与真机结果确定最低 macOS 版本，并写入 Tauri `minimumSystemVersion`。
- [ ] 确认 Apple Developer Program、Developer ID Application 证书和 Notary Service 凭据可用。
- [ ] 为后续实施创建 Trellis 父任务，并按 Phase 1～6 建立可独立验证的子任务。

**退出条件：** 功能范围、CPU 架构、最低系统版本、分发渠道和签名责任人已记录。

---

## 4. Phase 1：macOS 可编译基线

**预估：** 2～4 个工程日

### 4.1 模块边界

- [ ] 在 `src-tauri/src/agent/mod.rs` 中将产品逻辑与 Windows Runtime 解耦。
- [ ] 保留 `product`、`work`、`task_manager` 为跨平台模块。
- [ ] 将 Codex 发现、launcher、runtime 和 recovery observation 收敛到清晰的平台边界。
- [ ] 保留 Windows 现有实现和错误码，不为 macOS 重写 Windows 逻辑。
- [ ] 修正 `source_write_atomic_replace.rs` 的 Unix `ErrorKind` 导入。
- [ ] 为 macOS 暂缺的 Runtime 能力提供显式、稳定的 unavailable 边界，不使用静默 fallback。

### 4.2 测试

- [ ] 为平台模块选择增加 compile-time 覆盖。
- [ ] 保留 Windows 单元测试的 `cfg(windows)` 边界。
- [ ] 将纯业务逻辑测试从 Windows 限制中移出。

### 4.3 验证命令

```bash
npm ci
npm run lint
npm run build
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo check --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

**退出条件：** macOS 主机完成全量 check/clippy/test，不再因 Windows 模块裁剪失败；不声称 Agent 运行时已可用。

---

## 5. Phase 2：Codex Agent Runtime 与恢复契约

**预估：** 7～12 个工程日

### 5.1 macOS launcher

- [ ] 使用固定 executable + argv 启动 Codex，不经过 shell command string。
- [ ] 在 exec 前建立独立 process group/session，避免启动成功后再追加归属的竞态。
- [ ] 保持 stdin/stdout/stderr 管道的最小继承集合。
- [ ] 为 launcher 输入、argv、cwd、空字节和超长参数增加单元测试。
- [ ] 定义 macOS 进程启动令牌，防止 PID 复用导致错误恢复或误杀。

### 5.2 Runtime 终止与证据

- [ ] 为正常取消实现“中断请求 → 宽限等待 → process group 终止”。
- [ ] 终止后同时验证直接 child 退出和受管 process group 不再存在。
- [ ] 终止证据绑定 Runtime ID、PID/PGID、启动令牌和观测时间。
- [ ] 无法确认终止时保持 `unknown` 和 Workspace Claim，不自动释放。
- [ ] 定义应用崩溃/强杀后 macOS 可证明的恢复上限。
- [ ] 若 macOS 不能证明原 Runtime 身份，必须进入人工收口，不模拟 Windows Named Job 证据。

### 5.3 State Store 迁移

- [ ] 设计新 migration，表达 Runtime 平台、containment 类型和平台证据。
- [ ] 保留现有 Windows Job 字段的语义和历史数据。
- [ ] 将当前只允许 `proc_thread_attribute_job_list` 的 CHECK 约束改为按 containment 类型验证。
- [ ] 增加 macOS 终止证据类型，并更新 Claim 释放查询。
- [ ] 使用真实旧库 fixture 验证 Windows 数据升级。
- [ ] 验证 migration 失败时旧库保持完整，不产生部分迁移。

### 5.4 必测场景

- [ ] Codex 正常启动、协议初始化和正常完成。
- [ ] 启动期取消不遗留 child/grandchild。
- [ ] 运行期取消不提前释放 Claim。
- [ ] Codex 直接崩溃后状态和证据一致。
- [ ] Serena Desktop 强杀后重启不误杀无关进程。
- [ ] PID 复用不能通过 Runtime 身份验证。
- [ ] 终止证据不完整时保持 fail-closed。
- [ ] 人工解锁只使用现有 Local Human Authority 入口。

**退出条件：** macOS Agent 的启动、取消、终止、崩溃恢复和 Claim 释放均有平台真实证据和自动化测试。

---

## 6. Phase 3：CLI 发现、Serena 安装和进程树

**预估：** 3～5 个工程日

### 6.1 Finder/LaunchAgent 环境

- [ ] 不假设 macOS GUI 进程继承 `.zshrc` 或 `.zprofile` 的 `$PATH`。
- [ ] 按既定顺序检查应用环境 PATH、`~/.local/bin`、`/opt/homebrew/bin`、`/usr/local/bin` 和产品私有 runtime。
- [ ] 可执行文件发现同时检查常规文件和 Unix execute bit。
- [ ] 状态页显示实际命中路径和稳定诊断，不暴露敏感环境内容。

### 6.2 Codex 发现

- [ ] 支持直接安装的 `codex`。
- [ ] 支持 npm 安装中的 `@openai/codex-darwin-arm64` 和 `@openai/codex-darwin-x64`。
- [ ] 仍然解析并验证 vendor binary，不通过 npm shell shim 启动 Runtime。
- [ ] 对架构不匹配、文件无执行权限和版本不兼容返回可区分诊断。

### 6.3 uv / Serena 安装

- [ ] 移除 macOS 路径对 `winget`、`LOCALAPPDATA` 和 `uv.exe` 的依赖。
- [ ] 冻结 macOS uv 获取方式和供应链验证。
- [ ] 按 CPU 架构选择 uv 产物，下载时校验固定摘要。
- [ ] 保留 `UV_TOOL_DIR`、`UV_TOOL_BIN_DIR`、固定 Serena 版本和安装后能力检查。
- [ ] 验证含空格、中文和外置卷路径。

### 6.4 进程树管理

- [ ] Serena 主服务、Workspace Serena Runtime 和 cloudflared 均在独立 process group 中启动。
- [ ] 停止时终止完整 process group，不只调用直接 child `kill()`。
- [ ] 超时、取消、应用退出和启动失败共用同一所有权契约。
- [ ] 证明停止不会影响用户在 Terminal 中自行启动的 Serena/Codex/cloudflared。

**退出条件：** 从 Finder 和登录项启动时，Codex、Git、uv、Serena 仍能稳定发现或给出可操作诊断；所有受管进程树可完整回收。

---

## 7. Phase 4：macOS 桌面集成

**预估：** 2～3 个工程日

### 7.1 系统行为

- [ ] macOS 使用 `/usr/bin/open` 打开 URL 和日志目录，Linux 仍使用 `xdg-open`。
- [ ] 处理 macOS Dock reopen 事件，无可见窗口时显示并聚焦主窗口。
- [ ] 验证红色关闭按钮、隐藏到菜单栏和 `Cmd+Q` 的不同语义。
- [ ] `Cmd+Q` 和菜单栏“退出”均必须等待既有 shutdown 流程完成。
- [ ] 验证 single-instance 二次启动能唤醒隐藏窗口。
- [ ] 验证 LaunchAgent 启用、禁用、登录启动和应用升级后路径。

### 7.2 菜单栏、通知与声音

- [ ] 决定 macOS 菜单栏图标左键是打开菜单还是显示主窗口，并保持一致。
- [ ] 验证通知权限的未决定、允许和拒绝状态。
- [ ] 为 macOS 实现真实声音提示，或在 macOS 隐藏声音开关；不保留“开关开启但实际静默”。
- [ ] 验证通知点击后的应用激活行为。

### 7.3 权限和文件系统

- [ ] 在 macOS `Info.plist` 增加 `NSLocalNetworkUsageDescription`，说明 LAN MCP/Broker 用途。
- [ ] 在 macOS 15+ 真机验证 LAN 首次授权、拒绝、重启和系统设置恢复。
- [ ] 验证桌面、文稿、下载、iCloud Drive 和外置卷中的 Workspace。
- [ ] 验证工作区选择器授予的路径可被 Serena、Git、CodeGraph 和 Source Tools 继续使用。
- [ ] 首版直接分发不启用 App Sandbox；若后续进入 Mac App Store，必须另立任务设计 security-scoped bookmarks 和外部 CLI 执行策略。

### 7.4 UI 和文案

- [ ] 将“Windows 与应用生命周期”改为平台中性文案。
- [ ] 将“Windows 登录后启动”改为“登录后启动”或平台化标签。
- [ ] macOS 下将“托盘”显示为“菜单栏”。
- [ ] 调整 `Alt`/`Option`、`Ctrl`/`Command` 等快捷键提示。
- [ ] 等宽字体增加 `SF Mono`/`Menlo` 回退，保留现有中文字体。
- [ ] 更新 `docs/ui/DESIGN.md` 中的 `Windows-native shell` 定义，不改变现有信息密度和色彩系统。

**退出条件：** Finder、Dock、菜单栏、LaunchAgent、通知、LAN 权限和文件选择均通过真机验收。

---

## 8. Phase 5：macOS 打包、签名、公证和发布

**预估：** 2～4 个工程日

### 8.1 Tauri 配置

- [x] 创建 macOS 平台配置，避免直接将全局 `targets` 从 `nsis` 改成会影响 Windows 的值。
- [ ] 输出 `.app` 和 `.dmg`。
- [ ] 配置 category、minimum system version、copyright、Info.plist 和必要 entitlements。
- [ ] 保留 Hardened Runtime，不添加未证明必要的 exception entitlement。
- [ ] 验证 `icon.icns` 在 Finder、Dock、菜单栏和 DMG 中的效果。
- [ ] 确认 `THIRD_PARTY_NOTICES` 在 macOS bundle 中的位置。

### 8.2 GitHub Actions

- [ ] CI 增加 macOS runner，执行前端和 Rust 全量质量 Gate。
- [ ] Release 拆分为 Windows 产物、macOS arm64 产物、macOS x86_64 产物和统一发布阶段。
- [ ] 发布阶段仅上传经过验证的 NSIS/DMG 产物。
- [ ] 保留 tag-only、版本一致性、`--locked`、禁止 soft-fail 等现有契约。
- [ ] 为 workflow 结构更新 `scripts/ci-workflow.test.mjs` 和 `scripts/release-workflow.test.mjs`。

### 8.3 签名与公证

- [ ] 在 GitHub Secrets 中配置 Developer ID 证书、密码和签名身份。
- [ ] 配置 App Store Connect API Key，或 `APPLE_ID` + app-specific password + Team ID。
- [ ] 完成 codesign、notarization 和 staple。
- [ ] 对 `.app` 内所有 Mach-O 和嵌套代码执行签名校验。
- [ ] 使用 `spctl` 验证 Gatekeeper 接受结果。

### 8.4 产物验证脚本

- [ ] 新增 macOS installer verifier，检查产物唯一性、版本、架构、文件大小和 SHA-256。
- [ ] 检查 `.app` 的 bundle identifier 仍为 `io.github.lifei6671.serena-desktop`。
- [ ] 检查 DMG 包含唯一的 Serena Desktop.app 和 Applications 安装入口。
- [ ] 验证替换 `.app` 和删除 `.app` 不删除用户配置、任务数据库和日志。

**参考：**

- Tauri macOS 分发：<https://v2.tauri.app/distribute/>
- Tauri DMG：<https://v2.tauri.app/distribute/dmg/>
- Tauri macOS 配置：<https://v2.tauri.app/reference/config/#macconfig>
- Apple 公证要求：<https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution>
- Apple 本地网络隐私：<https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy>

**退出条件：** GitHub Release 可以稳定产出经签名、公证、staple 和脚本验证的 macOS DMG，同时 Windows NSIS 产物不回归。

---

## 9. Phase 6：双平台回归与人工验收

**预估：** 3～5 个工程日

### 9.1 自动化 Gate

- [ ] Windows 执行完整 lint/build/test/check/clippy/Rust test/NSIS policy Gate。
- [ ] macOS arm64 执行完整 lint/build/test/check/clippy/Rust test/DMG Gate。
- [ ] 如承诺 Intel，macOS x86_64 执行相同 Gate。
- [ ] 两个平台均验证 release tag 与 package/Cargo/Tauri 版本一致。
- [ ] `git diff --check` 通过，工作流中无 `continue-on-error` 或等价软失败。

### 9.2 macOS 真机验收

- [ ] 全新用户环境从 DMG 安装，Gatekeeper 无异常警告。
- [ ] 首次启动、单实例、Dock reopen、菜单栏和优雅退出通过。
- [ ] Git、uv、Serena、Codex、CodeGraph 在 Terminal 启动和 Finder 启动两种情况下验证。
- [ ] Serena 安装、启动、停止、重启、索引和 Workspace 切换通过。
- [ ] Codex Agent 创建、中断、取消、后续执行、应用崩溃和重启恢复通过。
- [ ] Quick Tunnel、ngrok、自建 HTTPS 和仅 MCP 模式通过。
- [ ] 通知权限、声音、LAN 权限、文件选择和外置卷通过。
- [ ] 覆盖含空格、中文、软链接和大小写差异的 Workspace 路径。
- [ ] 覆盖升级安装、删除应用和重新安装，验证用户数据保留。

### 9.3 文档

- [ ] README 增加 macOS 下载、CPU 架构选择、DMG 安装和首次权限说明。
- [ ] README 开发命令增加 macOS 前置依赖和验证命令。
- [ ] 状态页/设置页的 Windows 专属文案全部平台化。
- [ ] 记录 macOS Runtime 恢复能力与 Windows Job Object 的已知差异。
- [ ] 记录公证失败、CLI 找不到、LAN 权限拒绝和残留进程的排查入口。

**最终退出条件：** 自动化 Gate 全部通过，macOS 真机验收清单已完成，Windows 版回归通过，签名/公证产物可从 GitHub Release 独立下载和安装。

---

## 10. 预估工作量

| 范围 | 粗略工作量 | 说明 |
|---|---:|---|
| 完整对齐 Windows | 4～7 个工程周 | 包含 Agent Runtime/Store 迁移、双架构、签名公证与真机验收 |
| 仅 Serena/MCP，不提供 Agent | 1～2 个工程周 | 属于功能降级版，不能按完整 macOS 版验收 |

工作量的最大变量是 Phase 2，而不是 UI 或 DMG 打包。若 Runtime 恢复契约发生实质变更，必须先更新设计文档和数据迁移方案，再重新估算。

---

## 11. 实施期不可退让项

- [ ] 不用“直接 child 已退出”代替“受管进程树已终止”证据。
- [ ] 不用 PID 单值作为跨重启 Runtime 身份。
- [ ] 不为了让 macOS 通过而释放证据不完整的 Workspace Claim。
- [ ] 不通过 shell string 启动 Codex、Serena 或 cloudflared。
- [ ] 不把未签名或未公证产物标记为正式 macOS Release。
- [ ] 不删除、跳过或弱化现有 Windows 验收 Gate。
- [ ] 不以覆盖率数字代替进程崩溃、重启恢复、PID 复用和真机分发测试。
