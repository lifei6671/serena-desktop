# macOS 版本改造任务清单

> **For agentic workers:** 实施本清单前必须先完成 Phase 0 决策并创建 Trellis 任务；实施阶段使用 `superpowers:subagent-driven-development` 或 `superpowers:executing-plans`。

**目标：** 在不降低已验收 Windows 版本质量的前提下，交付可由普通用户下载、安装并完成首次 Gatekeeper 手工放行后正常使用的 macOS Apple Silicon 版 Serena Desktop。首版使用 ad-hoc 签名 DMG；Developer ID 签名、公证和 staple 延期。

**架构方向：** 保留 React/Tauri 产品层和 MCP/Agent 业务契约，将 Windows Job Object、进程启动、终止证据和可执行文件发现收敛为平台实现。macOS 使用可验证的 Unix process group/session 模型，但不伪造与 Windows Named Job 等价的跨重启证据。

**技术栈：** Tauri 2、Rust、React 19、TypeScript、SQLite/rusqlite、GitHub Actions、macOS ad-hoc code signing、DMG。

---

## 1. 当前基线

### 1.1 已验证事实

- 当前代码已在 arm64、macOS 26.5.2 主机通过 `cargo check --locked` 和完整 Rust 测试；最低 macOS 12.0 真机仍未验证。
- Phase 1 的 Rust 编译阻断已经解除；Phase 2A/2B 已完成 launcher、Runtime containment、StateStore v10、Startup Recovery 与 Claim fail-closed 契约；Phase 3A 已将 macOS ARM64 Runtime 接入共享 Codex Provider。
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
[Phase 5 DMG、ad-hoc 签名与发布]
          ↓
[Phase 6 双平台回归与人工验收]
```

Phase 2 是关键路径。在 Runtime 身份、终止证据和崩溃恢复契约未确定前，不应直接改数据库约束或解除 Windows 条件编译。

---

## 3. Phase 0：范围与发布决策

**预估：** 0.5～1 个工程日

- [x] 首版与 Windows 功能对齐，包含 Codex Agent、任务恢复、Remote Access 和系统通知；允许分阶段实现，但发布前必须完成总验收。
- [x] 首版分发渠道固定为 GitHub Release + DMG，不进入 Mac App Store。
- [x] 确定首版架构范围仅为 Apple Silicon（ARM64 / aarch64）；Intel Mac 与 Rosetta x86_64 Codex 不在首版范围。
- [x] 确定首版产物只保留 arm64 语义；Universal Mach-O 仅在包含 ARM64 slice 且通过完整 compatibility 验证时可作为候选，不因此承诺 Intel 支持。
- [x] 首版最低系统版本目标记录为 macOS 12.0，并写入 Tauri `minimumSystemVersion`；真实兼容性必须在 Phase 6 的 macOS 12.0 或可信等价环境中验证，失败时只允许提高最低版本并记录证据。
- [x] 当前没有 Apple Developer Program 账户；首版使用 ad-hoc 签名，不要求 Developer ID、公证或 staple，也不得声称 Apple 已验证。
- [x] 已创建 Trellis 父任务，并按 Phase 1～6 建立可独立验证的子任务。

**退出条件：** 功能范围、CPU 架构、最低系统版本目标、分发渠道和首版签名责任已经冻结；最低版本的真实兼容性属于 Phase 6 发布验收。

---

## 4. Phase 1：macOS 可编译基线

**预估：** 2～4 个工程日

### 4.1 模块边界

- [x] 在 `src-tauri/src/agent/mod.rs` 中将产品逻辑与 Windows Runtime 解耦。
- [x] 保留 `product`、`work`、`task_manager` 为跨平台模块。
- [x] 将 Codex 发现、launcher、runtime 和 recovery observation 收敛到清晰的平台边界。
- [x] 保留 Windows 现有实现和错误码，不为 macOS 重写 Windows 逻辑。
- [x] 修正 `source_write_atomic_replace.rs` 的 Unix `ErrorKind` 导入。
- [x] Phase 1 曾为当时尚未实现的 macOS Runtime 提供显式 unavailable 边界；后续 Phase 2/3 已用真实 macOS Runtime / Provider 替代该临时边界，没有保留静默 fallback。

### 4.2 测试

- [x] 为平台模块选择增加 compile-time 覆盖。
- [x] 保留 Windows 单元测试的 `cfg(windows)` 边界。
- [x] 将纯业务逻辑测试从 Windows 限制中移出。

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

**当前状态：** Phase 2A/2B 的底层进程与恢复证据契约已经实现并通过 macOS 自动化测试；Phase 3A 已接入真实 App Server initialize/stdio/shutdown，execute/continue/cancel 共享 Provider 契约已通过 fixture 回归。仍未用真实模型 Turn 消耗账户用量做端到端业务验收。

### 5.1 macOS launcher

- [x] 使用固定 executable + argv 启动 Codex，不经过 shell command string。
- [x] 在 exec 前建立独立 process group/session，避免启动成功后再追加归属的竞态。
- [x] 保持 stdin/stdout/stderr 管道的最小继承集合。
- [x] 为 launcher 输入、argv、cwd、空字节和超长参数增加单元测试。
- [x] 定义 macOS 进程启动令牌，防止 PID 复用导致错误恢复或误杀。

### 5.2 Runtime 终止与证据

- [x] 业务取消保持 Codex `turn/interrupt`；Runtime teardown 独立使用 `SIGTERM → bounded grace → SIGKILL`，两套语义不互相替代。
- [x] 终止后同时验证直接 child 退出和受管 process group 不再存在。
- [x] 终止证据绑定 Runtime ID、PID/PGID、启动令牌和观测时间。
- [x] 无法确认终止时保持 `unknown` 和 Workspace Claim，不自动释放。
- [x] 定义应用崩溃/强杀后 macOS 可证明的恢复上限。
- [x] 若 macOS 不能证明原 Runtime 身份，必须进入人工收口，不模拟 Windows Named Job 证据。

### 5.3 State Store 迁移

- [x] 设计新 migration，表达 Runtime 平台、containment 类型和平台证据。
- [x] 保留现有 Windows Job 字段的语义和历史数据。
- [x] 将当前只允许 `proc_thread_attribute_job_list` 的 CHECK 约束改为按 containment 类型验证。
- [x] 增加 macOS 终止证据类型，并更新 Claim 释放查询。
- [x] 使用真实旧库 fixture 验证 Windows 数据升级。
- [x] 验证 migration 失败时旧库保持完整，不产生部分迁移。

### 5.4 必测场景

- [ ] Codex 正常启动、协议初始化和正常完成。
- [ ] 启动期取消不遗留 child/grandchild。
- [ ] 运行期取消不提前释放 Claim。
- [ ] Codex 直接崩溃后状态和证据一致。
- [x] Serena Desktop 强杀后重启不误杀无关进程。
- [x] PID 复用不能通过 Runtime 身份验证。
- [x] 终止证据不完整时保持 fail-closed。
- [x] 人工解锁只使用现有 Local Human Authority 入口。

**退出条件：** macOS Agent 的启动、取消、终止、崩溃恢复和 Claim 释放均有平台真实证据和自动化测试。

---

## 6. Phase 3：CLI 发现、Serena 安装和进程树

**预估：** 3～5 个工程日

**当前状态：** Phase 3A（Codex Provider / ARM64 discovery）与 Phase 3B（uv / 进程树）主体已实现；Finder 路径已验证旧候选、GUI 精简环境、Codex 0.155.1、Serena 1.7.0 与 CodeGraph 1.6.0 binary discovery。Host 普通 Terminal 的 current-user LaunchAgent registration roundtrip/exact restore 为 `PASS`；独立临时 job PID 97374 的真实 argv、精简 PATH/HOME、旧 executable SHA、单主实例及 Codex/Git/uv/Serena discovery 均为 `PASS`。Host cleanup 后 service、临时 plist、主 PID 与旧 Codex identities 已消失，但 Serena PID 97403 成为 PPID 1 的孤儿并占用 `9121`，正式 Runtime termination evidence 为 `unknown`，所以该次 cleanup 为 `FAIL`。根因是 launchd `SIGTERM` 未进入现有 Tauri shutdown authority；当前 macOS-only 修复已用 Tokio 安全 signal API 把 `SIGTERM` 投递到 Tauri 主线程上的同一 `request_exit`，完整 Rust Gate 与新 ARM64 `.app` 静态 Gate 均通过，新 executable SHA-256 为 `eb11a6ff…11ea0`。真实 bootout cleanup 复验仍为 `NOT_RUN`，不能用代码/自动化替代。主窗口隐藏视觉语义留给 Phase 4/6，不阻塞 Phase 3。CodeGraph Workspace 的 `NOT_PREPARED` 仍是非阻塞 preparation state。当前 blocker 为待复验的 cleanup、外置 APFS 卷 Serena 安装和 Windows 当前版本同步后的实机回归。Codex compatibility 继续使用“平台/架构预检 + `--version` 与 schema export + 必要 JSON Schema 子集校验”，version/binary/schema hash 只作身份和诊断证据。

### 6.1 Finder/LaunchAgent 环境

- [x] 不假设 macOS GUI 进程继承 `.zshrc` 或 `.zprofile` 的 `$PATH`；Finder 风格精简 PATH 已有自动化回归。
- [x] 按固定顺序检查当前 PATH、`~/.local/bin`、Homebrew/npm 确定位置、npm ARM64 vendor binary，最后才是 ChatGPT.app bundled candidate。
- [x] 可执行文件发现同时检查 regular file、Unix execute bit、canonical path 与 ARM64 Mach-O slice。
- [ ] 状态页显示实际命中路径和稳定诊断，不暴露敏感环境内容。

### 6.2 Codex 发现

- [x] 支持 PATH 或确定位置中直接安装的 ARM64 `codex`；早期不兼容候选不会阻断后续兼容候选。
- [x] 支持 npm 安装中的 `@openai/codex-darwin-arm64` vendor binary；首版明确不支持 `darwin-x64` 或 Rosetta fallback。
- [x] 解析并验证真实 vendor Mach-O binary，Runtime 使用 absolute executable + argv 直接启动，不通过 npm JavaScript shim 或 shell wrapper。
- [x] 对 host/candidate 架构不匹配、无执行权限、非 Mach-O，以及必要 App Server Schema 契约不兼容返回稳定诊断。
- [x] 兼容性准入复用 Windows/macOS 共享的必要 JSON Schema 子集校验器；version、binary SHA-256 与完整 schema SHA-256 只作为 Runtime identity / diagnostic evidence，不作为准入白名单。macOS 首版仍只接受原生 ARM64 Mach-O，不支持 x86_64/Rosetta fallback。

### 6.2.1 Codex 兼容性与历史证据（2026-09-22）

- [x] `codex-cli 0.153.4` ARM64 npm artifact 曾完成 architecture/version/binary hash/schema hash 与真实 App Server lifecycle 验证；这些值保留为历史 Contract Evidence，不再充当当前全局准入白名单。
- [x] 当前兼容性探针只执行受管的 `codex --version` 与 `codex app-server generate-json-schema --experimental --out <dir>`，不启动 App Server、不发送业务 JSON-RPC；生成的 schema 由 Windows/macOS 共用的校验器验证 SerenaDesktop 实际依赖的方法、字段、类型与 ThreadItem 变体。
- [x] schema 新增方法、定义或可选字段允许通过；缺少必要方法/字段或关键类型变化返回 `CODEX_APP_SERVER_INCOMPATIBLE`。version、binary SHA-256 与完整 schema SHA-256 仍持久化/记录，用于 Runtime identity、诊断和回归证据。
- [x] 正式 Runtime 仍必须通过 initialize / initialized、stdio JSONL、Provider lifecycle、Process Group termination evidence 与既有 Runtime/Claim 安全契约；schema probe 通过不能替代真实 Runtime Gate。
- [x] 外置 APFS 卷中含空格和中文的 canonical 路径已通过 ARM64 candidate / lifecycle 验证，测试卷与镜像已卸载删除。
- [x] Finder 双击当前候选、GUI 精简环境与 live Codex 0.155.1 discovery/runtime 已通过；独立 LaunchAgent PID 97374 的真实 job、精简 PATH/HOME、`--autostart` argv 与 Codex/Git/uv/Serena discovery 也已通过。PID 68522 的 Finder 标准退出为 `PASS`；临时 LaunchAgent cleanup 的独立失败见 6.4/6.6，Phase 6 release candidate 将再做完整退出验收。
- [ ] Windows 实机 Job Object / Host Crash Gate 尚未重跑；共享兼容校验器与 Provider fixture 回归不能替代 Windows 实机证据。

### 6.3 uv / Serena 安装

- [x] 移除 macOS 路径对 `winget`、`LOCALAPPDATA` 和 `uv.exe` 的依赖。
- [x] 冻结 macOS uv 获取方式和供应链验证。
- [x] 按 CPU 架构选择 uv 产物，下载时校验固定摘要。
- [x] 保留 `UV_TOOL_DIR`、`UV_TOOL_BIN_DIR`、固定 Serena 版本和安装后能力检查。
- [ ] 验证含空格、中文和外置卷路径。

### 6.4 进程树管理

- [x] Serena 主服务、Workspace Serena Runtime 和 cloudflared 均在独立 process group 中启动。
- [x] 停止时终止完整 process group，不只调用直接 child `kill()`。
- [ ] 超时、取消、应用退出和启动失败共用同一所有权契约；Finder 标准退出已通过，旧临时 LaunchAgent cleanup 的 Serena PID 97403/`9121` 残留与正式 Runtime `unknown` 已定位并形成 SIGTERM 修复候选，但新候选尚未完成 Host bootout 实证，应用退出路径仍未闭环。
- [x] 证明停止不会影响用户在 Terminal 中自行启动的 Serena/Codex/cloudflared。

### 6.5 Phase 3B 自动化证据（2026-09-22）

- [x] Codex leader 在 grace 期间退出时不再仅凭旧 PGID 升级 `SIGKILL`，而是返回 `CODEX_RUNTIME_TERMINATION_UNCONFIRMED`。
- [x] Darwin helper 验证 `PID=PGID=SID`、直接 child reap、group empty，以及终止目标组不影响另一独立组。
- [x] Serena 安装命令超时和 Quick Tunnel 停止均通过带 descendant 的 fixture，未遗留后代进程。
- [x] 官方固定 uv `0.12.17` 归档已执行显式网络测试，固定 SHA-256、精确成员提取、权限与运行版本验证通过。
- [x] `cargo check --locked`、`cargo clippy --locked --all-targets -- -D warnings` 和完整 `cargo test --locked` 通过；完整测试结果为 `1069 passed; 0 failed; 19 ignored`。
- [x] Finder 真人启动、GUI 精简 PATH、Serena 1.7.0 user-local discovery 与 CodeGraph 1.6.0 user-local binary discovery 已通过；CodeGraph Workspace readiness/query 为 `NOT_PREPARED`，但不属于 Phase 3 退出 Gate。
- [x] LaunchAgent registration roundtrip ignored Gate 已由 Host 普通 Terminal 执行通过：`enable=true`、target/`--autostart` 参数正确、`disable=false`、`exact_state=true`，测试 plist 无遗留。
- [x] 真实 launchd job/LaunchAgent environment/discovery 为 `PASS`：独立 label/plist 在 `gui/501` domain 拉起 PID 97374，真实 argv、精简 PATH/HOME、executable SHA、单主实例与 Codex/Git/uv/Serena discovery 均通过。主窗口隐藏视觉语义留给 Phase 4/6。Host cleanup 已移除 job/plist/main，但未回收 Serena child；外置卷安装与 Windows 当前版本同步后的实机生命周期状态不变。
- [x] macOS `SIGTERM` 已通过 Tokio `SignalKind::terminate()` 安全接收，并经 `run_on_main_thread` 调用既有 `request_exit`；测试证明单次 signal dispatch 只调用一次退出 callback，且与 `run_shutdown_once` 的幂等 owner 语义兼容。Windows 与其他 signal 未改变，`Cargo.lock` 无变化。

### 6.6 Phase 3 closeout 复核（2026-09-22）

- [x] 当前 `codex-cli 0.155.1` ARM64 已按共享契约完成 host/candidate preflight、受管 `--version`、schema export 与必要子集校验；binary SHA-256 `8eaf1ad1…`、schema SHA-256 `058e9af9…` 仅作 identity/diagnostic evidence。
- [x] 正式 Codex lifecycle 与真实 Product E2E 通过：initialize、start 正常完成、同 thread continue、cancel、Claim 释放和 `macos_live_process_group_empty` 均有当前证据；schema probe 未发送 App Server RPC。
- [x] ARM64 `.app` 已构建并通过 ad-hoc `codesign --verify --deep --strict`；Host 从 Finder 标准重启的新实例 PID 81872 启动时间为 `2026-09-22T21:37:35.192149+08:00`，晚于构建完成时间，executable SHA-256 精确匹配候选 `ae61d53b…018c45`。该实例在 GUI 精简 PATH 下发现 canonical Codex 0.155.1、官方 Serena 1.7.0 user-local target 与 CodeGraph 1.6.0 user-local target。
- [x] 产品 `install_serena()` 在中文+空格隔离路径安装并验证官方 Serena 1.7.0；独立 start/restart/stop 与首次 Workspace capability acquire 均通过。
- [x] 当前完整 Rust Gate 为 `1082 passed; 0 failed; 25 ignored`；fmt、check、clippy 均通过。`npm run tauri build` 内含 frontend production build，生成的新 ARM64 `.app` 通过 ad-hoc codesign 静态核验；executable SHA-256 为 `eb11a6ffc4f72037eae59f1aac13c33fd6e38b076e8e093b87f869c29d811ea0`。
- [x] Finder 双击来源、新候选 identity、真实 GUI 精简环境、Codex、Serena 与 CodeGraph binary discovery 已通过；CodeGraph candidate 为 `~/.local/bin/codegraph`，canonical target 为 `~/.codegraph/versions/v1.6.0/bin/codegraph`。
- [x] 上一 Finder 实例 PID 68522 已通过标准 `Cmd+Q` 完整退出：受管 Serena/Codex/9120/9121、Runtime、Execution 与 Claim 均有 termination/release evidence，结论不只依赖 PID 消失。
- [x] Scope boundary 已记录：当前 Workspace 的 CodeGraph readiness/query 为 `NOT_PREPARED`；Provider 等价只读 `status --json` 返回 initialized/complete、0 pending changes，但 extraction version 24 落后于当前 25 且 `reindexRecommended=true`。该状态不是 Phase 3 blocker；未执行 init/sync/index，后续 rebuild/live query 仅属 Local Human preparation/follow-up。
- [x] LaunchAgent registration roundtrip：macOS-only ignored Gate 使用与产品一致的 `MacosLauncher::LaunchAgent` + `--autostart`；Host 普通 Terminal 真实执行 `1 passed; 0 failed`，target/argument、disable 与 exact restore 全部通过，测试 plist 无遗留。
- [x] LaunchAgent 真实 job/environment/discovery 已完成：临时 label `io.github.lifei6671.serena-desktop.phase3-launchagent-gate` 以 `RunAtLoad=true`、`KeepAlive=false` 拉起 PID 97374，actual argv/environment、SHA、单实例和 dependency discovery 均为 `PASS`。窗口隐藏视觉语义属于 Phase 4/6，不阻塞 Phase 3。
- [ ] 当前轮外置 APFS Serena 安装为 `UNAVAILABLE`：DiskManagement/DiskArbitration 拒绝创建临时 image；失败资源已清理，既有 Codex 外置卷历史证据不等同于本轮安装 PASS。
- [ ] Phase 3 退出/无残留 Gate：PID 68522 的 Finder 标准 `Cmd+Q` 路径为 `PASS`，且旧临时 LaunchAgent Execution release evidence complete、Claim=0；但旧 cleanup 后 Serena PID 97403/`9121` 仍残留，正式 Runtime termination evidence 为 `unknown`。SIGTERM 修复候选的代码、自动化与静态 bundle Gate 已通过，真实 PASS 必须等 Host 定向清理旧 orphan 后，使用同一临时 LaunchAgent bootstrap/bootout 新候选并确认 Serena/Codex/ports/Runtime evidence 全部收口。
- [ ] Windows 实机 Job Object/Host Crash 继续为 `UNAVAILABLE/NOT_RUN`；自动化 fixture 不替代真机证据。

详细命令、失败根因与 Host Review 边界见 `.trellis/tasks/09-22-macos-p3-closeout/evidence.md`。父 Phase 3 的 Terminal/Finder/登录项 dependency discovery Acceptance 已满足；当前因新候选 bootout cleanup 待 Host 复验、真实外置 APFS 与 Windows 当前版本同步后的实机回归保持 `planning`。旧 orphan PID 97403 必须按已冻结 identity 定向清理，禁止按进程名全局 kill，也不得影响当前合法 Finder/Host 实例。LaunchAgent 隐藏窗口视觉分项留给 Phase 4/6，不阻塞 Phase 3；CodeGraph Workspace preparation 也不计入 blocker。

**退出条件：** 从 Finder 和登录项启动时，Codex、Git、uv、Serena 仍能稳定发现或给出可操作诊断；所有受管进程树可完整回收。

---

## 7. Phase 4：macOS 桌面集成

**预估：** 2～3 个工程日

**当前状态：** Phase 4A 已接入固定 `/usr/bin/open`、macOS Dock reopen 事件和最终 `RunEvent::Exit` shutdown 补偿；标准 Quit Apple Event 真机验证确认宿主、Serena broker 与 `9121` 监听一并退出。Phase 4B 已通过 AudioToolbox 接入用户首选警告音，并通过声音调用 Gate、完整 Rust Gate、arm64 `.app` 构建和 ad-hoc 签名验证。声音实际可听性、通知权限、点击激活和四种通知/声音组合仍需在当前 schema-subset Codex 兼容契约下重新执行真机矩阵；红色关闭按钮语义、single-instance、LaunchAgent、LAN plist、文件权限和 UI 文案仍待后续处理。

### 7.1 系统行为

- [x] macOS 使用 `/usr/bin/open` 打开 URL 和日志目录，Linux 仍使用 `xdg-open`。
- [x] 处理 macOS Dock reopen 事件，无可见窗口时显示并聚焦主窗口。
- [ ] 验证红色关闭按钮、隐藏到菜单栏和 `Cmd+Q` 的不同语义。
- [x] Dock、`Cmd+Q` 和菜单栏“退出”进入最终 `RunEvent::Exit` 时同步等待统一、幂等的 shutdown 流程完成。
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

## 8. Phase 5：macOS DMG、ad-hoc 签名与发布

**预估：** 2～4 个工程日

### 8.1 Tauri 配置

- [x] 创建 macOS 平台配置，避免直接将全局 `targets` 从 `nsis` 改成会影响 Windows 的值。
- [ ] 输出 Apple Silicon `arm64` `.app` 和 `.dmg`；首版不构建或发布 x86_64/Universal 产物。
- [ ] 配置 category、minimum system version、copyright、Info.plist 和实际需要的 entitlements。
- [ ] 保留 Hardened Runtime；使用 `signingIdentity: "-"` 进行 ad-hoc 签名，不添加未证明必要的 exception entitlement。
- [ ] 验证 `icon.icns` 在 Finder、Dock、菜单栏和 DMG 中的效果。
- [ ] 确认 `THIRD_PARTY_NOTICES` 在 macOS bundle 中的位置。

### 8.2 GitHub Actions

- [ ] CI 增加 macOS arm64 runner，执行前端和 Rust 全量质量 Gate。
- [ ] Release 拆分为 Windows x64 产物、macOS arm64 产物和统一发布阶段；首版不得生成 macOS x86_64 产物。
- [ ] 发布阶段仅上传经过验证的 NSIS/DMG 产物。
- [ ] 保留 tag-only、版本一致性、`--locked`、禁止 soft-fail 等现有契约。
- [ ] 为 workflow 结构更新 `scripts/ci-workflow.test.mjs` 和 `scripts/release-workflow.test.mjs`。

### 8.3 ad-hoc 签名与 Gatekeeper

- [ ] `.app` 及 bundle 内所有 Mach-O / 嵌套代码通过 `codesign --verify --deep --strict`，并确认使用 ad-hoc 签名。
- [ ] 首版不配置 Developer ID、App Store Connect、Notary Service 或 staple；这些能力在取得 Apple Developer Program 账户后另立任务。
- [ ] 发布说明和 README 明确声明产物未经过 Apple Developer ID 公证，不使用“Apple 已验证”等表述。
- [ ] 从浏览器下载正式候选 DMG，按真实 Gatekeeper 流程验证：首次尝试打开 → 系统设置“隐私与安全性” → “仍要打开” → 再次确认 → 后续正常启动。
- [ ] Gatekeeper 手工放行步骤作为正式安装流程文档化；不要求 `spctl` 将 ad-hoc 未公证产物判定为 Apple 已验证。

### 8.4 产物验证脚本

- [ ] 新增 macOS release verifier，检查 DMG 唯一性、版本、arm64 架构、文件大小、SHA-256、bundle identifier 和 ad-hoc 签名完整性。
- [ ] 检查 `.app` 的 bundle identifier 仍为 `io.github.lifei6671.serena-desktop`，版本与 Release Tag 一致。
- [ ] 检查 DMG 只包含唯一的 Serena Desktop.app 与 Applications 安装入口。
- [ ] Release Summary/Release Notes 输出 macOS 产物 filename、size 和 SHA-256，便于用户校验下载文件。
- [ ] 验证替换 `.app` 和删除 `.app` 不删除用户配置、任务数据库和日志。

**参考：**

- Tauri macOS 分发：<https://v2.tauri.app/distribute/>
- Tauri DMG：<https://v2.tauri.app/distribute/dmg/>
- Tauri macOS 配置：<https://v2.tauri.app/reference/config/#macconfig>
- Tauri macOS 签名：<https://v2.tauri.app/distribute/sign/macos/>
- Apple 本地网络隐私：<https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy>

**退出条件：** GitHub Release 可以稳定产出并验证唯一的 macOS arm64 ad-hoc 签名 DMG，安装文档准确覆盖 Gatekeeper 手工放行流程，Windows NSIS 产物与发布契约不回归。

---

## 9. Phase 6：双平台回归与人工验收

**预估：** 3～5 个工程日

### 9.1 自动化 Gate

- [ ] Windows 执行完整 lint/build/test/check/clippy/Rust test/NSIS policy Gate。
- [ ] macOS arm64 执行完整 lint/build/test/check/clippy/Rust test/DMG/ad-hoc signing Gate。
- [ ] 两个平台均验证 release tag 与 package/Cargo/Tauri 版本一致。
- [ ] `git diff --check` 通过，工作流中无 `continue-on-error` 或等价软失败。
- [ ] Release verifier 确认 Windows 仅发布 x64 NSIS、macOS 仅发布 arm64 DMG，不产生未承诺架构的产物。

### 9.2 macOS 真机验收

- [ ] 从 GitHub Release/浏览器下载正式候选 DMG，完成挂载、拖入 Applications 和首次 Gatekeeper 手工允许；记录预期的“未验证开发者/未公证”提示，不把该提示本身判为失败。
- [ ] 手工允许后可正常重复启动；首次启动、单实例、Dock reopen、菜单栏和优雅退出通过。
- [ ] Git、uv、Serena、Codex、CodeGraph 在 Terminal 启动和 Finder 启动两种情况下验证。
- [ ] Serena 安装、启动、停止、重启、索引和 Workspace 切换通过。
- [ ] Codex Agent 创建、正常完成、中断、取消、continue、应用崩溃和重启恢复通过。
- [ ] Quick Tunnel、ngrok、自建 HTTPS 和仅 MCP 模式通过。
- [ ] 通知权限、声音、LAN 权限、文件选择和外置卷通过。
- [ ] 覆盖含空格、中文、软链接和大小写差异的 Workspace 路径。
- [ ] 覆盖升级安装、删除应用和重新安装，验证用户数据保留。
- [ ] 在 macOS 12.0 或可信等价环境执行最低版本 Gate；若失败，只能提高 `minimumSystemVersion` 并同步文档/Release 要求。

### 9.3 文档

- [ ] README 增加 macOS Apple Silicon 下载、DMG 安装、Gatekeeper 手工允许和首次权限说明，并明确首版未经过 Apple Developer ID 公证。
- [ ] README 开发命令增加 macOS 前置依赖和验证命令。
- [ ] 状态页/设置页的 Windows 专属文案全部平台化。
- [ ] 记录 macOS Runtime 恢复能力与 Windows Job Object 的已知差异。
- [ ] 记录 Gatekeeper 手工允许、CLI 找不到、LAN 权限拒绝和残留进程的排查入口。

**最终退出条件：** 自动化 Gate 全部通过，macOS arm64 真机验收清单已完成，Windows 版回归通过，用户可以从 GitHub Release 下载 ad-hoc 签名 DMG，按已文档化的 Gatekeeper 手工放行步骤完成安装并正常使用。

---

## 10. 预估工作量

| 范围 | 粗略工作量 | 说明 |
|---|---:|---|
| 完整对齐 Windows | 4～7 个工程周 | 包含 Agent Runtime/Store 迁移、arm64 DMG/ad-hoc 发布链路与双平台真机验收 |
| 仅 Serena/MCP，不提供 Agent | 1～2 个工程周 | 属于功能降级版，不能按完整 macOS 版验收 |

工作量的最大变量是 Phase 2，而不是 UI 或 DMG 打包。若 Runtime 恢复契约发生实质变更，必须先更新设计文档和数据迁移方案，再重新估算。

---

## 11. 实施期不可退让项

- [ ] 不用“直接 child 已退出”代替“受管进程树已终止”证据。
- [ ] 不用 PID 单值作为跨重启 Runtime 身份。
- [ ] 不为了让 macOS 通过而释放证据不完整的 Workspace Claim。
- [ ] 不通过 shell string 启动 Codex、Serena 或 cloudflared。
- [ ] 正式 macOS Release 必须通过 ad-hoc `codesign` 完整性验证，并在 README/Release Notes 明确未经过 Apple Developer ID 公证及首次 Gatekeeper 手工放行步骤；不得声称 Apple 已验证。
- [ ] 不删除、跳过或弱化现有 Windows 验收 Gate。
- [ ] 不以覆盖率数字代替进程崩溃、重启恢复、PID 复用和真机分发测试。
