# Phase 3：CLI 发现与进程树

## Goal

完成 Finder/LaunchAgent 环境下的 Codex、Git、uv、Serena 发现与安装，并统一受管进程组生命周期。

## Requirements

- 依赖：Phase 2B Runtime 恢复与 State Store 集成已完成并归档。
- 在 Finder 和 LaunchAgent 环境中按固定顺序发现 Codex、Git、uv、Serena，不假设继承 shell dotfiles 的 `$PATH`。
- 验证常规文件、Unix execute bit、CPU 架构和版本兼容性，并返回可区分且不泄露敏感环境的诊断。
- 支持直接安装的 Codex 及 npm Darwin vendor binary，不通过 npm shell shim 启动 Runtime。
- Codex candidate 先完成 macOS ARM64 executable preflight，再通过共享的 App Server JSON Schema 必要子集兼容校验；version、binary SHA-256 与完整 schema SHA-256 只作为 identity/diagnostic evidence，不使用历史精确 allowlist 拒绝 schema-compatible 的后续版本。
- compatibility probe 只执行受管的 `--version` 与 `app-server generate-json-schema`，不启动 App Server；真实 execute/continue/cancel 仍必须通过正式 Runtime / Provider 产品 Gate。
- 冻结 macOS uv 获取、架构选择和摘要验证，移除 `winget`、`LOCALAPPDATA`、`uv.exe` 依赖。
- Serena、Workspace Runtime 和 cloudflared 使用独立 process group，并只回收应用拥有的进程树。

## Acceptance Criteria

- [x] Terminal、Finder 和登录项启动均能稳定发现依赖或给出可操作诊断；Codex discovery 在当前 schema-subset 契约下完成真实产品路径验证。
- [ ] 覆盖含空格、中文和外置卷的安装及 Workspace 路径。
- [ ] 超时、取消、应用退出和启动失败均完整回收受管进程树；Finder 标准退出路径已通过，旧临时 LaunchAgent cleanup 遗留 Serena orphan/`9121`、正式 Runtime termination evidence 为 `unknown`。macOS SIGTERM 已桥接到统一 shutdown authority 并通过自动化，但新候选尚待 Host 重跑同一 LaunchAgent bootstrap/bootout Gate。
- [x] 停止操作不会影响用户在 Terminal 中自行启动的相关进程。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.

## Phase 3 closeout evidence（2026-09-22）

- 当前 `codex-cli 0.155.1` ARM64 通过 host/candidate preflight、受管 `--version`、schema export、共享必要子集校验与正式 initialize/lifecycle；binary/schema hash 只作 evidence。真实 Product E2E 的 start 正常完成、continue、cancel、Claim 释放与 `macos_live_process_group_empty` 均通过。
- Host 已从 Finder 双击包含 Serena 与 CodeGraph Capability user-local fallback 的最新 ARM64 `.app`，并在完成 Finder Gate 后退出该实例。随后独立临时 LaunchAgent job 拉起同一 executable SHA `ae61…18c45`；PID 97374 的真实 argv 精确含 `--autostart`，环境为 `PATH=/usr/bin:/bin:/usr/sbin:/sbin`、`HOME=/Users/lifeilin`。StateStore/现场身份证明 canonical Codex 0.155.1 Runtime、Git 2.50.1、uv 0.12.17 candidate 与 Serena 1.7.0 user-local fallback 均为 `PASS`。CodeGraph 1.6.0 binary discovery 为 `PASS`；Workspace 索引因 `reindexRecommended=true` 为非阻塞 `NOT_PREPARED`。
- 六次 probe runtime 均完整收口。2026-09-22 Host 标准退出旧 PID 68522 后，Serena PID 68553 写入完整 shutdown 日志，Codex Runtime 写入 `terminated/complete/macos_live_process_group_empty`，三个 execution 的 release evidence 均 complete 且旧 Claim 为 0，该 Finder 路径为 `PASS`。Host 后续清理临时 LaunchAgent 时，service/plist/PID 97374 与旧 Codex identities 已消失，Execution release complete、Claim 为 0；但 Serena PID 97403 成为 PPID 1 的孤儿并继续监听 `9121`，正式 Runtime termination evidence 为 `unknown`。因此受管进程树生命周期 Acceptance 重新打开，不能仅凭主 PID 消失判定完成。
- 产品 `install_serena()` 在中文+空格隔离路径成功安装并验证官方 Serena 1.7.0；真实 start/restart/stop 与 Workspace capability 通过。当前 `/Volumes` 只有系统根卷别名，且 DiskManagement/DiskArbitration 不可用，没有安全真实外置 APFS 目标；本轮未再尝试 image，也未写入用户卷，外置卷安装保持 `UNAVAILABLE`。
- SIGTERM 根因已按最小边界修复：macOS-only Tokio signal listener 只接收 `SIGTERM`，通过 Tauri 主线程调用既有 `request_exit -> run_shutdown_once -> shutdown_impl`；不接管其他 signal，不改变 Windows 或 Serena/Codex ownership。完整 Gate 通过：当前 Rust `1082 passed; 0 failed; 25 ignored`，fmt/check/clippy 通过；`npm run tauri build` 内含 frontend production build 并成功生成新 ARM64 `.app`，executable SHA-256 为 `eb11a6ffc4f72037eae59f1aac13c33fd6e38b076e8e093b87f869c29d811ea0`。真实 bootout cleanup 复验仍为 `NOT_RUN`。
- macOS current-user LaunchAgent registration roundtrip ignored Gate 已由 Host 在普通 Terminal 执行通过：`enable=true`、plist target 为当前测试 executable、argument 精确为 `--autostart`、`disable=false`、最终 `exact_state=true`，`1 passed; 0 failed`。这只证明插件 registration 与恢复，不等于 launchd loaded job 或真实 LaunchAgent environment 启动。
- 父任务继续保持 `planning`。登录项 dependency discovery Acceptance 已满足，LaunchAgent registration roundtrip 与 job/environment/discovery 均为 `PASS`；主窗口隐藏视觉语义留给 Phase 4/6，不阻塞 Phase 3。旧 cleanup failure 不因源码修复自动改写；Host 需先定向清理已冻结身份的旧 orphan PID 97403（不得按进程名全局 kill），再用相同独立 label 对 SHA `eb11a6ff…11ea0` 候选执行 bootstrap/bootout，并确认 Serena/Codex/ports/Runtime evidence 全部收口。其余 blocker 为外置 APFS 本轮安装及 Windows 当前版本同步后的实机回归（跨平台 release safety evidence，不新增本 PRD Acceptance）。CodeGraph `rebuild_index` 仅为非阻塞 follow-up，不纳入 Phase 3 closeout。详细证据见子任务 `09-22-macos-p3-closeout/evidence.md`。
