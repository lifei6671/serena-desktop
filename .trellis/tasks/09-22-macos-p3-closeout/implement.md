# Phase 3 Closeout Implementation Plan

## 1. 冻结基线与 Authority

- [x] 核验四份 Host Authority 文档 SHA256。
- [x] 记录 branch、HEAD、dirty worktree、主机/工具链、Codex path/version/arch/hash。
- [x] 确认旧 allowlist 只作历史 evidence。

## 2. Codex schema-subset 与真实 Runtime Gate

- [x] 执行本机 Codex schema-only smoke，记录 version/binary/schema hash 与 ARM64 preflight。
- [x] 执行正式 Runtime lifecycle smoke，确认 initialize 与 `macos_live_process_group_empty`。
- [ ] 构建 arm64 `.app` 并取得 GUI 环境 discovery 后端证据；Finder 可见操作仍为 `NOT_RUN`。
- [ ] 真实 Product E2E 已执行 start/continue/cancel；可见 `.app` UI 路径仍为 `NOT_RUN`。

## 3. Serena 与路径 Gate

- [x] 验证固定 uv 与 Serena 1.7.0 真实安装/发现。
- [x] 验证 Serena start/stop/restart 和 Workspace capability。
- [x] 用隔离空格/中文路径运行真实安装与 capability。
- [ ] 临时 APFS image 创建被 DiskManagement/DiskArbitration 拒绝，记录为 `UNAVAILABLE`；失败资源已清理。

## 4. 所有权与退出收口

- [x] 执行可安全覆盖的 cancel/timeout/停止路径。
- [ ] 标准退出 `.app` 后核验受管 Serena/Codex/cloudflared、Broker 监听与产品进程无残留。
- [x] 确认未使用进程名全局终止，用户自行启动的同类进程不受影响。

## 5. 缺陷修复与回归

- [x] 严格 scope review 已撤回未产生通过证据的旧综合测试半修复；只保留三个直接支撑 Phase 3 Gate 的测试能力。
- [x] 修复 CodeGraph Capability Provider 在 macOS Finder 精简 PATH 下未复用 user-local candidate 的缺口，并增加 PATH 优先、fallback、三入口共享候选与非 macOS 编译边界回归。
- [x] 为 launchd `SIGTERM` 增加 macOS-only 安全 listener，通过 Tauri 主线程复用同一 `request_exit`，并增加可控异步/幂等 shutdown 回归。
- [x] rebuild 最新 ARM64 `.app` 并冻结 executable SHA；未在 Codex 沙箱内对 Host 执行 `bootout`。
- [ ] Host 定向清理旧 orphan PID 97403 后，对新候选重复同一临时 LaunchAgent bootstrap/bootout，确认 Serena/Codex/ports/Runtime evidence 全部收口。
- [x] 运行 `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`。
- [x] 运行 `cargo check --manifest-path src-tauri/Cargo.toml --locked`。
- [x] 运行 `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings`。
- [x] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --locked`。
- [x] 运行 `npm run lint`、`npm test` 与 `npm run build`。
- [x] 运行 `git diff --check` 并核对没有无关改动。

## 6. 证据与任务状态

- [x] 更新本任务 evidence、父 Phase 3 PRD和 `docs/macos-porting-checklist.md`。
- [x] Windows 实机项记录 `UNAVAILABLE/NOT_RUN`；真人观察项记录 `NOT_RUN`。
- [x] 仅在所有父退出条件真实满足时归档父任务；当前继续保持 `planning` 并列明阻塞。
- [x] 不提交、不推送，停在 Host Review 边界。
