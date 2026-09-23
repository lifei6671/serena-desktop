# Phase 3 Closeout Evidence

记录时间：2026-09-22～2026-09-23（Asia/Shanghai）

## 1. 结论

本轮已完成当前 schema-subset compatibility contract 下的 Codex ARM64 discovery、正式 Runtime、真实 Product start/continue/cancel、官方 Serena 1.7.0 安装与独立生命周期、Workspace capability，以及完整 Rust/frontend Gate。包含 Serena 与 CodeGraph Capability user-local fallback 的最新 `.app` 已完成 Finder 与独立 LaunchAgent 两条真实启动路径；current-user LaunchAgent registration roundtrip、真实 launchd job/environment/discovery 与最终 `SIGTERM` bootout cleanup 均为 `PASS`。最终复核确认固定 service 已不存在、临时 plist 仍保留，旧 main/Serena/Codex identity `31903/31924/34479` 与对应 process group、`9120/9121` owner、Runtime/Execution/Claim 均完整收口；当前 Finder 新实例 owner 为 `40704/40730`、`hostId=host-40704-1790129915775196-1`，与旧 LaunchAgent identity 明确不同。历史 `runtime-97374-1790086858581909-4=unknown` 原样保留为 fail-closed evidence，但不是当前 live blocker。Serena 31924 日志缺少正常 shutdown 文本不改变五组 cleanup Gate 的完成结论。`--autostart` 主窗口视觉隐藏仍属于 Phase 4/6，不阻塞 Phase 3。当前 Phase 3 只剩外置 APFS 路径 Gate（父 Acceptance 仍要求）和 Windows 同版本真机回归这项跨平台收口安全证据；详见第 26 节。

不使用历史精确 Codex hash allowlist 作为准入标准。以下 version 与 hash 只记录为 identity/diagnostic evidence。

## 2. Codex discovery 与 Runtime

```text
host: macOS 26.5.2 (25F84), arm64
candidate: /Users/lifeilin/.local/bin/codex
canonical runtime: /Users/lifeilin/.codex/packages/standalone/releases/0.155.1-aarch64-apple-darwin/bin/codex
format: Mach-O 64-bit executable arm64
version: codex-cli 0.155.1
binary SHA-256: 8eaf1ad12fe6bf89b1710330f58900014322c7c5af677e43be116d8ac5fc0a9e
schema SHA-256: 058e9af9dc4ac3a39b3a382f14d117d5cd3beff329e2cbbf6929346f92eef1aa
```

- `real_compatible_macos_arm64_schema_smoke`：PASS。执行 host/candidate preflight、受管 `--version`、`app-server generate-json-schema --experimental --out` 和共享必要 schema 子集校验；未发送 App Server RPC。
- `real_compatible_macos_arm64_lifecycle_smoke`：PASS。首次使用默认 `CODEX_HOME` 因当前沙箱禁止写入用户 SQLite 而得到 `CODEX_STDIO_EOF`；改用无凭据的隔离临时 `CODEX_HOME` 后 initialize/lifecycle 通过，终止 evidence 为 `macos_live_process_group_empty`。这是执行环境限制，不是兼容性降级。
- `real_fixed_product_continuation_e2e`：scope review 后复跑 PASS，21.73 秒。真实 `codex-cli 0.155.1` 完成 start 正常结束、同 thread continue、长任务 cancel、Claim 释放与 Runtime 收口；凭据只复制到自动删除的测试临时目录，未写入日志或 evidence。

## 3. `.app` 产品路径

- `npm run tauri build`：PASS，生成 `src-tauri/target/release/bundle/macos/Serena Desktop.app`。
- bundle executable：ARM64 Mach-O；bundle ID `io.github.lifei6671.serena-desktop`；`codesign --verify --deep --strict` PASS；本地 ad-hoc 签名。DMG、Developer ID 和 notarization 属于 Phase 5，本轮未开始。
- 系统 `open`/LaunchServices 在当前受管 shell 中返回 `kLSNoExecutableErr`，且 `mdls`/`lsregister` 无法读取 workspace；Computer Use 绑定请求没有获得 UI 控制，但实际启动了本轮刚构建 bundle。`lsof` 冻结证据确认 PID 38998 的 executable 精确来自上述 `.app`，并监听 `127.0.0.1:9120`。
- 产品数据库确认 GUI 环境发现 canonical Codex，记录 `codex-cli 0.155.1` 与上述 schema hash；六次 compatibility/probe runtime 均为 `terminated/complete/macos_live_process_group_empty`。
- 本节初始取证时，旧产品进程仍有一个池化 runtime 记录为 `running/unknown`，且 Finder 双击、LaunchAgent 和可见 UI 真人流程均为 `NOT_RUN`。后续证据已确认该 runtime 转为 `terminated/complete/macos_live_process_group_empty`，Finder 启动、真实 Agent lifecycle 与 PID 68522 标准退出均已通过；LaunchAgent job/environment/discovery 也已通过。`--autostart` 窗口隐藏缺视觉证据的分项继续留给 Phase 4/6，不阻塞 Phase 3；临时 LaunchAgent cleanup 的独立失败见第 21 节。

## 4. Serena 1.7.0 与路径

- 已有官方安装发现：`/Users/lifeilin/.local/share/uv/tools/serena-agent/bin/serena`，`Serena 1.7.0`。
- `official_macos_serena_install_matches_pinned_version`：scope review 后复跑 PASS，14.25 秒。经产品 `install_serena()` 在 `/tmp/Serena P3 scope 中文 空格.*` 隔离路径安装固定 Serena，安装后 executable/version/capability 检查通过；缓存与安装目录已删除。
- `official_serena_start_restart_stop`：scope review 后复跑 PASS，5.23 秒。Gate 已从 Broker 集成测试下沉到 `serena.rs`，直接验证 `SupervisorState`；真实 start 后 `Running`，restart 后 PID 变化，stop 后 `Stopped`、`managed_process_present=false`、`process_id=None`。测试在读取断言前先执行正式 stop，start/restart 返回错误时也会先尝试收口。
- `official_serena_first_acquire_auto_creates_project_configuration`：PASS。真实 Workspace capability 首次 acquire 自动创建 `.serena/project.yml`，相关临时 runtime 已清理。
- 旧综合 `official_serena_lifecycle` 曾尝试把固定 `project-1/project-2` 改为随机稳定 Workspace ID，但隔离 uv/cache 后仍在后段以 `WORKSPACE_CONTEXT_REQUIRED` 失败。严格 scope review 判定该改动没有支撑任何已通过 Phase 3 Gate，已完整撤回；`src-tauri/src/mcp/mod.rs` 相对 HEAD 为零 diff，不留下半修复。Phase 3 所需安装、独立生命周期与 capability 只由上述窄 Gate 提供证据。
- 临时 APFS sparse image 创建被当前环境的 DiskManagement/DiskArbitration 拒绝，错误为 `hdiutil: create failed - 设备未配置`；未产生可挂载镜像，临时目录已删除。当前轮外置卷安装 Gate 为 `UNAVAILABLE`。checklist 中既有外置卷 Codex 历史证据不被改写为本轮 Serena 安装 PASS。

## 5. 进程与清理状态

- Codex schema/lifecycle/Product E2E、成功的 Serena install/start/restart/stop/capability Gate 均使用隔离资源并完成自身清理。
- Host 处理前，一次早期窄 Serena 测试误用 `Broker::stop()`，断言后留下本任务创建的官方 Serena 独立 session leader PID 18311、监听 `127.0.0.1:57958`。测试已修正为直接调用 `SupervisorState::stop()`，复跑通过；当时沙箱拒绝向已脱离测试父进程的 group 发信号。
- Host 处理前，`.app` PID 38998 与其池化 Codex runtime/9120 也因相同权限边界尚未完成标准 Quit。
- 未发现本任务启动的 cloudflared。未按进程名执行全局 kill，也未影响用户自行运行的 Codex/Serena/cloudflared。
- 上述内容是 Host 处理前的历史状态。2026-09-22 复核已确认 PID 38998/18311、端口 57958 和旧 runtime PGID 4079 均无残留，旧 Runtime/Claim 也已完整收口；该特定 FAIL/BLOCKED 已解除，详见第 10 节。完整 Phase 3 仍因其他未执行 Gate 不可归档。

## 6. 自动化 Gate

```text
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check                 PASS
cargo check --manifest-path src-tauri/Cargo.toml --locked                PASS
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
                                                                          PASS
cargo test --manifest-path src-tauri/Cargo.toml --locked                  PASS
                                                                          1080 passed; 0 failed; 25 ignored
npm run lint                                                             PASS
npm test                                                                 PASS
                                                                          117 passed; 0 failed
npm run build                                                            PASS
git diff --check                                                         PASS
```

scope review 后第一次完整 Rust 回归中，既有 `bounded_command_timeout_reaps_descendant_process_group` 因 `leaf.pid` 尚未出现而瞬态失败；未修改该测试，单独重跑通过，随后第二次完整 Rust 回归全绿。本次 Serena discovery 修复后的第一次完整回归再次在同一 `leaf.pid` 读取点瞬态失败（其余 `1076 passed`），未修改或弱化该测试；定向复跑 PASS，随后当时的 `cargo test --locked --quiet` 得到 `1077 passed; 0 failed; 24 ignored`。新增 LaunchAgent ignored Gate 后，本次完整回归为上述 `1080 passed; 0 failed; 25 ignored`。

## 7. 明确 PARTIAL / NOT_RUN / UNAVAILABLE

- Finder/GUI discovery：分项状态见第 17 节。Host 明确 Finder 双击来源、新候选 executable 身份、GUI 精简 PATH、Codex 0.155.1、Serena 1.7.0 user-local fallback 与 CodeGraph binary discovery 均为 `PASS`；CodeGraph Workspace readiness/query 因 `reindexRecommended=true` 为 `NOT_PREPARED`，不得误记为 discovery failure 或 query PASS，且不属于 Phase 3 退出 blocker。
- LaunchAgent registration roundtrip：`PASS`。Host 在普通 Terminal 完成 `enable → plist target/argument → disable → exact restore`，`1 passed; 0 failed`；测试前后均为 disabled，测试专用 plist 未遗留，见第 18 节。
- LaunchAgent 真实 job/environment/discovery：`PASS`。固定临时 job 已由 launchd 拉起 PID 97374，job identity、真实 argv、精简 PATH/HOME、executable SHA、单主实例与 Codex/Git/uv/Serena discovery 均通过。`--autostart` 隐藏主窗口缺少视觉证据，但该视觉语义属于 Phase 4/6，不阻塞 Phase 3，见第 20 节。
- PID 68522 Finder 实例的标准退出、Serena/Codex/端口、Runtime evidence 与 Claim cleanup：`PASS`，见第 17 节。第 21 节保留旧候选 cleanup failure 的历史证据；最新固定 LaunchAgent 的 service/main/children/process groups/ports/Runtime/Execution/Claim 已在 bootout 后全部收口，最终 Gate 为 `PASS`，见第 26 节。Phase 6 release candidate 仍需完整退出验收。
- 外置 APFS 本轮 Serena 安装：`UNAVAILABLE`（DiskManagement/DiskArbitration）。
- Windows 当前版本同步后的实机 Job Object、Host Crash、共享兼容性回归：同步前保持 `UNAVAILABLE/NOT_RUN`。
- 通知、声音、LAN、DMG、GitHub Actions、notarization 与 Phase 4/5 矩阵：不在本任务范围，未开始。

## 8. 严格 scope review 与最终代码边界

严格 scope review 后保留四个显式 ignored/测试专用 Gate，并新增两个 Phase 3 必需的窄生产修复：

- `src-tauri/src/agent/product/tests.rs`：让既有真实 Product E2E 在 macOS 编译运行；macOS 从用户 `HOME` 读取 auth 到自动删除的临时 `CODEX_HOME`，evidence 也限定在临时目录。保留原因是它直接产生 start/continue/cancel、Claim 与 Runtime 收口证据。
- `src-tauri/src/installer.rs`：通过产品 `install_serena()` 安装并验证官方 Serena 1.7.0。保留原因是它直接产生真实受管安装证据。
- `src-tauri/src/serena.rs`：直接通过 `SupervisorState` 验证 start/restart/stop，不再依赖 Broker、Workspace 或 command wrapper。保留原因是它直接产生独立 Serena 生命周期证据，并显式在断言前收口。
- `src-tauri/src/autostart.rs` 与 `src-tauri/src/lib.rs`：仅在 macOS test build 编译 autostart 测试模块，新增独立测试 app name 的 LaunchAgent registration roundtrip ignored Gate。Gate 使用与产品相同的 `MacosLauncher::LaunchAgent` 和 `--autostart`，验证 plist `Label`/`ProgramArguments`，并对测试自己的 plist 做字节级快照恢复；产品 plugin、command、single-instance、LaunchAgent 配置和 UI 均未改变。
- `src-tauri/src/discovery.rs`：仅在 macOS 将原有 PATH candidate 扩展为 `find_executable("serena").or_else(|| user_local_candidate("serena"))` 的等价窄 helper；Windows 与其他平台仍只使用原有 PATH discovery。explicit、managed、`InstallationSource`、版本与 installer 语义均未改变。
- `src-tauri/src/codegraph_capability.rs`：让 installation probe、status/prepare runner 与 direct MCP Runtime starter 共享同一窄候选 helper；仅 macOS 在 PATH 未命中后尝试 `$HOME/.local/bin/codegraph`，其他平台保持 `which_command` 语义。Runtime/Workspace/stop/error 契约未改变，详见第 15 节。

已删除/撤回：`src-tauri/src/mcp/mod.rs` 的窄生命周期测试、旧综合测试的随机 Workspace ID 迁移及相关替换。它们没有带来通过证据，且会把 Phase 3 closeout 扩大到过期 Broker/Workspace context 语义。

此前 closeout 代码 diff 为上述 5 个文件，`276 insertions / 30 deletions`；本次只追加上述 2 个 test-only 文件改动。没有 Phase 4/5、产品 autostart 或 Windows Runtime/Job Object 语义修改。

## 9. Host Review 边界

未提交、未推送。Finder 与 LaunchAgent discovery、registration roundtrip 及最终 bootout cleanup 均已闭环；第 21 节旧候选 cleanup failure 与 `runtime-97374-1790086858581909-4=unknown` 继续作为历史 fail-closed evidence，不追溯改写。第 26 节的最终真实复核确认旧 LaunchAgent main/Serena/Codex、process groups、ports、Runtime、Execution 与 Claim 全部收口，当前 Finder owner 是不同 host identity。`--autostart` 隐藏窗口的视觉证据移交 Phase 4/6，不阻塞 Phase 3。当前真实 blocker 仅为外置 APFS 路径 Gate（若父 Acceptance 继续要求）及 Windows 同版本真机回归。

## 10. Host cleanup 复核（2026-09-22 20:42 +08:00）

本节是只读复核；没有发送 signal、调用 kill/quit、修改数据库或变更产品代码。

### 10.1 旧目标与当前实例身份冻结

- 旧 PID 38998：不存在；旧 PID 18311：不存在；`127.0.0.1:57958`：无监听。
- 旧 runtime PGID 4079 与旧测试 PGID 18311 的当前成员集合均为空。该现场结果只用于交叉核验；旧 Runtime 的完成结论以 StateStore 中已持久化的终止 evidence 为准，不以 PID/PGID 当前为空代替。
- 当前合法 `.app` 为 PID 39638，PPID 1、PGID 39638、SID 1，启动时间 `2026-09-22T20:13:43.972+08:00`，start token `darwin_proc_bsd_start_v1:1790079223:972671`，executable 为 `src-tauri/target/release/bundle/macos/Serena Desktop.app/Contents/MacOS/serena-desktop`。其 PGID 当前成员为 `[39638]`，监听 `127.0.0.1:9120`。
- 当前合法 Codex Runtime 为 PID 50661，PPID 39638、PGID/SID 50661，启动时间 `2026-09-22T20:37:58.291+08:00`，start token `darwin_proc_bsd_start_v1:1790080678:291869`，executable 为 `~/.codex/packages/standalone/releases/0.155.1-aarch64-apple-darwin/bin/codex`。该身份与 StateStore 当前 runtime 精确匹配，不能误判为旧残留。

### 10.2 StateStore reconciliation 与 Claim

- 旧 runtime `runtime-38998-1790076994361320-4` 属于 `host-38998-1790062364657158-1`：`state=terminated`、`termination_evidence_state=complete`、`termination_evidence_type=macos_live_process_group_empty`、`termination_evidence_at=2026-09-22T20:13:09.625+08:00`、`stopped_at` 同时刻；记录的 PID/PGID/SID 均为 4079，process start token 为 `darwin_proc_bsd_start_v1:1790076994:632307`。
- 该终止 evidence 在当前新 `.app` 启动前约 34 秒写入，属于 live shutdown 的 process-group-empty 证据，不是仅凭 PID 不存在推断，也不是人工数据库修补或 `macos_recovered_process_group_empty`。
- 旧 host 的所有 runtime 非终态计数为 0。旧 runtime 关联的两个 execution 均为 `completed`，`release_evidence_state=complete`、`release_evidence_kind=same_runtime_cleanup`，对应 Workspace Claim 均不存在。
- 当前 runtime `runtime-39638-1790080678028051-3` 属于新 host `host-39638-1790079225086982-1`，PID/start token/PGID/SID 与现场 PID 50661 完全一致。其 `state=running`、`termination_evidence_state=unknown`、当前 execution/Claim 存在，是本次复核时仍在运行的合法 Agent；活动 runtime 尚无终止 evidence 是预期状态，不是旧实例 fail-closed 残留。

### 10.3 当前 owner、capability 与监听

- 当前产品 owner 只有 PID 39638；其当前 Agent/Codex owner 为 PID 50661。现场子树中的 `node_repl`、`node`、`SkyComputerUseClient` 与 `codex-code-mode-host` 都可沿 PPID/SID 回溯到该当前 Codex Runtime；未发现上一轮 host 38998、旧 PGID 4079 或旧测试 PID/PGID 18311 的成员。
- 当前 `127.0.0.1:9120` 仅由新 `.app` PID 39638 监听。`9121` 在复核快照时无监听；产品日志显示官方 Serena 1.7.0 曾正常启动并 ready，随后于 `2026-09-22 15:03:52` 正常 shutdown，因此这不是旧残留，也不伪造成当前 Serena 正在运行。
- 当前未发现 `serena-agent` Workspace capability/runtime 进程或 `cloudflared` 进程/监听。当前没有活动 capability/tunnel 时，“不存在”是合法状态；既有 Workspace capability Gate 的通过证据仍来自第 4 节，不由本次空闲快照替代。
- 结论：上一轮标准 shutdown/Host cleanup 后的残留集合已复核清空，旧 Runtime evidence 与 Claim 均可解释且 fail-closed 契约未被绕过。Finder/LaunchAgent、外置 APFS 与 Windows 真机状态没有因本节而改变。

## 11. Finder/LaunchAgent 与真实外置卷复核（2026-09-22）

本节只使用只读系统查询、现有自动化测试与一个未能注册的隔离 LaunchAgent diagnostic；没有停止当前 PID 39638、修改真实登录项、写入外置卷或变更产品代码。

### 11.1 当前 GUI application 环境：`PARTIAL`

- PID 39638 身份再次冻结为 PPID 1、PGID 39638、SID 1，executable 为 release bundle 内 `Serena Desktop.app/Contents/MacOS/serena-desktop`，启动时间 `2026-09-22T20:13:43.972671+08:00`，start token `darwin_proc_bsd_start_v1:1790079223:972671`。
- `launchctl print gui/501/application.io.github.lifei6671.serena-desktop.29148420.29148433` 将它标识为 RunningBoard 管理的 `spawn type = app`、`spawn role = ui`，working directory 为 `/`，参数只有 bundle executable，没有 `--autostart`。
- 进程实际环境为 `PATH=/usr/bin:/bin:/usr/sbin:/sbin`、`HOME=/Users/lifeilin`、`SHELL=/bin/zsh`。PATH 不含 `~/.local/bin`、`/opt/homebrew/bin` 或 npm 路径，属于典型 GUI/LaunchServices 精简环境；但进程来源无法区分 Finder 双击、`open` 或其他 LaunchServices 请求，因此不记 Finder 双击 PASS。
- StateStore 当前 runtime `runtime-39638-1790080678028051-3` 记录 canonical executable `~/.codex/packages/standalone/releases/0.155.1-aarch64-apple-darwin/bin/codex`、`codex-cli 0.155.1` 与当前 schema hash，现场 PID/start token/PGID/SID 也匹配。由此可真实证明该 GUI-like 环境发现并运行了 canonical Codex。
- 现有回归 `finder_minimal_path_uses_fixed_candidate`、`codegraph_candidate_falls_back_to_user_local_bin`、`macos_uv_candidates_do_not_depend_on_shell_profiles` 均 PASS。使用同一精简环境显式运行三个用户目录候选的 `--version` 也分别得到 `codex-cli 0.155.1`、CodeGraph `1.6.0`、Serena `1.7.0`；这只证明候选本身可执行，不替代产品 discovery。
- CodeGraph 生产路径有 `~/.local/bin/codegraph` fallback，当前候选 canonicalize 到 `~/.codegraph/versions/v1.6.0/bin/codegraph`。但读取当前 UI snapshot 的 Computer Use 未获批准，Mac MCP capability 调用也因当前 approval policy 拒绝，因此本次不把当前实例的 live CodeGraph snapshot 记为 PASS。
- 诊断时的 Serena 边界不同：当前配置 `serenaPath=null`，产品 managed path `~/Library/Application Support/io.github.lifei6671.serena-desktop/runtime/bin/serena` 不存在，精简 PATH 又不含现有 `~/.local/bin/serena`；当时 `discovery::detect()` 只检查显式配置、managed path 与 PATH。因此当前仍在运行的旧会话没有成功发现用户目录 Serena 的证据，且 `9121` 未监听。该源码缺口已在第 12 节最小修复，并已构建为第 13 节候选；未由 Host 标准退出/重启前不能改变这一 live evidence。

### 11.2 LaunchAgent/login item：当时 `NOT_RUN`（隔离 diagnostic `UNAVAILABLE`）

- `~/Library/LaunchAgents`、`/Library/LaunchAgents`、当前 `launchctl gui/501` domain 与 `sfltool dumpbtm` 均无 Serena Desktop LaunchAgent/login item；当前唯一 Serena job 是上述普通 GUI application job，且无 `--autostart`。因此没有可以复核的真实登录启动实例。
- 产品已初始化 `MacosLauncher::LaunchAgent` 并配置 `--autostart`，但初始化插件不等于用户已启用或系统已执行登录项。
- 为避免影响 PID 39638，创建了仅输出非敏感 `PATH/HOME` 并执行现有三个 CLI `--version` 的独立临时 plist；`launchctl bootstrap gui/501` 返回 `Bootstrap failed: 5: Input/output error`。服务未注册、无进程、无输出文件，临时 plist 已删除。没有尝试提升权限或改写真实 login item。
- 当时结论：LaunchAgent 实际登录启动为 `NOT_RUN`，受管环境中的隔离 bootstrap 能力为 `UNAVAILABLE`；现有 minimal-PATH 单元测试和 GUI application 证据只能提供 `PARTIAL` 支撑。Host 后续已在普通 Terminal 完成独立临时 job bootstrap，第 20 节覆盖最新状态。

### 11.3 真实外置 APFS：`UNAVAILABLE`

- `/Volumes` 仅有 `Macintosh HD -> /`，与根卷 device identity 相同；没有其他已挂载卷。它是系统卷别名，不是可用的外置测试目标。
- `diskutil list external physical`、`diskutil list` 与 `diskutil info -plist` 均因当前环境无法使用 DiskManagement/DiskArbitration 而失败；`mount`、`df`、`stat` 的只读交叉检查也只看到根卷。
- 因此没有明确非系统关键、可写的真实外置 APFS 卷。本轮没有创建测试目录、没有写入或删除任何卷上用户数据，也没有再次尝试 `hdiutil` image。外置卷 Serena install/path capability Gate 保持 `UNAVAILABLE`。

### 11.4 当前 Phase 3 剩余 blocker

- 真实启用的 LaunchAgent 登录启动；不得由 Finder/RunningBoard GUI application 证据替代。
- 真实非系统外置 APFS 卷上的 Serena install/path capability Gate。
- Windows 当前版本同步后的实机回归；作为跨平台 release safety evidence 跟踪，不新增父 Phase 3 PRD Acceptance。

## 12. macOS Serena user-local discovery 修复（2026-09-22）

### 12.1 最小实现边界

- `detect()` 的优先级仍是 explicit `serena_path` → `managed_serena` → PATH candidate；只把 macOS 的第三层 candidate 收窄扩展为 PATH 未命中时回退 `user_local_candidate("serena")`。
- Windows 与其他平台的 helper 分支仍直接调用 `find_executable("serena")`；没有改变其候选顺序或返回语义。
- 没有改变 `InstallationSource`、`supported_version()`、`inspect_candidate()`、installer、Runtime、Process Group、Claim 或 Windows Job Object。
- 新增 `macos_serena_path_candidate` 只表达 PATH 优先、user-local 后备，不引入通用 discovery abstraction。

### 12.2 自动化证据

- 新测试 `macos_serena_candidate_falls_back_to_user_local_after_path`：PASS；覆盖 Finder-style PATH 未命中时选择 `~/.local/bin/serena`，以及 PATH 命中时优先 PATH candidate。
- 既有 `discovery_preserves_explicit_path_and_managed_precedence`：PASS；继续证明 explicit 与 managed 优先级未被 helper 改写。
- focused `cargo test --locked discovery::tests::`：`20 passed; 0 failed`。
- 完整 Gate：fmt/check/clippy PASS；Rust `1077 passed; 0 failed; 24 ignored`；frontend `117 passed; 0 failed`；build 与 `git diff --check` PASS。

### 12.3 Live Gate 边界

- PID 39638 在修复前已启动，内存中的 discovery 逻辑来自旧 build；源码、单测和后来写入同一路径的新 bundle 均不能追溯改变该历史运行会话。
- 第 14 节已确认 Host 标准退出旧会话并从 Finder 启动新候选；新实例在相同精简 PATH 下通过 user-local fallback 启动官方 Serena 1.7.0。因此 Serena GUI discovery 已从本节的历史 `PARTIAL` 提升为 `PASS`。该阶段尚未完成的新实例退出随后已由 PID 68522 的真实标准 `Cmd+Q` 在第 17.2 节闭环。

## 13. 包含 Serena user-local fallback 的 `.app` 构建候选（2026-09-22）

### 13.1 构建身份

- `npm run tauri build`：PASS；Tauri release build 与 bundle 签名完成，生成 `src-tauri/target/release/bundle/macos/Serena Desktop.app`。构建命令完成取证时间为 `2026-09-22T21:05:06+08:00`，bundle 与主 executable mtime 均为 `2026-09-22T21:04:52+08:00`。
- 本次构建使用当前工作区 `src-tauri/src/discovery.rs`，其 SHA-256 为 `1d32407269a74d89f4291fb9264137597765388e26c55f8d38e4c6db4099be3c`；该源码包含第 12 节已经完整 Gate 验证的 macOS `find_executable("serena").or_else(|| user_local_candidate("serena"))` 语义。因此该 bundle 是包含 `~/.local/bin/serena` fallback 的新候选，不是此前 PID 39638 启动时的旧 build。
- bundle 主 executable 为 `Contents/MacOS/serena-desktop`，SHA-256：`f83d633a992619810e60cb20c751333ef6444851a9faf69daf16a829f9e948da`。

### 13.2 静态 bundle Gate

- `file` 与 `lipo -archs`：PASS；`Mach-O 64-bit executable arm64`，且唯一架构为 `arm64`。
- `CFBundleIdentifier`：`io.github.lifei6671.serena-desktop`；`CFBundleExecutable`：`serena-desktop`；`LSMinimumSystemVersion`：`12.0`。
- `codesign --verify --deep --strict --verbose=2`：PASS，bundle `valid on disk` 且满足 Designated Requirement。
- `codesign -dvvv`：`Signature=adhoc`、`TeamIdentifier=not set`；没有将本地 ad-hoc 候选误记为 Developer ID 或 notarized 产物。DMG、notarization 与 Phase 5 未执行。

### 13.3 运行实例边界

- 构建后只读复核 PID 39638：PPID 1、PGID 39638、SID 1、启动时间 `2026-09-22T20:13:43.972671+08:00`、start token `darwin_proc_bsd_start_v1:1790079223:972671`，与构建前冻结值一致。
- 本轮没有向 PID 39638 发送 signal、调用 quit 或把它作为新候选重启。虽然新 bundle 已写入同一路径，PID 39638 仍是构建前启动的旧运行会话，其既有 Runtime/discovery evidence 保持不变，不能作为新 fallback 的产品实机 PASS。
- Host 已在本节之后标准退出 PID 39638，并从 Finder 双击启动上述候选；第 14 节记录新实例的 identity 与 discovery 证据。后续 PID 68522 的真实标准退出已闭环 Phase 3 shutdown lifecycle。本节当时的剩余 blocker 包含 LaunchAgent、外置 APFS 与 Windows；LaunchAgent 后续结果见第 20 节。CodeGraph live snapshot 作为 discovery 修复证据跟踪，不新增 Phase 3 退出条件。

## 14. Finder 真人启动与新候选 live discovery 复核（2026-09-22 21:20 +08:00）

本节以 Host 明确确认“在 Finder 中直接双击最新构建的 `Serena Desktop.app`”作为唯一启动来源的人类观察证据；其余结论来自只读进程、`launchctl`、文件、日志、监听与 StateStore 查询。本轮没有启动/停止 Serena capability、Codex Runtime 或用户业务，没有发送 signal、调用 quit、修改数据库或改动产品代码。

### 14.1 Finder 实例身份：`PASS`

- 当前仅有一个 bundle GUI 主进程：PID 68522、PPID 1、PGID 68522、SID 1，启动时间 `2026-09-22T21:15:23.163681+08:00`，start token `darwin_proc_bsd_start_v1:1790082923:163681`。启动时间晚于候选构建完成取证时间 `2026-09-22T21:05:06+08:00`。
- executable 为 `src-tauri/target/release/bundle/macos/Serena Desktop.app/Contents/MacOS/serena-desktop`，现场 SHA-256 为 `f83d633a992619810e60cb20c751333ef6444851a9faf69daf16a829f9e948da`，与第 13 节候选完全一致。
- RunningBoard job `application.io.github.lifei6671.serena-desktop.29285962.29285976` 为 `state=running`、`bundle id=io.github.lifei6671.serena-desktop`、`spawn type=app`、`spawn role=ui`，program/唯一 argument 均为上述 bundle executable，working directory 为 `/`，没有 `--autostart`。结合 Host 的 Finder 双击观察，Finder 启动来源与新候选身份通过；这不构成 LaunchAgent 登录启动证据。

### 14.2 GUI 环境：`PASS`

- `launchctl print pid/68522` 与具体 RunningBoard job 均给出进程自身环境：`PATH=/usr/bin:/bin:/usr/sbin:/sbin`、`HOME=/Users/lifeilin`、`SHELL=/bin/zsh`、`TMPDIR=/var/folders/sq/9tr34l617h5fp65hjxgvly140000gn/T/`。这是真实 GUI job 环境，不是当前 shell 环境。
- PATH 不含 `~/.local/bin`、Homebrew 或 npm 路径，符合 Finder/LaunchServices GUI 精简环境。

### 14.3 Codex 与 Serena discovery：`PASS`

- StateStore 当前 host 为 `host-68522-1790082923987239-1`。其正式 Runtime `runtime-68522-1790082987477220-3` 记录 canonical executable `~/.codex/packages/standalone/releases/0.155.1-aarch64-apple-darwin/bin/codex`、`codex-cli 0.155.1`、schema SHA-256 `058e9af9…`，PID/PGID/SID 68732 与 start token `darwin_proc_bsd_start_v1:1790082987:671031` 均和现场一致。该 Runtime 为当前合法 owner，`state=running`、termination evidence `unknown` 且当前 execution/Claim 存在，是活动业务的预期 fail-closed 状态，不是残留。
- 当前配置 `serenaPath=null`；managed candidate `~/Library/Application Support/io.github.lifei6671.serena-desktop/runtime/bin/serena` 不存在；GUI PATH 又不含 `~/.local/bin`。因此 explicit、managed 与 PATH 三种命中均被现场证据排除。
- `~/.local/bin/serena` 是指向 `~/.local/share/uv/tools/serena-agent/bin/serena` 的 symlink。当前主进程于 `21:15:25.658522` 启动 PID 68553（PPID 68522、PGID/SID 68553）；该进程实际加载 `~/.local/share/uv/tools/serena-agent/lib/python3.13/site-packages`，日志明确记录 `version=1.7.0`、PID/PPID 与 macOS ARM64，`127.0.0.1:9121` 正常监听。结合新候选源码的唯一剩余 candidate 分支，可确定本次命中的是刚修复的 `~/.local/bin/serena` fallback 及其 canonical target，而非 explicit、managed 或 PATH 偶然命中。
- 产品主 broker `127.0.0.1:9120` 由 PID 68522 监听，Serena `9121` 由 PID 68553 监听。Host 明确观察 Mac MCP 已恢复；自动 MCP Workspace 只读调用因当前 approval policy 为 `never` 被 Host 层拒绝，未把该策略拒绝误记为产品失败，也未绕过策略启动额外 capability。

### 14.4 CodeGraph 与当前 owner 边界

- `~/.local/bin/codegraph` 可执行并返回 `1.6.0`，UI discovery 的 macOS user-local fallback 回归通过；但 Host 随后从当前 Finder 实例实际调用公开 `codegraph_explore`，返回 `CODEGRAPH_RUNTIME_START_FAILED`。
- 失败根因已定位到 `src-tauri/src/codegraph_capability.rs`：`discover_installation`、`run_command` 与 `start_runtime` 分别重新调用 `rmcp::transport::which_command("codegraph")`。当前 GUI `PATH=/usr/bin:/bin:/usr/sbin:/sbin` 无法解析 `~/.local/bin/codegraph`，因此 UI discovery 与 Capability Runtime 使用了不同候选逻辑。
- 因此修复前当前 Finder 实例的 CodeGraph live 产品 Gate 为 `FAIL`，不是 `PARTIAL` 或 `PASS`；失败发生在 direct MCP child 启动前，没有形成可持有的 CodeGraph Runtime owner。
- 当前合法受管 owner 只有 Finder 主进程 PID 68522、Serena broker PID 68553 与 Codex Runtime PID 68732 及其当前后代；未发现 cloudflared 或 CodeGraph runtime。没有停止或重启这些活动进程。

### 14.5 Finder Gate 结论与待执行退出

- Finder 双击启动来源：`PASS`；新候选 executable identity：`PASS`；GUI 精简环境：`PASS`；Codex 0.155.1 discovery/runtime：`PASS`；Serena 1.7.0 user-local fallback/broker：`PASS`；CodeGraph live product Runtime：`FAIL`（`CODEGRAPH_RUNTIME_START_FAILED`）。
- 本节取证时 PID 68522 尚未执行标准 `Cmd+Q`，因此当时的 Finder 标准退出与 Serena/Codex process-group-empty、9120/9121 清理、Runtime termination evidence 和 Claim 释放为 `NOT_RUN`。Host 随后已完成该操作，第 17.2 节记录其完整 PASS 证据；这不再是当前 blocker。
- 本节当时 LaunchAgent 保持 `NOT_RUN`；后续真启动结果见第 20 节。外置 APFS 保持 `UNAVAILABLE`，Windows 当前分支实机 Gate 保持 `UNAVAILABLE/NOT_RUN`。父 Phase 3 不归档。

## 15. Finder 精简 PATH 下 CodeGraph Capability Runtime 修复（2026-09-22）

### 15.1 最小实现边界

- `src-tauri/src/codegraph_capability.rs` 新增单一私有 `codegraph_command` helper。macOS 先调用既有 `which_command("codegraph")`，仅在 PATH 未命中时尝试 `$HOME/.local/bin/codegraph`；fallback 仍由 `which_command` 校验为可执行候选。Windows 与其他平台继续直接调用原有 `which_command("codegraph")`，不增加用户目录搜索语义。
- `discover_installation`、`run_command` 与 `start_runtime` 全部改为从该 helper 取得候选。installation probe、`status`/显式 prepare command runner 与 direct MCP Runtime starter 不再各自重新解析 PATH。
- runner 的 argv/cwd/stdio/timeout/cancellation、Runtime 的 `serve --mcp --path`、`CODEGRAPH_NO_DAEMON=1`、WorkspaceLease/RuntimeSlot/stop ownership 以及既有错误码映射均未改变。没有触发或新增 `init`、`sync`、`index`，也没有引入通用跨平台 Runtime abstraction。
- 修复后源码 SHA-256：`88a677a673e66316b7bee842b7d8672b5302792fa4fa47cee8169c4bd5958834`。

### 15.2 自动化证据

- focused `cargo test --manifest-path src-tauri/Cargo.toml --locked codegraph_capability::tests:: -- --nocapture`：`14 passed; 0 failed`。新增回归覆盖 macOS PATH miss 后 user-local fallback、PATH 命中优先、installation/runner/runtime 保留同一候选，以及非 macOS 分支继续保持 `which_command` 编译边界。
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：PASS。
- `cargo check --manifest-path src-tauri/Cargo.toml --locked`：PASS。
- `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings`：PASS。
- `cargo test --manifest-path src-tauri/Cargo.toml --locked`：`1080 passed; 0 failed; 24 ignored`。
- `npm run lint`：PASS；`npm test`：`117 passed; 0 failed`；`npm run build`：PASS。
- `git diff --check`：PASS。

### 15.3 Live Gate 与 Host Review 边界

- 本节没有重建 `.app`、没有退出或重启当前 Finder 实例，也没有再次调用其 `codegraph_explore`。PID 68522 仍运行修复前的 bundle executable，源码与自动化通过不能追溯改变该进程。
- 本节取证时 Finder live CodeGraph 保持 `FAIL`，当时的下一步是基于当前源码 rebuild `Serena Desktop.app`、标准退出旧实例并从 Finder 启动新 bundle。第 16、17 节已完成该序列并将最终状态更新为 binary discovery `PASS`、Workspace readiness/query `NOT_PREPARED`；后者是非阻塞 preparation state。
- 当前按要求停在 Host Review；未进入 Phase 4/5，未提交、未推送。

## 16. 包含 CodeGraph Capability fallback 的最新 `.app` 静态候选（2026-09-22）

### 16.1 构建身份

- `npm run tauri build`：PASS；Tauri release build、bundle 生成与本地签名完成。命令完成取证时间为 `2026-09-22T21:33:55+08:00`，bundle 与主 executable mtime 均为 `2026-09-22T21:33:43+08:00`。
- 产物路径：`src-tauri/target/release/bundle/macos/Serena Desktop.app`；主 executable 为 `Contents/MacOS/serena-desktop`。
- 主 executable SHA-256：`ae61d53bac198304545a5d723748d606c7d87d4dc4c3ba80efa6bf25ac018c45`。此前 PID 68522 对应候选的 executable SHA-256 为 `f83d633a992619810e60cb20c751333ef6444851a9faf69daf16a829f9e948da`，两者不同。
- 构建使用的 `src-tauri/src/discovery.rs` SHA-256 为 `1d32407269a74d89f4291fb9264137597765388e26c55f8d38e4c6db4099be3c`；`src-tauri/src/codegraph_capability.rs` SHA-256 为 `88a677a673e66316b7bee842b7d8672b5302792fa4fa47cee8169c4bd5958834`。

### 16.2 静态 bundle Gate

- `file`：`Mach-O 64-bit executable arm64`；`lipo -archs`：唯一架构 `arm64`，PASS。
- `CFBundleIdentifier`：`io.github.lifei6671.serena-desktop`；`CFBundleExecutable`：`serena-desktop`；`LSMinimumSystemVersion`：`12.0`。
- `codesign --verify --deep --strict --verbose=2`：PASS；bundle `valid on disk` 且满足 Designated Requirement。
- `codesign -dvvv`：`Signature=adhoc`、CodeDirectory flags 包含 `adhoc,runtime`、`TeamIdentifier=not set`；这是本地 ad-hoc 候选，不是 Developer ID 或 notarized 产物。构建日志也明确使用 identity `-`，notarization 因无配置而跳过；没有进入 Phase 5。

### 16.3 两项 macOS user-local fallback 的构建归属

- Serena fallback：上述 `discovery.rs` 在 macOS 将 `find_executable("serena")` 的 PATH 结果作为第一候选，仅在未命中时调用 `user_local_candidate("serena")`，即 `~/.local/bin/serena`。该源码参与本次 release 编译，构建前后 SHA-256 不变。
- CodeGraph Capability fallback：上述 `codegraph_capability.rs` 的共享 `codegraph_command` 先执行 `which_command("codegraph")`，仅在 macOS PATH 未命中时执行 `which_command($HOME/.local/bin/codegraph)`；installation probe、status/prepare runner 与 direct MCP Runtime starter 均复用该 helper。该源码参与本次 release 编译，构建前后 SHA-256 不变。
- 主 executable 的静态字符串同时包含 `.local/bin/codegraph` 与 `CODEGRAPH_NO_DAEMON`，与当前 CodeGraph Capability fallback/direct Runtime 源码一致。结合本次 `Compiling serena-desktop` release 日志及两份冻结源码 SHA，可确认该 bundle 同时包含 Serena 与 CodeGraph Capability 两项 macOS user-local fallback。

### 16.4 运行实例边界

- 本轮只构建并静态验证 bundle，没有启动、退出或重启 Serena Desktop，也没有调用 `codegraph_explore`。第 14 节 PID 68522 的启动时间为 `2026-09-22T21:15:23.163681+08:00`，早于本候选的 `21:33:43` bundle/executable mtime，且其已冻结 executable SHA 为旧值 `f83d…948da`；不得把该旧运行会话当成本节新候选。
- 本节静态候选阶段的 CodeGraph live 产品状态仍为历史 `FAIL`。Host 随后已标准退出旧实例并从 Finder 启动 SHA-256 为 `ae61…18c45` 的新候选；第 17 节给出最终 binary discovery `PASS` 与 Workspace readiness/query `NOT_PREPARED` 结论。
- 未提交、未推送；未修改其他代码，未进入 Phase 4/5。

## 17. 最新候选 Finder/CodeGraph live 复核（2026-09-22）

### 17.1 当前 Finder 实例身份：`PASS`

- Host 明确从 Finder 双击最新候选。RunningBoard job `application.io.github.lifei6671.serena-desktop.29298138.29298160` 为 `state=running`、`bundle id=io.github.lifei6671.serena-desktop`、`spawn type=app`、`spawn role=ui`，program 与唯一 argument 均为 bundle 内 `Contents/MacOS/serena-desktop`，working directory 为 `/`，没有 `--autostart`。
- Darwin BSD identity：PID `81872`、PPID `1`、PGID `81872`、SID `1`，start token `darwin_proc_bsd_start_v1:1790084255:192149`，start time `2026-09-22T21:37:35.192149+08:00`。启动时间晚于第 16 节构建完成时间 `2026-09-22T21:33:55+08:00`。
- `lsof` 冻结的 executable 路径为 `src-tauri/target/release/bundle/macos/Serena Desktop.app/Contents/MacOS/serena-desktop`；现场 SHA-256 为 `ae61d53bac198304545a5d723748d606c7d87d4dc4c3ba80efa6bf25ac018c45`，与 Host 指定的新候选完全一致。
- 当前进程环境仍为 `PATH=/usr/bin:/bin:/usr/sbin:/sbin`、`HOME=/Users/lifeilin`，属于 Finder/LaunchServices 精简环境；`which codegraph` 在该 PATH 下退出码为 1。

### 17.2 上一 Finder 实例标准退出：`PASS`

- `proc_pidinfo` 对旧主进程 PID `68522`、旧 Serena PID `68553` 与旧 Codex PID `68732` 均返回 `ESRCH`。当前 `127.0.0.1:9120` 只由新主进程 PID 81872 监听，`127.0.0.1:9121` 只由新 Serena PID 81916 监听；旧 owner 不再占用端口。
- Serena 日志记录 PID 68553 在 `2026-09-22T21:37:28.021+08:00` 开始 shutdown，并于 `21:37:28.122+08:00` 完成 application shutdown、写入 `Finished server process [68553]`。这提供了正式 broker shutdown 证据，不仅是 PID 消失。
- 旧 Codex Runtime `runtime-68522-1790082987477220-3` 于 `2026-09-22T21:37:27.959+08:00` 写入 `state=terminated`、`termination_evidence_state=complete`、`termination_evidence_type=macos_live_process_group_empty`，PID/PGID/SID 均为 68732；其余六个 probe runtime 也全部为相同 complete evidence。
- 旧 Runtime 的三个 execution 分别为 `cancelled/completed/completed`，均为 `release_evidence_state=complete`、`release_evidence_kind=same_runtime_cleanup`、`background_cleanup_state=empty`；旧 host 的非终态 Runtime 数、非终态 execution 数与 Claim 数均为 0。因此 PID 68522 的标准 `Cmd+Q` cleanup 已完整闭环。

### 17.3 当前 Serena 与 Codex discovery：`PASS`

- `~/.local/bin/serena` canonicalize 到 `~/.local/share/uv/tools/serena-agent/bin/serena`，在同一精简 PATH/HOME 下 `--version` 返回 `Serena 1.7.0`。当前 PID 81916 的 loaded modules 来自该 user-local uv tool，日志记录 `version=1.7.0`、PPID 81872、macOS ARM64，并由其监听 `127.0.0.1:9121`。
- 当前 host `host-81872-1790084256218913-1` 的正式 Runtime `runtime-81872-1790084370873712-3` 记录 canonical executable `~/.codex/packages/standalone/releases/0.155.1-aarch64-apple-darwin/bin/codex`、`codex-cli 0.155.1` 与 schema SHA-256 `058E9AF9…`；现场 PID 82256 加载的 executable 与记录一致。该 Runtime 当前属于活动 execution/Claim，`running/unknown` 是活动 owner 的预期状态，不是旧实例残留。

### 17.4 CodeGraph candidate 与只读 status

- 当前 fallback candidate 为 `~/.local/bin/codegraph`，canonical target 为 `~/.codegraph/versions/v1.6.0/bin/codegraph`，是可执行 POSIX shell script；在 Finder 同款精简 PATH/HOME 下通过绝对 fallback 执行 `--version` 返回 `1.6.0`。
- 在 canonical Workspace `/Users/lifeilin/wx_lifeilin/github.com/lifei6671/serena-desktop` 中，以 Provider 相同 argv/current_dir 只读执行 `codegraph status --json <canonical-root>`，完整结果如下。未执行 `init`、`sync` 或 `index`：

```json
{
  "initialized": true,
  "version": "1.6.0",
  "projectPath": "/Users/lifeilin/wx_lifeilin/github.com/lifei6671/serena-desktop",
  "indexPath": "/Users/lifeilin/wx_lifeilin/github.com/lifei6671/serena-desktop/.codegraph",
  "lastIndexed": "2026-09-22T13:28:05.186Z",
  "fileCount": 269,
  "nodeCount": 6954,
  "edgeCount": 31614,
  "dbSizeBytes": 43479040,
  "walSizeBytes": 10271192,
  "backend": "node-sqlite",
  "journalMode": "wal",
  "nodesByKind": {
    "class": 8,
    "component": 1,
    "constant": 57,
    "enum": 138,
    "enum_member": 504,
    "file": 265,
    "function": 3045,
    "import": 1071,
    "interface": 18,
    "method": 1045,
    "property": 16,
    "struct": 395,
    "trait": 10,
    "type_alias": 64,
    "variable": 317
  },
  "languages": ["javascript", "python", "rust", "tsx", "typescript", "yaml"],
  "pendingChanges": {"added": 0, "modified": 0, "removed": 0},
  "worktreeMismatch": null,
  "index": {
    "builtWithVersion": "1.4.1",
    "builtWithExtractionVersion": 24,
    "currentExtractionVersion": 25,
    "reindexRecommended": true,
    "state": "complete",
    "pendingRefs": 0
  }
}
```

### 17.5 CodeGraph Gate 判定

- Finder binary discovery：`PASS`。精简 PATH 本身不含 CodeGraph，但 Capability Provider 的 user-local fallback 已成功解析 candidate/canonical target 并执行真实 1.6.0 CLI；旧版 `CODEGRAPH_RUNTIME_START_FAILED` 已消失。
- Workspace readiness/query：`NOT_PREPARED`。虽然 `initialized=true`、index `state=complete` 且 pending changes 全为 0，但 `reindexRecommended=true`，因为 index extraction version 24 落后于当前 25。Provider 按既有契约将该状态投影为 Degraded，并在 Runtime acquire 前返回 NotPrepared；Host 当前公开 `codegraph_explore` 返回 `CODEGRAPH_NOT_INITIALIZED` 与这一边界一致，不再是 binary/runtime start failure。
- Workspace 后续如需恢复 query，可由 Local Human 执行 preparation `rebuild_index`；这是非阻塞 follow-up，不是 Phase 3 退出 blocker。本轮没有自动执行 `init`、`sync` 或 `index`，没有修改 `.codegraph`。当前 Codex 环境发起的额外公开工具复核被 approval policy `never` 拒绝，因此没有把该 Host 层策略拒绝误记为产品结果；上述公开错误码采用 Host 已确认的真实调用证据。

### 17.6 当前实例状态与剩余 Gate

- 当前新实例 PID 81872 仍在运行，Serena PID 81916、Codex PID 82256、9120/9121 与当前 Runtime/Execution/Claim 均属于该活动 owner，因此当前实例退出仍为 `NOT_RUN`。这项事实保留，但不构成 Phase 3 blocker：第 17.2 节 PID 68522 已提供真实标准 `Cmd+Q` 的完整 shutdown/release evidence，最新代码只新增 Serena/CodeGraph candidate fallback，未修改 app shutdown、Process Group、Claim 或 Runtime stop；当前 CodeGraph `NOT_PREPARED` 也未启动 CodeGraph Runtime，重复退出不会验证新增 stop ownership。Phase 6 将对 release candidate 再执行完整退出验收。
- 本节当时的真实剩余 blocker 包含独立 launchd job 下的 LaunchAgent environment/产品 discovery、真实非系统外置 APFS 路径/安装，以及 Windows 当前版本同步后的真机回归。LaunchAgent 后续已完成 job/environment/discovery，最新判定见第 20 节；外置 APFS 与 Windows 状态不变，父 Phase 3 不归档。
- 本轮除 Phase 3 文档外未修改产品代码，未提交、未推送，也未进入 Phase 4/5。

## 18. macOS LaunchAgent registration roundtrip Gate（2026-09-22）

### 18.1 实现边界

- `src-tauri/src/autostart.rs` 新增 macOS-only ignored integration test：`autostart::tests::macos_launch_agent_registration_roundtrip_restores_initial_state`。初始化参数与产品完全一致：`MacosLauncher::LaunchAgent` + `Some(vec!["--autostart"])`。
- 测试使用独立 package name `Serena Desktop LaunchAgent Roundtrip Test`，只定位并操作该测试 app 自己的当前用户 plist；不读取、删除或覆盖用户其他 LaunchAgent，也不操作产品 `Serena Desktop.plist`。
- Gate 先读取 `is_enabled`，对测试 plist 做存在性与原始字节快照，再执行 `enable → is_enabled=true → plist 只读解析 → disable → is_enabled=false`。成功路径显式恢复，panic/错误路径由 `Drop` 再次恢复；初始存在时按原字节恢复，初始不存在时只删除测试文件。
- 锁定的 `auto-launch 0.5.0` 实现没有公开 plist path，但实现明确使用 `~/Library/LaunchAgents/<app_name>.plist`，`Label=<app_name>`，`ProgramArguments=[canonical current_exe, args...]`。测试由 Tauri package info 取得 app name，不猜 label，并用系统 `/usr/bin/plutil` 只读解析 `Label` 与 `ProgramArguments`。
- 插件的 LaunchAgent 实现只写/删 plist，`is_enabled()` 只检查文件存在；不执行 `launchctl bootstrap`。因此本 Gate 的最大安全验证边界是 registration 文件，不是当前会话 loaded job，更不是登录后真实拉起。

### 18.2 Host 普通 Terminal 真实执行结果：`PASS`

执行命令：

```text
cargo test --manifest-path src-tauri/Cargo.toml --locked \
  autostart::tests::macos_launch_agent_registration_roundtrip_restores_initial_state \
  -- --ignored --exact --nocapture
```

Host 在非 Codex 文件沙箱的普通 Terminal 执行同一 ignored Gate，冻结输出如下：

```text
running 1 test
P3 LaunchAgent initial: app_name=Serena Desktop LaunchAgent Roundtrip Test; path=/Users/lifeilin/Library/LaunchAgents/Serena Desktop LaunchAgent Roundtrip Test.plist; enabled=false
P3 LaunchAgent enable: label=Serena Desktop LaunchAgent Roundtrip Test; target=/Users/lifeilin/wx_lifeilin/github.com/lifei6671/serena-desktop/src-tauri/target/debug/deps/serena_desktop_lib-4e476e648196885f; argument=--autostart; enabled=true
P3 LaunchAgent disable: app_name=Serena Desktop LaunchAgent Roundtrip Test; registration=absent; enabled=false
P3 LaunchAgent restore: app_name=Serena Desktop LaunchAgent Roundtrip Test; enabled=false; exact_state=true
test autostart::tests::macos_launch_agent_registration_roundtrip_restores_initial_state ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 1104 filtered out
```

- current-user registration create/read/delete roundtrip：`PASS`。
- `ProgramArguments` target 为当前测试 executable，第二个参数精确为 `--autostart`：`PASS`。
- 初始 `enabled=false`，结束恢复为 `enabled=false` 且 `exact_state=true`：`PASS`；测试专用 plist 无遗留。
- 该结果覆盖插件 registration 机制和参数生成，不证明 plist 已被 launchd bootstrap，也不证明产品在真实 LaunchAgent environment 中完成 discovery。

### 18.3 验证与剩余真人 Gate

- focused discovery：PASS，macOS ignored Gate 被正确列出（`0 failed; 1 ignored`）。
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`：PASS。
- `cargo check --manifest-path src-tauri/Cargo.toml --locked`：PASS。
- `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings`：PASS。
- `cargo test --manifest-path src-tauri/Cargo.toml --locked`：PASS，`1080 passed; 0 failed; 25 ignored`。
- 本节完成时真实 launchd job/LaunchAgent environment 启动仍为 `NOT_RUN`；第 20 节已记录 Host 随后执行当前 GUI domain 临时 job Gate 的结果。整个序列不要求注销或重启，也不修改产品真实 autostart registration。
- 外置 APFS 继续为 `UNAVAILABLE`，Windows 当前版本同步后的实机回归继续为 `UNAVAILABLE/NOT_RUN`；本节不改变二者状态。

## 19. 无需注销的真实 launchd 临时 job Gate（Host 已执行；cleanup 结果见第 21 节）

### 19.1 锁定依赖语义与隔离边界

- `tauri-plugin-autostart 2.5.1` 将 macOS LaunchAgent 的 app path 设置为 canonical current executable，并把初始化参数原样交给 `auto-launch 0.5.0`。
- `auto-launch 0.5.0` 实际 plist 字段只有 `Label=<app_name>`、`ProgramArguments=[canonical current_exe, "--autostart"]`、`RunAtLoad=true`；它不写 `KeepAlive`，也不调用 `launchctl bootstrap`。缺省 `KeepAlive` 为 false；临时 Gate 显式写 `KeepAlive=false`，只把该默认语义写明，不扩展产品行为。
- 产品真实 registration 使用 package name `Serena Desktop`，对应 `~/Library/LaunchAgents/Serena Desktop.plist`。临时 Gate 固定使用独立 label `io.github.lifei6671.serena-desktop.phase3-launchagent-gate` 和同名 plist，绝不覆盖或删除产品 registration。
- 当前 release executable 已只读确认存在、为 ARM64，SHA-256 为 `ae61d53bac198304545a5d723748d606c7d87d4dc4c3ba80efa6bf25ac018c45`。

### 19.2 创建、bootstrap 与取证命令

以下整段已由 Host 在普通 Terminal 执行。它先拒绝已有 Serena Desktop 进程、已有同名临时 job 或 plist；bootstrap 后不会自动 bootout/delete，会把 job 留给 Host/ChatGPT 继续取证。

```zsh
/bin/zsh <<'SERENA_LAUNCHAGENT_GATE'
set -euo pipefail

APP_EXE='/Users/lifeilin/wx_lifeilin/github.com/lifei6671/serena-desktop/src-tauri/target/release/bundle/macos/Serena Desktop.app/Contents/MacOS/serena-desktop'
LABEL='io.github.lifei6671.serena-desktop.phase3-launchagent-gate'
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
PRODUCT_PLIST="$HOME/Library/LaunchAgents/Serena Desktop.plist"
USER_ID="$(/usr/bin/id -u)"
DOMAIN="gui/$USER_ID"
SERVICE="$DOMAIN/$LABEL"

if [[ ! -x "$APP_EXE" ]]; then
  print -u2 "release bundle executable 不存在或不可执行：$APP_EXE"
  exit 1
fi

if /usr/bin/pgrep -x serena-desktop >/dev/null 2>&1 || /usr/bin/pgrep -f "$APP_EXE" >/dev/null 2>&1; then
  print -u2 '检测到 Serena Desktop 仍在运行。请先在应用中按 Cmd+Q，并确认进程退出后重跑本段。'
  /usr/bin/pgrep -fl 'serena-desktop|Serena Desktop.app' || true
  exit 1
fi

if [[ "$PLIST" == "$PRODUCT_PLIST" ]]; then
  print -u2 '临时 plist 与产品 registration 冲突，已拒绝执行。'
  exit 1
fi

if [[ -e "$PLIST" ]]; then
  print -u2 "临时 plist 已存在，未覆盖：$PLIST"
  exit 1
fi

if /bin/launchctl print "$SERVICE" >/dev/null 2>&1; then
  print -u2 "同名临时 job 已加载，未重复 bootstrap：$SERVICE"
  exit 1
fi

/bin/mkdir -p "$HOME/Library/LaunchAgents"
TEMP_PLIST="$(/usr/bin/mktemp "$HOME/Library/LaunchAgents/.${LABEL}.XXXXXX")"
trap '/bin/rm -f "$TEMP_PLIST"' EXIT

/usr/bin/plutil -create xml1 "$TEMP_PLIST"
/usr/bin/plutil -insert Label -string "$LABEL" "$TEMP_PLIST"
/usr/bin/plutil -insert ProgramArguments -json '["/Users/lifeilin/wx_lifeilin/github.com/lifei6671/serena-desktop/src-tauri/target/release/bundle/macos/Serena Desktop.app/Contents/MacOS/serena-desktop","--autostart"]' "$TEMP_PLIST"
/usr/bin/plutil -insert RunAtLoad -bool true "$TEMP_PLIST"
/usr/bin/plutil -insert KeepAlive -bool false "$TEMP_PLIST"
/usr/bin/plutil -lint "$TEMP_PLIST"

[[ "$(/usr/bin/plutil -extract Label raw -n "$TEMP_PLIST")" == "$LABEL" ]]
[[ "$(/usr/bin/plutil -extract ProgramArguments.0 raw -n "$TEMP_PLIST")" == "$APP_EXE" ]]
[[ "$(/usr/bin/plutil -extract ProgramArguments.1 raw -n "$TEMP_PLIST")" == '--autostart' ]]
[[ "$(/usr/bin/plutil -extract RunAtLoad raw -n "$TEMP_PLIST")" == 'true' ]]
[[ "$(/usr/bin/plutil -extract KeepAlive raw -n "$TEMP_PLIST")" == 'false' ]]

/bin/ln "$TEMP_PLIST" "$PLIST"
/bin/rm "$TEMP_PLIST"
TEMP_PLIST=''
trap - EXIT

/usr/bin/plutil -lint "$PLIST"
/usr/bin/plutil -p "$PLIST"
/bin/launchctl bootstrap "$DOMAIN" "$PLIST"
/bin/sleep 2

JOB_INFO="$(/bin/launchctl print "$SERVICE")"
print -r -- "$JOB_INFO"
JOB_PID="$(print -r -- "$JOB_INFO" | /usr/bin/awk '$1 == "pid" && $2 == "=" { print $3; exit }')"
if [[ -z "$JOB_PID" ]]; then
  print -u2 'job 已加载但没有 live PID；保留 job/plist 供取证，不自动清理。'
  exit 1
fi

ARGV="$(/bin/ps -ww -p "$JOB_PID" -o command= | /usr/bin/sed -E 's/^[[:space:]]+//')"
print -r -- "launchd PID=$JOB_PID"
print -r -- "launchd argv=$ARGV"
if [[ "$ARGV" != "$APP_EXE --autostart" ]]; then
  print -u2 '实际 argv 与预期不一致；保留 job/plist 供取证。'
  exit 1
fi

PID_INFO="$(/bin/launchctl print "pid/$JOB_PID")"
JOB_PATH="$(print -r -- "$PID_INFO" | /usr/bin/awk '$1 == "PATH" && $2 == "=>" { print $3; exit }')"
if [[ -z "$JOB_PATH" ]]; then
  print -u2 '无法从 launchctl pid domain 读取 PATH；保留 job/plist 供取证。'
  exit 1
fi
print -r -- "launchd PATH=$JOB_PATH"

UV_CANDIDATES=("$HOME/Library/Application Support/io.github.lifei6671.serena-desktop/runtime/uv/0.12.17/uv")
for directory in "${(@s/:/)JOB_PATH}"; do
  UV_CANDIDATES+=("$directory/uv")
done
UV_CANDIDATES+=("$HOME/.local/bin/uv" '/opt/homebrew/bin/uv' '/usr/local/bin/uv')
UV_PATH=''
for candidate in "${UV_CANDIDATES[@]}"; do
  if [[ -f "$candidate" && -x "$candidate" ]]; then
    UV_PATH="$candidate"
    break
  fi
done
if [[ -z "$UV_PATH" ]]; then
  print -u2 '按产品固定候选顺序未发现 uv；保留 job/plist 供取证。'
  exit 1
fi
print -r -- "uv candidate=$UV_PATH"
"$UV_PATH" --version

print -r -- '临时 LaunchAgent job 已保留。现在不要执行 cleanup。'
print -r -- '请通过菜单栏 Serena Desktop 图标打开主界面，在“状态”页点击“重新检测”，冻结 Codex/Git/Serena 结果后再交给 Host/ChatGPT 取证。'
SERENA_LAUNCHAGENT_GATE
```

UI 取证边界：`--autostart` 会按产品逻辑隐藏主窗口；通过菜单栏图标“打开主界面”显示的是同一 PID，不会替换 launchd owner。状态页“重新检测”覆盖 Codex、Git、Serena/CodeGraph 产品发现；uv 没有独立状态卡，因此上面脚本严格复现 `installer.rs` 的当前 macOS candidate 顺序并执行只读 `--version`，只能记录为 candidate discovery，不冒充产品已执行安装路径。

### 19.3 后续 cleanup（取证完成后再执行）

本轮 job、argv、PATH、discovery 与必要日志取证已经完成。以下命令应由 Host 在普通 Terminal 执行；本轮 Agent 不代为 cleanup，执行后还需复核 job/plist 均不存在：

```zsh
/bin/zsh <<'SERENA_LAUNCHAGENT_CLEANUP'
set -euo pipefail

LABEL='io.github.lifei6671.serena-desktop.phase3-launchagent-gate'
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
SERVICE="gui/$(/usr/bin/id -u)/$LABEL"

if /bin/launchctl print "$SERVICE" >/dev/null 2>&1; then
  /bin/launchctl bootout "$SERVICE"
fi

attempt=0
while /bin/launchctl print "$SERVICE" >/dev/null 2>&1; do
  attempt=$((attempt + 1))
  if (( attempt >= 50 )); then
    print -u2 "bootout 后 job 仍存在，未删除 plist：$SERVICE"
    exit 1
  fi
  /bin/sleep 0.2
done

/bin/rm -f "$PLIST"
[[ ! -e "$PLIST" ]]
print -r -- '临时 LaunchAgent job 与 plist 已清理；产品 Serena Desktop.plist 未触碰。'
SERENA_LAUNCHAGENT_CLEANUP
```

## 20. LaunchAgent 真启动取证（2026-09-22 22:19–22:26 +08:00）

本节只读检查 launchd、内核进程身份、open files、监听、产品日志与 StateStore；只读执行 Git/uv/Serena/CodeGraph version/status probe。没有发送 signal、调用 quit、执行 install/init/sync/index/rebuild，也没有启动无关 capability。临时 job/plist 保持现场。

### 20.1 job、plist 与 launchd owner：`PASS`

- service 为 `gui/501/io.github.lifei6671.serena-desktop.phase3-launchagent-gate`，plist 为 `~/Library/LaunchAgents/io.github.lifei6671.serena-desktop.phase3-launchagent-gate.plist`。`launchctl print` 冻结到 `type=LaunchAgent`、`state=running`、`job state=running`、`spawn type=daemon (3)`、`runs=1`、`pid=97374`、`last exit code=(never exited)`、`properties=runatload | inferred program`。
- job program 为 release bundle 内 `Contents/MacOS/serena-desktop`；launchd arguments 精确为 `[program, "--autostart"]`。plist 只包含同一 `Label`/`ProgramArguments`、`RunAtLoad=true`、`KeepAlive=false`，owner 为 `lifeilin:staff`，mtime 为 `2026-09-22T22:19:06+08:00`。
- `launchctl print pid/97374` 记录 `originator=.../Serena Desktop.app`、`creator=serena-desktop[97374]`、`euid=501`，并把 PID 97374 归入该临时 label 的 resource/jetsam coalition。`lsappinfo` 对该 PID 没有 LaunchServices application 记录；结合 job PID 精确一致，可确认它属于上述 LaunchAgent，而不是 Finder/open 的 RunningBoard application job。

### 20.2 主进程 identity、argv、environment 与唯一实例：`PASS`

- 内核 `proc_pidinfo(PROC_PIDTBSDINFO)` 与 `getpgid`/`getsid` 冻结：PID `97374`、PPID `1`、PGID `97374`、SID `1`，start token `darwin_proc_bsd_start_v1:1790086746:850752`，start time `2026-09-22T22:19:06.850752+08:00`。
- `KERN_PROCARGS2` 读取到实际 `argc=2`：`argv[0]` 为 release bundle executable，`argv[1]` 精确为 `--autostart`。同一内核读取与 `launchctl print pid/97374` 均得到实际 `PATH=/usr/bin:/bin:/usr/sbin:/sbin`、`HOME=/Users/lifeilin`；该 PATH 不含 `~/.local/bin`、Homebrew 或 npm 目录，属于 LaunchAgent 精简环境。
- `lsof` 冻结 executable 为同一 bundle 路径、cwd 为 `/`，现场 SHA-256 为 `ae61d53bac198304545a5d723748d606c7d87d4dc4c3ba80efa6bf25ac018c45`，精确匹配预期；`file`/`lipo` 为唯一 `arm64` Mach-O。bundle 主进程枚举只有 PID 97374，它独占监听 `127.0.0.1:9120`，因此只有一个合法主实例。

### 20.3 LaunchAgent 环境 dependency discovery：`PASS`

- Codex：StateStore 当前 host `host-97374-1790086747106072-1` 有六个 `macos-contract-*` probe，全部为 `terminated/complete/macos_live_process_group_empty`；正式 Runtime `runtime-97374-1790086858581909-4` 为 `running`，记录 canonical executable `~/.codex/packages/standalone/releases/0.155.1-aarch64-apple-darwin/bin/codex`、`codex-cli 0.155.1`、PID/PGID/SID 98523 与 start token `darwin_proc_bsd_start_v1:1790086858:781362`。现场实际 argv 为 `[canonical codex, app-server, --listen, stdio://]`，且 PPID 为 97374。结论：真实产品 Runtime discovery/ownership `PASS`。
- Git：同一 `PATH/HOME` 下 `command -v git` 命中 `/usr/bin/git`，`git --version` 返回 `git version 2.50.1 (Apple Git-155)`。结论：稳定 candidate/version diagnostic `PASS`。
- uv：按产品固定顺序检查 managed `runtime/uv/0.12.17/uv`、`~/.local/bin/uv`、PATH、`/opt/homebrew/bin/uv`、`/usr/local/bin/uv`；前两项和精简 PATH 均未命中，最终命中 `/opt/homebrew/bin/uv`，返回 `uv 0.12.17 (Homebrew 2026-09-18 aarch64-apple-darwin)`。这只记为 candidate discovery `PASS`，不冒充 install 路径已执行。
- Serena：当前配置 `serenaPath=null`，managed `runtime/bin/serena` 不存在，精简 PATH 也不能解析 `serena`；`~/.local/bin/serena` canonicalize 到 `~/.local/share/uv/tools/serena-agent/bin/serena`。产品实际 child PID 97403 的 PPID 为 97374、PGID 为 97403，实际 argv 通过 user-local link 启动 `start-mcp-server ... --host 127.0.0.1 --port 9121 --open-web-dashboard false`；日志记录 `version=1.7.0`、process/parent `97403/97374`、macOS ARM64，并在 `22:19:10.365` 写入“Serena MCP 端口已就绪”。PID 97403 独占监听 `127.0.0.1:9121`。由于 explicit/managed/PATH 均被排除，当前真实 child 直接证明 user-local fallback `PASS`。
- CodeGraph（非 Phase 3 必需项）：`~/.local/bin/codegraph` canonicalize 到 `~/.codegraph/versions/v1.6.0/bin/codegraph`，`--version` 为 `1.6.0`。只读 `status --json` 仍为 `initialized=true`、index `state=complete`、pending changes 全为 0，但 extraction version `24 < 25`、`reindexRecommended=true`，因此 binary discovery `PASS`、Workspace readiness/query `NOT_PREPARED`。本轮未执行 `init`、`sync`、`index` 或 `rebuild_index`。

### 20.4 `--autostart` 隐藏主窗口语义：Phase 4/6 `PARTIAL`

- 正向证据：实际 argv 精确含 `--autostart`；当前产品 `lib.rs` 在 setup 中以 `is_autostart_launch(std::env::args_os())` 命中后调用 `window.hide()`，非 autostart 分支才调用 `tray::show_main_window`。当前 Serena child argv 还显式包含 `--open-web-dashboard false`。
- 缺口：产品日志没有记录 `window.hide()` 结果；当前受管环境无法读取 CoreGraphics window list，Computer Use 也未获 Serena Desktop UI 控制授权，因此没有视觉或窗口状态快照可证明主窗口从未短暂显示或当前确实不可见。该分项保留 `PARTIAL`，不由源码分支或启动参数冒充视觉 `PASS`；它归入 Phase 4/6 桌面语义，不阻塞 Phase 3 dependency discovery。

### 20.5 Gate 判定与 cleanup 边界

- registration roundtrip：`PASS`；真实 launchd job identity/argv/environment/executable/单实例：`PASS`；Codex/Git/uv/Serena discovery：`PASS`；CodeGraph binary discovery：`PASS`，Workspace readiness/query：`NOT_PREPARED`。父 Phase 3 第一项 Acceptance 所要求的 Terminal/Finder/登录项 dependency discovery 已满足，可标记 `[x]`。
- `--autostart` 主窗口隐藏为 Phase 4/6 的 `PARTIAL`，不改变上述 Phase 3 判定。外置 APFS 仍未完成，Windows 当前版本同步后的实机 Gate 仍为 `UNAVAILABLE/NOT_RUN`，父任务保持 `planning`。
- 本节取证结束时 job/plist 仍在现场；Host 后续已执行第 19.3 节 cleanup。清理后的 job/plist/main removal 与 child/Runtime 复核结果见第 21 节。

## 21. LaunchAgent cleanup 后现场复核（2026-09-22 22:33–22:37 +08:00）

本节只读复核 launchd、plist、内核进程身份、监听、日志与 StateStore；没有发送终止 signal、修改数据库、触发 install/index/rebuild 或变更产品代码。

### 21.1 job、plist 与主进程 removal：`PASS`

- `launchctl print gui/501/io.github.lifei6671.serena-desktop.phase3-launchagent-gate` 返回 `Could not find service`（exit 113），临时 service 已不存在。`launchctl print pid/97374` 只保留 `properties=slain` 的非活动历史 stub，没有 service 或 process，不代表 PID 仍存活。
- 临时 plist `~/Library/LaunchAgents/io.github.lifei6671.serena-desktop.phase3-launchagent-gate.plist` 已不存在。产品路径 `~/Library/LaunchAgents/Serena Desktop.plist` 当前也不存在；第 11.2 节/临时 Gate 前的现场已记录产品 LaunchAgent 原本不存在，而临时脚本使用独立固定文件名，因此结论是“产品 plist 本来不存在且未被本次临时 Gate 删除或改写”，不是把缺失误记为 cleanup 删除。
- 旧临时主 PID 97374 的内核 lookup 为 `ESRCH`。release bundle executable 的 SHA-256 仍为 `ae61d53bac198304545a5d723748d606c7d87d4dc4c3ba80efa6bf25ac018c45`。

### 21.2 child identity、孤儿与端口：`FAIL`

- 本次 job 曾拥有的正式 Codex PID 98523（PGID/SID 98523，start token `darwin_proc_bsd_start_v1:1790086858:781362`）及六个 probe PID/PGID 均为 `ESRCH`，对应 process group 均为空；未发现这些 Codex identity 的孤儿残留。
- Serena PID 97403 未被 cleanup 回收。内核身份仍为 PPID `1`、PGID/SID `97403`、start token `darwin_proc_bsd_start_v1:1790086748:815820`、start time `2026-09-22T22:19:08.815820+08:00`。实际 argv 仍是 user-local Serena 1.7.0 的 `start-mcp-server`，使用产品 `runtime/serena-home/broker.yml`，监听 `127.0.0.1:9121`；其原始日志明确记录 parent 为 97374。该身份连续性证明它是临时 LaunchAgent child 在父进程消失后的孤儿，不是 Finder/open 的新实例。
- 复核时另有 RunningBoard application job 的新 bundle 主 PID 3292，占用 `127.0.0.1:9120`，其 Codex child 为 PID 3563；二者不属于旧临时 label。新实例日志在 `22:33:09.960` 记录 `9121` 已占用的启动错误，与孤儿 PID 97403 的监听一致。因而 `9120` 是当前新实例的合法 owner，`9121` 是临时 Gate 残留。

### 21.3 StateStore termination/release/Claim：`PARTIAL`

- host `host-97374-1790086747106072-1` 的六个 `macos-contract-*` probe runtime 均为 `terminated/complete/macos_live_process_group_empty`，与现场 PID/PGID 为空一致。
- 正式 Runtime `runtime-97374-1790086858581909-4` 当前为 `state=unknown`、`termination_evidence_state=unknown`、`termination_evidence_type=null`、`stopped_at=null`。产品在 `2026-09-22T22:33:08.369+08:00` 写入 `orphan resource unknown`。虽然其 Codex PID/PGID 已消失，仍不得绕过 fail-closed 契约推断为 complete。
- execution `execution-97374-1790086858579055-3` 为 `completed`，`release_evidence_state=complete`、`release_evidence_kind=same_runtime_cleanup`，release 时间为 `2026-09-22T22:31:17.921+08:00`；关联 Claim 计数为 0。结论是 Execution/Claim release `PASS`，但 Runtime termination evidence 未完成。

### 21.4 Gate 判定与剩余 blocker

- LaunchAgent registration roundtrip：`PASS`；真实 launchd job/argv/environment/CLI discovery：`PASS`；job/plist/main removal：`PASS`；临时 Gate cleanup 总体：`FAIL`，原因是 Serena orphan/`9121` 残留且正式 Runtime termination evidence 为 `unknown`。
- `--autostart` 主窗口隐藏的视觉语义继续为 Phase 4/6 `PARTIAL`，不阻塞 Phase 3。Phase 3 当前 blocker 为本次 cleanup failure、真实外置 APFS Serena 安装 `UNAVAILABLE`，以及 Windows 当前版本同步后的实机 Job Object/Host Crash Gate `UNAVAILABLE/NOT_RUN`。CodeGraph Workspace `NOT_PREPARED` 仍是非阻塞 preparation state。

## 22. launchd SIGTERM 统一 shutdown 修复候选（2026-09-22 22:48 +08:00）

### 22.1 根因与最小实现边界

- 第 21 节失败说明 `launchctl bootout` 直接终止 PID 97374 时，进程级 `SIGTERM` 没有产生 Tauri `ExitRequested`/`Exit` 事件，因此没有进入现有 `request_exit -> run_shutdown_once -> commands::shutdown_impl` authority。cleanup 脚本只负责卸载 job，不能代替应用内进程树与 Runtime evidence 收口。
- `src-tauri/Cargo.toml` 只给现有 Tokio 依赖增加 `signal` feature；`Cargo.lock` 无变化，也没有引入新 crate。
- 新增 `src-tauri/src/macos_termination.rs`，且仅在 `target_os = "macos"` 编译。listener 使用 `tokio::signal::unix::signal(SignalKind::terminate())` 接收 `SIGTERM`，不安装自写 POSIX signal handler，不在 signal 上下文中执行 Rust/Tauri/shutdown 逻辑。
- 收到 `SIGTERM` 后只通过 `AppHandle::run_on_main_thread` 调用既有 `request_exit`；资源 owner、隐藏窗口、失败日志、`app.exit(0)`、`run_shutdown_once` 的成功幂等/失败可重试语义全部沿用原路径。listener 持续等待后续 `SIGTERM`，因此一次 shutdown 失败不会永久失去后续 signal 入口；竞态仍由单一 `ShutdownState` 决定 owner。
- listener 创建失败或主线程投递失败只写稳定 app log category `termination signal`，不 panic、不阻断启动。没有接管 `SIGINT`、`SIGHUP` 或不可捕获的 `SIGKILL`。
- Windows 退出语义以及 Serena/Codex Process Group、Runtime/Claim、StateStore termination evidence、compatibility/discovery 契约均未改变。`lib.rs` 进入本次修复前已有的 macOS 测试 `autostart` cfg 改动被保留，没有被本次工作覆盖。
- 修复后源码 SHA-256：`src-tauri/Cargo.toml` 为 `f02e2c3baf96ebb86c3079a685c7e0435fe6fc25771c7a5c83517c59175e1373`，`src-tauri/src/lib.rs` 为 `385a52b6394cdaaf69b91f5c4c68b2c81debe750104cf0b2c0b6bf9a5b592069`，`src-tauri/src/macos_termination.rs` 为 `27a6be40d44d9ec3c77a704952677cb6c7daf3b444d59bb084bb63b7ed869264`。

### 22.2 自动化 Gate

- 新测试 `sigterm_dispatch_uses_idempotent_shutdown_authority_once` 使用可控 future 模拟 signal receipt，不向测试 runner 发送真实 `SIGTERM`；验证单次通知只调用一次 exit callback，并在随后模拟 UI/Exit 竞态时仍只有一个 `run_shutdown_once` owner 执行 shutdown。
- 新测试 `closed_sigterm_stream_does_not_dispatch_exit` 验证 listener 未收到通知时不调用退出 callback。
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：PASS。
- `cargo check --manifest-path src-tauri/Cargo.toml --locked`：PASS。
- `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings`：PASS。
- `cargo test --manifest-path src-tauri/Cargo.toml --locked`：`1082 passed; 0 failed; 25 ignored`。
- `npm run tauri build`：PASS；其 `beforeBuildCommand` 已执行 frontend `tsc && vite build`。本次没有前端源码改动，因此未额外重复 lint/unit test。
- `git diff --check`：PASS。

### 22.3 最新 ARM64 `.app` 静态候选

- 产物：`src-tauri/target/release/bundle/macos/Serena Desktop.app`；bundle/executable mtime 均为 `2026-09-22T22:48:17+08:00`。
- 主 executable SHA-256：`eb11a6ffc4f72037eae59f1aac13c33fd6e38b076e8e093b87f869c29d811ea0`。
- `file` 与 `lipo -archs`：`Mach-O 64-bit executable arm64`，唯一架构为 `arm64`。
- `codesign --verify --deep --strict --verbose=2`：PASS；bundle valid on disk 且满足 Designated Requirement。签名仍为本地 `adhoc`、`TeamIdentifier=not set`，未冒充 Developer ID/notarized 产物，也未进入 Phase 5。
- `CFBundleIdentifier=io.github.lifei6671.serena-desktop`、`CFBundleExecutable=serena-desktop`、`LSMinimumSystemVersion=12.0`。

### 22.4 真实 Gate 边界与 Host 后续两步

- 本轮没有在 Codex 沙箱内执行 `launchctl bootstrap`/`bootout`，没有向任何现有 Serena Desktop、Serena 或 Codex 进程发送 signal，也没有把自动化/静态 bundle 结果冒充真实 cleanup `PASS`。第 21 节旧实例结果保持 `FAIL`，正式 Runtime 的 `unknown` evidence 不被追溯改写。
- 当前只读 `lsof` 仍确认 PID 97403 使用 user-local Serena Python/site-packages 并独占监听 `127.0.0.1:9121`；受管环境禁止 `ps`/`pgrep`，因此沿用第 21 节冻结的 PPID/PGID/SID/start token/argv identity。它必须由 Host 定向清理，禁止按进程名全局 kill，也不得终止当前合法 Finder/Host 实例。
- Host 第一步：在普通 Terminal 重新核对 PID 97403 是否仍精确匹配第 21 节的 Serena identity、start time/token、PPID 1、PGID/SID 97403 与 `9121` listener；只有全部匹配才对 PID 97403 定向发送 `SIGTERM`，bounded wait 后若同一 identity 仍存活才定向升级 `SIGKILL`。若任一 identity 不匹配必须停止，不得假设 PID 未复用。
- Host 第二步：确认没有合法 Serena Desktop 实例后，对 SHA `eb11a6ff…11ea0` 使用第 19.2 节同一固定独立 label/plist 执行 bootstrap，形成新的 job/main/Serena/正式 Codex Runtime identity；取证完成后使用第 19.3 节同一 `bootout` cleanup。只有 service/plist/main、Serena/Codex process groups、`9120/9121` 均无该 Gate owner 残留，正式 Runtime 为 `terminated/complete/macos_live_process_group_empty`，Execution release complete 且 Claim=0，才可将 LaunchAgent cleanup 改为 `PASS`。
- 在上述 Host Gate 完成前，Phase 3 cleanup blocker 状态为“修复候选已构建，真实复验 `NOT_RUN`”；父任务继续 `planning`，本子任务继续 `in_progress`。

## 23. Host Review：SIGTERM listener 一次性消费修正（2026-09-22 22:55 +08:00）

### 23.1 Review 发现与修正边界

- 第 22 节初版 listener 使用 `while` 持续接收 `SIGTERM`。第一次 signal 已投递 `request_exit`、`run_shutdown_once.started=true` 且 shutdown 尚未完成时，第二次 signal 可能再次进入 `request_exit`，命中“退出清理正在进行”分支并调用 `show_main_window`。单一 shutdown owner 没有被破坏，但该 UI 副作用没有必要，必须在真机 Gate 前关闭。
- `install()` 现在只 await 一次 `termination.recv()`；首个 `SIGTERM` 到达后只投递一次既有 `request_exit`，随后 listener task 结束，不再消费或 dispatch 第二个 signal。
- listener 初始化失败与 `run_on_main_thread` 投递失败仍写原有稳定 app log category `termination signal`；没有改变错误文本或启动容错行为。
- `ShutdownState`、`run_shutdown_once`、`request_exit` 与 `commands::shutdown_impl` 均未修改；其成功幂等/失败可重试契约保持不变。Windows、其他 signal、Serena/Codex Process Group、Runtime/Claim/StateStore 契约也未改变。
- 修正后 `src-tauri/src/macos_termination.rs` SHA-256：`7890c1c7e53370702756e27d1d7f8d0f815fc958368c5ba151b6faf315aa3deb`。第 22 节 helper hash 与 executable SHA 只代表被本节取代的初版候选。

### 23.2 自动化 Gate

- focused `cargo test --manifest-path src-tauri/Cargo.toml --locked macos_termination::tests:: -- --nocapture`：`3 passed; 0 failed`。
- `sigterm_dispatches_exit_callback_once`：单个可控 signal 只调用一次退出 callback。
- `listener_stops_before_second_signal`：双信号 `VecDeque` fixture 调用一次 listener helper 后仍保留第二个通知，证明第二个 signal 未被读取或 dispatch；测试未向 runner 发送真实 signal。
- `sigterm_and_ui_share_one_shutdown_owner`：SIGTERM 路径后模拟 UI shutdown，`run_shutdown_once` owner 调用总数仍为 1。
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：PASS。
- `cargo check --manifest-path src-tauri/Cargo.toml --locked`：PASS。
- `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings`：PASS。
- `cargo test --manifest-path src-tauri/Cargo.toml --locked`：`1083 passed; 0 failed; 25 ignored`。
- `git diff --check`：PASS。

### 23.3 重建后的 ARM64 `.app`

- 产品 Rust 源码已变化，因此重新执行 `npm run tauri build`：PASS。没有单独重跑 frontend lint/unit test；Tauri 的既有 `beforeBuildCommand` 仍按构建契约执行 `tsc && vite build` 并通过。
- bundle/executable mtime 均为 `2026-09-22T22:55:10+08:00`；主 executable SHA-256：`d8eac58ced32a1d3866366bd654aee161b1ec6efb82739f12700971c32d08a2d`。
- `file`/`lipo -archs`：`Mach-O 64-bit executable arm64`，唯一架构 `arm64`。
- `codesign --verify --deep --strict --verbose=2`：PASS；仍为本地 `adhoc`、`TeamIdentifier=not set`，未进入 Phase 5。

### 23.4 真实 Gate 状态

- 本节没有执行 Host `launchctl bootstrap`/`bootout`，没有向现有 Serena Desktop、Serena 或 Codex 进程发送 signal。第 21 节旧 cleanup 仍为 `FAIL`，新候选真实 LaunchAgent Gate 仍为 `NOT_RUN`。
- Host 后续仍按第 22.4 节两步执行，但第二步必须使用本节最新 SHA `d8eac58c…d08a2d`；只有 Serena/Codex/ports/Runtime evidence 全部收口后才能改为 `PASS`。

## 24. 最新 SIGTERM 候选 bootout 前现场冻结尝试（2026-09-23 09:33 +08:00）：`BLOCKED`

本节只读检查 `launchctl`、临时 plist 与 TCP listener。没有执行 `bootout`，没有发送 signal、退出应用、删除 plist、修改 StateStore/数据库、修改代码或提交推送。固定预期 executable SHA-256 为 `d8eac58ced32a1d3866366bd654aee161b1ec6efb82739f12700971c32d08a2d`；由于旧 orphan 硬阻断条件命中，本轮没有继续执行主进程 SHA、Serena child、正式 Codex Runtime/Execution/Claim 的身份冻结，不能形成有效的 bootout 前基线。

### 24.1 当前 LaunchAgent job/plist：已冻结，尚未 bootout

- service 为 `gui/501/io.github.lifei6671.serena-desktop.phase3-launchagent-gate`，`launchctl print` 返回 `type=LaunchAgent`、`state=running`、`job state=running`、`runs=1`、`pid=21470`、`last exit code=(never exited)`、`properties=runatload | inferred program`。
- program 为 `/Users/lifeilin/wx_lifeilin/github.com/lifei6671/serena-desktop/src-tauri/target/release/bundle/macos/Serena Desktop.app/Contents/MacOS/serena-desktop`；arguments 精确为同一 executable 加 `--autostart`。
- plist `~/Library/LaunchAgents/io.github.lifei6671.serena-desktop.phase3-launchagent-gate.plist` 仍存在；只读 `plutil -p` 冻结到 `Label=io.github.lifei6671.serena-desktop.phase3-launchagent-gate`、同一 `ProgramArguments`、`RunAtLoad=true`、`KeepAlive=false`。
- `lsof` 显示当前主 PID 21470 监听 `127.0.0.1:9120`。

### 24.2 旧 orphan/9121 硬阻断：`FAIL`

- `2026-09-23T09:33:26+08:00` 的只读 `lsof -nP -a -p 97403 -iTCP:9121 -sTCP:LISTEN` 仍返回 `python3.1 97403 lifeilin ... TCP 127.0.0.1:9121 (LISTEN)`。这证明旧 PID 97403 仍是 9121 的 live owner，不满足“旧 orphan 已不存在且 9121 不再由它监听”的前置条件。
- 当前受管环境拒绝 `/bin/ps`（`operation not permitted`），且 `kill -0` 受 sandbox 影响，故没有把 `kill -0` 的失败误记为 `ESRCH`。listener 的 live kernel open-file 证据已足以命中用户指定的硬阻断条件。
- 因此本轮 Gate 立即停止并判定 `BLOCKED`。没有继续把当前 9121 owner 冒充为 PID 21470 的新 Serena child，也没有冻结或判定当前正式 Codex Runtime、Execution/Claim、9120/9121 owner 全链路为 `PASS`。

### 24.3 当前边界与下一步

- 当前临时 job、plist、PID 21470 与端口现场均保持原状；本轮未执行 `launchctl bootout`，也未删除 plist。
- “Host 下一步只需执行一次 bootout”这一结论当前不成立；在旧 PID 97403/9121 blocker 被按既有冻结身份定向处理、且重新建立无污染的 LaunchAgent bootout 前基线之前，不得执行或记录本轮 bootout cleanup `PASS`。
- blocker 消除并重新完成第 24 节缺失的主进程 SHA、Serena child、正式 Codex Runtime/Execution/Claim 与端口 ownership 冻结后，Host 才应只执行一次最短命令 `/bin/launchctl bootout gui/$(/usr/bin/id -u)/io.github.lifei6671.serena-desktop.phase3-launchagent-gate`；临时 plist 必须保留到 bootout 后取证完成。

## 25. 最终 bootout 前 clean baseline 冻结（2026-09-23 09:59–10:05 +08:00）：`CLEAN`

本节以 Host 明确确认“固定 LaunchAgent service 真正 running，且本次没有通过 Finder/Dock 手工启动”为启动来源的人类证据，并只读检查 `launchctl`、plist、内核进程身份、open files、监听、产品日志和 StateStore。四份权威输入的 SHA-256 分别精确匹配 `3973d005…b217`、`de45fa36…e7c`、`ee7fad36…a88c`、`ba8b2ca5…3bdd`；固定 release executable SHA-256 为 `d8eac58ced32a1d3866366bd654aee161b1ec6efb82739f12700971c32d08a2d`。本轮没有执行 `bootout`、发送 signal、退出应用、删除 plist、修改 StateStore/数据库、修改产品代码、提交或推送。

### 25.1 LaunchAgent job、plist 与 main identity：`PASS`

- service `gui/501/io.github.lifei6671.serena-desktop.phase3-launchagent-gate` 在 `2026-09-23T10:05:11+08:00` 仍为 `type=LaunchAgent`、`state=running`、`job state=running`、`runs=1`、`pid=31903`、`last exit code=(never exited)`、`properties=runatload | inferred program`。
- program 为 `/Users/lifeilin/wx_lifeilin/github.com/lifei6671/serena-desktop/src-tauri/target/release/bundle/macos/Serena Desktop.app/Contents/MacOS/serena-desktop`；launchd arguments 与内核 `KERN_PROCARGS2` 均冻结到同一 executable 加唯一参数 `--autostart`。plist `~/Library/LaunchAgents/io.github.lifei6671.serena-desktop.phase3-launchagent-gate.plist` 存在且 `plutil -lint` 为 `OK`，内容为同一 `Label`/`ProgramArguments`、`RunAtLoad=true`、`KeepAlive=false`。
- main PID `31903` 的 PPID/PGID/SID 为 `1/31903/1`，start token 为 `darwin_proc_bsd_start_v1:1790128545:166548`，start time 为 `2026-09-23T09:55:45.166548+0800`。真实环境为 `PATH=/usr/bin:/bin:/usr/sbin:/sbin`、`HOME=/Users/lifeilin`；`launchctl print pid/31903` 与内核读取一致。
- executable 现场 SHA-256 精确为 `d8eac58ced32a1d3866366bd654aee161b1ec6efb82739f12700971c32d08a2d`，文件为唯一 `arm64` Mach-O。全进程表按 canonical executable path 枚举只有 PID `31903`；它唯一监听 `127.0.0.1:9120`。结论：当前只有一个合法 Serena Desktop 主实例，且它就是固定 LaunchAgent owner。

### 25.2 当前 Serena child 与 `9121`：`PASS`

- main PID `31903` 的直接受管 child 枚举只有 Serena PID `31924` 与正式 Codex PID `34479`。Serena PID `31924` 的 PPID/PGID/SID 为 `31903/31924/31924`，start token 为 `darwin_proc_bsd_start_v1:1790128547:226179`，start time 为 `2026-09-23T09:55:47.226179+0800`。
- 真实 argv 为 `~/.local/share/uv/tools/serena-agent/bin/python ~/.local/bin/serena start-mcp-server --context ~/Library/Application Support/io.github.lifei6671.serena-desktop/runtime/serena-home/broker.yml --transport streamable-http --host 127.0.0.1 --port 9121 --open-web-dashboard false`；真实 executable 为 user-local ARM64 Python `3.13`。
- 对应 Serena 日志 `mcp_20260923-095547_31924.txt` 明确记录 `version=1.7.0, process id=31924, parent process id=31903`，随后记录 Uvicorn 启动完成。PID `31924` 是 `127.0.0.1:9121` 的唯一 listener，PGID `31924` 当前唯一成员也是 PID `31924`。
- 旧 orphan PID/PGID `97403` 的 `proc_pidinfo` 返回 `ESRCH`，open-file/listener 为空且该 process group 无成员。结论：当前 `9121` owner 是本次 LaunchAgent main 的 Serena 1.7.0 child，不是历史 orphan。

### 25.3 当前正式 Codex Runtime、Execution 与 Claim：`PASS`

- 当前 host 为 `host-31903-1790128545388944-1`；其正式 Runtime 为 `runtime-31903-1790128647025850-4`。StateStore 冻结到 PID/PGID/SID `34479/34479/34479`、start token `darwin_proc_bsd_start_v1:1790128647:218179`、canonical executable `~/.codex/packages/standalone/releases/0.155.1-aarch64-apple-darwin/bin/codex`、version `codex-cli 0.155.1`、`state=running`、`termination_evidence_state=unknown`、`termination_evidence_type=null`；活动 Runtime 在 bootout 前保持 `running/unknown` 是预期 live 状态。
- 内核现场精确匹配：PID `34479` 的 PPID 为当前 main `31903`，PGID/SID 为 `34479/34479`，start time 为 `2026-09-23T09:57:27.218179+0800`；真实 argv 为 `[canonical codex, app-server, --listen, stdio://]`，PGID `34479` 只有该 PID 一个成员。
- 当前 execution 为 `execution-31903-1790128647023790-3`，绑定上述 Runtime，`status=running`、`dispatch_state=dispatched`、`release_evidence_state=incomplete`。唯一 Claim 为 Workspace `/Users/lifeilin/wx_lifeilin/github.com/lifei6671/serena-desktop`、同一 execution、`claim_type=exclusive_execution`。hostId/runtimeId/PID/parent/start token/Execution/Claim 因而全部属于当前 LaunchAgent main host。

### 25.4 上一普通实例与历史 fail-closed evidence：`PASS`

- 上一普通实例 main PID `28900`、Serena PID/PGID `28924` 与正式 Codex PID/PGID `29096` 的 `proc_pidinfo` 均返回 `ESRCH`，open-file/listener 为空，对应 process group 无成员；它们不占用 `9120/9121`。Serena 日志在 `09:55:35` 记录 PID `28924` 完成 application shutdown；正式 Runtime `runtime-28900-1790128209144309-4` 已为 `terminated/complete/macos_live_process_group_empty`，其四个 probe Runtime 也均为相同完成状态。
- 历史旧 owner `host-97374-1790086747106072-1` 的正式 Runtime `runtime-97374-1790086858581909-4` 仍保留 `state=unknown`、`termination_evidence_state=unknown`；这是旧 cleanup 的 fail-closed 历史记录，不被追溯改写。其旧 main `97374`、Serena `97403`、Codex PID/PGID `98523` 当前均为 `ESRCH`，对应旧 process group 无成员且不占用 `9120/9121`，所以它不是当前 baseline owner，也不存在 live 旧 owner。
- 旧 LaunchAgent PID `21470`、其正式 Codex PID/PGID `22553` 也均为 `ESRCH` 且 process group 为空；StateStore 已记录该 Runtime 为 `terminated/complete/macos_live_process_group_empty`。当前两个端口的唯一 owner分别只有 PID `31903` 与 `31924`。

### 25.5 Gate 判定与 Host 边界

- 当前固定 LaunchAgent host 的 main + Serena + Codex + `9120/9121` + Runtime/Execution/Claim identity 已完整闭合，并且没有 live 旧 owner；最终 bootout 前 baseline 判定为 `CLEAN`。
- 历史 `runtime-97374-1790086858581909-4` 的 `unknown` 继续保留为历史 fail-closed evidence，但按本次固定判定规则不无限阻塞新的 Host Gate。它不得被冒充为当前 owner，也不得因本次结果回写为 complete。
- 本节结束时 service、main/children、ports、Runtime/Execution/Claim 与临时 plist 全部保持现场。Host 下一步只执行一次 `/bin/launchctl bootout gui/$(id -u)/io.github.lifei6671.serena-desktop.phase3-launchagent-gate`；不要删除 plist，bootout 完成后保持现场并等待 ChatGPT 复核 cleanup 后的 service/main/children/process groups/ports/Runtime/Execution/Claim evidence。

## 26. 最终 bootout 后复核（2026-09-23）：`PASS`

本节记录 Host 对第 25 节固定 LaunchAgent 执行一次 `bootout` 后的最终真实现场。复核只读取 launchd、临时 plist、内核进程/进程组、监听端口与 StateStore；没有删除临时 plist，没有修改产品代码、数据库或历史 evidence，也没有提交或推送。

### 26.1 内核身份与端口 Gate：`PASS`

- 固定 service `gui/501/io.github.lifei6671.serena-desktop.phase3-launchagent-gate` 已不存在；临时 plist `~/Library/LaunchAgents/io.github.lifei6671.serena-desktop.phase3-launchagent-gate.plist` 仍存在，符合 bootout 后保留现场的取证边界。
- 旧 LaunchAgent main PID `31903`、Serena PID `31924` 与 Codex PID `34479` 的内核 lookup 均为 `ESRCH`；对应 PGID 成员数均为 `0`。旧 identity 不再占用 `9120/9121`。
- 当前 Finder 新实例 owner 为 main PID `40704`、Serena PID `40730`，`hostId=host-40704-1790129915775196-1`；该 hostId、PID 与启动身份均不同于旧 LaunchAgent owner，不能把当前合法 Finder listener 误记为旧残留。

### 26.2 Runtime termination evidence Gate：`PASS`

- 旧正式 Runtime `runtime-31903-1790128647025850-4` 为 `state=terminated`、`termination_evidence_state=complete`、`termination_evidence_type=macos_live_process_group_empty`。
- `stopped_at=1790129830509`，`termination_evidence_at=1790129830509`；StateStore evidence 与 PID/PGID 空集一致。

### 26.3 Execution release Gate：`PASS`

- 旧 execution `execution-31903-1790129323401898-6` 为 `status=completed`、`release_evidence_state=complete`、`release_evidence_kind=same_runtime_cleanup`。
- `background_cleanup_state=empty`，不存在仍待后台回收的旧 execution 资源。

### 26.4 Claim 与旧 host 终态 Gate：`PASS`

- old execution Claim 计数为 `0`，old host Claim 计数为 `0`。
- 旧 host 的非终态 Runtime 数为 `0`、非终态 Execution 数为 `0`；没有以释放 Claim 代替缺失 termination/release evidence。

### 26.5 日志交叉证据与最终判定：`PASS`

- Serena PID `31924` 日志未找到正常 shutdown 或 `Finished server process` 文本；该缺失只表示日志交叉证据不完整，不覆盖内核身份、端口、Runtime evidence、Execution release、Claim 五组已完成 Gate，也不把 cleanup 误记为 `FAIL`。
- `FINAL_SIGTERM_LAUNCHAGENT_CLEANUP=PASS`。LaunchAgent registration、真实 launchd discovery 与 `SIGTERM` cleanup 至此全部为 `PASS`。
- 历史 `runtime-97374-1790086858581909-4` 继续保持 `unknown`，作为第 21 节旧候选的 fail-closed evidence；它没有 live PID/PGID/port owner，不是当前 blocker，不因本节结果改写为 complete。
- `--autostart` 主窗口视觉隐藏仍属于 Phase 4/6，不阻塞 Phase 3。Phase 3 当前真正剩余项仅为外置 APFS 路径 Gate（若父 Acceptance 仍要求）以及 Windows 同版本真机回归这项跨平台收口安全证据。
