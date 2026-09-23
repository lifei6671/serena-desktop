# Phase 3 收口与真实产品 Gate

## Goal

按当前 schema-subset compatibility contract 完成 macOS Phase 3 真实产品 Gate、进程树与路径证据收口，不扩展到 Phase 4/5。

## Requirements

- 以 Host 指定并已核验 SHA256 的 `docs/macos-porting-checklist.md`、父 Phase 3 PRD、平台父任务 PRD和 `docs/codex-agent-runtime.md` 第 49 节为当前 Authority；已归档任务中的精确 Codex hash allowlist 仅作为历史 evidence，不作为准入标准。
- Codex discovery 必须依次保持 macOS ARM64 host/candidate preflight、受管 `--version`、受管 `app-server generate-json-schema` 和 SerenaDesktop 必要 JSON Schema 子集校验；version、binary SHA-256 与完整 schema SHA-256 只记录为 identity/diagnostic evidence。
- 真实 Agent Gate 必须从 LaunchServices 启动的已构建 `.app` 产品路径执行，覆盖正常完成，并在不损坏用户数据且可明确判断的前提下覆盖 cancel/continue；schema probe 不得代替正式 Runtime/Provider Gate。
- 使用真实官方 Serena 1.7.0 覆盖安装/发现、启动、停止、重启和 Workspace capability；所有测试资源必须隔离于临时 Workspace/Runtime。
- 覆盖含空格、中文与外置 APFS 卷的可自动化路径；如创建临时磁盘映像，结束时必须卸载并删除镜像、挂载点及任务临时资源。
- 停止或退出后核验应用拥有的 Serena、Codex 与 cloudflared 进程树和相关监听端口无残留，不影响用户自行启动的同类进程。
- 若发现真实缺陷，只做完成 Gate 所需的最小修复并增加回归测试；不得弱化 Runtime evidence、Claim、Process Group、Windows Job Object 或兼容校验。
- Windows 实机 Gate 在当前 Mac 标记 `UNAVAILABLE/NOT_RUN`；需要视觉、听觉或其他真人判断的 Gate 保持未通过，不用自动化结论替代。
- 运行受影响测试以及完整 macOS Rust/frontend Gate，并把命令、结果、证据和明确限制写回本任务、父 Phase 3 PRD与 checklist。
- 不提交、不推送；Trellis 的提交步骤由本任务的明确用户约束覆盖，最终停在 Host Review 边界。
- 不开始 DMG、GitHub Actions、Phase 4 UI/LAN/通知矩阵或 Phase 5 工作。

## Acceptance Criteria

- [x] 本机 ARM64 Codex 通过平台/架构 preflight、`--version`、schema export 与共享必要子集校验；version/binary/schema hash 只作为 evidence。
- [x] LaunchServices 启动 `.app` 后，真实 Agent start 正常完成；可安全执行的 cancel/continue 已执行并记录，不能安全执行的具体原因明确记录为 `NOT_RUN`。
- [x] 真实 Serena 1.7.0 安装/发现、启动、停止、重启与 Workspace capability 通过产品或等价真实运行路径验证。
- [ ] 空格、中文和外置 APFS 卷路径的可自动化 Gate 通过，且所有临时卷、镜像和测试资源已清理。
- [x] 受管 Serena、Codex、cloudflared 在停止/退出后无残留；无关用户进程不受影响。Finder 标准退出与固定 LaunchAgent `SIGTERM` bootout 均已通过；旧 LaunchAgent main/Serena/Codex、process groups、`9120/9121`、Runtime/Execution/Claim 全部收口。
- [x] 受影响测试、完整 Rust check/clippy/test 与 frontend lint/test/build 通过；失败项有真实根因与状态，不伪造通过。
- [x] Windows 实机 Gate 为 `UNAVAILABLE/NOT_RUN`；已执行与仍待执行的真人观察 Gate 均按真实状态记录，不由自动化替代。
- [x] 使用独立临时 label/plist 在当前用户 `gui/$UID` domain bootstrap release bundle，验证真实 launchd job、`--autostart` argv、LaunchAgent PATH 与 Codex/Git/uv/Serena discovery；registration roundtrip、job/environment/discovery 与最终 `SIGTERM` bootout cleanup 均为 `PASS`。主窗口隐藏的视觉语义留给 Phase 4/6，不阻塞 Phase 3。
- [x] 父 Phase 3 只在全部退出条件真实满足时归档；否则保持 `planning` 并准确列出阻塞项。
- [x] Phase 3 文档证据已更新，未修改 Phase 4/5 实现，未提交或推送。

## Notes

- 当前父 Phase 3 已是 `planning`。Terminal、Finder 与登录项 dependency discovery 均为 `PASS`；LaunchAgent registration roundtrip、真实 launchd job/environment/discovery 与最终 `SIGTERM` bootout cleanup 也均为 `PASS`。最终固定 LaunchAgent 的旧 main/Serena/Codex PID `31903/31924/34479` 均为 `ESRCH`，对应 PGID 成员为 0，且不再占用 `9120/9121`；旧 Runtime `runtime-31903-1790128647025850-4`、Execution `execution-31903-1790129323401898-6` 与 Claim 均有完整 termination/release evidence。当前 Finder 新实例 owner 为 `40704/40730`、`hostId=host-40704-1790129915775196-1`，与旧 LaunchAgent identity 明确不同。CodeGraph Workspace readiness/query 仍为非阻塞 `NOT_PREPARED`；主窗口隐藏视觉语义留给 Phase 4/6。当前 blocker 仅为外置 APFS 路径 Gate（父 Acceptance 仍要求）和 Windows 同版本真机回归这项跨平台收口安全证据，因此父任务仍不可归档。
- 本任务不复用归档 Phase 3A 文档中的精确 hash allowlist 结论。
- 详细结果见 `evidence.md`。2026-09-22 Finder 标准退出 PID 68522 的 Serena/Codex/ports/Runtime/Execution/Claim 已完整收口；2026-09-23 固定 LaunchAgent bootout 后，内核身份、端口、Runtime evidence、Execution release 与 Claim 五组 Gate 也全部完成。Serena PID 31924 日志缺少正常 shutdown/`Finished server process` 文本，但该日志缺失不覆盖五组真实 Gate，不构成 cleanup failure。
- Finder/LaunchAgent 状态边界：Finder 路径、Host 普通 Terminal registration roundtrip/exact restore、独立临时 job identity/argv/environment/discovery 与最终 bootout cleanup 均为 `PASS`。固定 service 已不存在，临时 plist 仍保留供 Host 后续删除；当前 Finder owner 与旧 LaunchAgent identity 已明确区分。CodeGraph Workspace readiness/query 为 `NOT_PREPARED`；主窗口隐藏视觉语义属于 Phase 4/6。
- 外置卷状态：`/Volumes` 只有系统根卷别名，DiskManagement/DiskArbitration 当前不可用，没有安全真实外置 APFS 目标，Gate 保持 `UNAVAILABLE`。
- Serena discovery 修复边界：仅 macOS 在 PATH 未命中后回退 `~/.local/bin/serena`；explicit/managed 优先级、Windows/其他平台、版本、installer 与 Runtime 语义不变。focused、完整 Rust/frontend Gate 均通过。
- Finder 启动、LaunchAgent registration roundtrip、真实 launchd job environment、Codex/Git/uv/Serena discovery 与最终 bootout cleanup 均已通过。`--autostart` 隐藏主窗口留给 Phase 4/6。当前未满足项仅为外置 APFS 本轮安装及 Windows 同版本真机回归；CodeGraph Workspace `NOT_PREPARED` 与后续 `rebuild_index` 只作为非阻塞 preparation/follow-up。父任务继续 `planning`，本子任务继续 `in_progress`。
- SIGTERM 修复边界：仅 macOS 启用 `tokio::signal::unix::SignalKind::terminate()`；listener 不在 POSIX handler 中执行应用逻辑，而是通过 `AppHandle::run_on_main_thread` 调用既有 `request_exit -> run_shutdown_once -> shutdown_impl`。Windows、其他 signals、Serena/Codex Process Group、Runtime/Claim/StateStore 契约均未改变。完整 Rust Gate 为 `1082 passed; 0 failed; 25 ignored`，release `.app` 已构建；最终真实 cleanup Gate 为 `PASS`，`FINAL_SIGTERM_LAUNCHAGENT_CLEANUP=PASS`。
- 历史 `runtime-97374-1790086858581909-4` 的 `state=unknown`、`termination_evidence_state=unknown` 保持不变，作为旧候选 cleanup 的 fail-closed evidence；其 PID/PGID/ports 已无 live owner，因此不是当前 blocker。
