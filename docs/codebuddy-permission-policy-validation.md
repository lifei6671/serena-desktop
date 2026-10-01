# CodeBuddy 第一版本地自动 Permission Policy 验证

> 当前合同已由文末 2026-10-01 Permission Authority Cutover 更新：取消自动选择 auto，保留 Provider current/default mode；下面 2026-09-30 的实现与测试结果为历史记录。
日期：2026-09-30；分支 `feat/codebuddy`。Host 已授权直接实现。未提交 Git，未 reset/checkout；保留已有 macOS CodeBuddy、ACP auto mode 与 Codex 未提交改动。未改用户配置，未安装/升级 CodeBuddy，未重启 Host。

## 当前行为

ACP auto session mode 保持。后续 permission 仅由 Host 本地确定性规则给出 advertised AllowOnce/RejectOnce；没有 LLM 审批、AllowAlways、RejectAlways、`-y` 或 bypassPermissions。权限源为已核验 execution row 冻结的 WorkspaceLease/mode，不查询当前 Registry/active workspace。

| 工具 | read_only | workspace_write |
| --- | --- | --- |
| Fetch | AllowOnce | AllowOnce |
| Read/Search | 至少一个可验证 workspace 路径，全部检查 | 同左 |
| Edit/Delete/Move | RejectOnce | 所有 locations/Diff/raw 明确路径在 workspace，至少一个目标 |
| Execute | git status/diff/log/show 和明确只读辅助 | 开发 allowlist 与只读辅助 |
| Think/SwitchMode/Other、未知 mode、不可解析请求 | RejectOnce | RejectOnce |

开发 allowlist：rustc；cargo check/test/build/fmt/clippy/doc/metadata/fetch；npm/pnpm/yarn test/build/lint/typecheck/check/ci/install；go test/build/vet/fmt/mod download；pytest/python -m pytest；git status/diff/log/show/fetch。辅助命令为 pwd/ls/cat/head/tail/grep/rg/find，仅允许 workspace 或有限只读系统根 `/usr`、`/bin`、`/sbin`、`/System/Library`、`/Library/Developer`。

命令仅简单引号分词，最多一个有效 workspace `cd … && safe-command`。拒绝任意 && 链、分号、管道、后台、重定向、换行/CR、backticks、命令替换、shell wrapper、环境赋值、globs/expansion、response files，以及明显外部执行/写入选项。sudo/su/ssh/scp/rsync/deploy/release/publish/push、git reset/clean/checkout/switch/restore、curl/wget 不在 allowlist。保守解析会拒绝部分合法复杂命令；不会猜测或调用模型补判。

显式 rustc/cargo `-o`、`--out-dir`、`--target-dir` 输出经过 canonical workspace/OS temp 检查。OS temp 不适用于 Edit/Delete/Move。普通 absolute outside、父目录跳转、symlink/junction parent escape 拒绝；macOS `/var`、`/tmp`、`/etc` 系统别名映射到 canonical 身份，任意外部用户 symlink alias 不能映射为 workspace。Windows drive/UNC/verbatim prefix 和大小写使用组件身份与 ordinal 比较。

初始 exact ToolCall 才注册 id；已知 partial updates 合并 kind/status/name/locations/raw_input/content，terminal 删除。请求字段覆盖同一工具当前 snapshot；count/累计 bytes 均有界。外来 session/conversation/request metadata 不允许修改/删除或借用 snapshot。SDK 宽容 optional-field 解码前额外验证显式 kind/status/locations/content 等字段，避免坏路径静默消失。raw command/path 不进入安全 Activity、日志或 Remote MCP 投影。

每个 optionId 非空、唯一；唯一 typed AllowOnce 才能被选。allow 不可选时退回唯一 RejectOnce，无合法单次选项返回 PermissionOptions。AllowOnce 物理 flush 不发 PermissionDenied；RejectOnce 保留 CODEBUDDY_PERMISSION_DENIED 与固定安全 Activity；permission decision 不取得 terminal/release authority。

## 修改文件

- `src-tauri/src/agent/codebuddy/permission_policy.rs`：独立策略、路径/命令矩阵与 focused tests。
- `src-tauri/src/agent/codebuddy/permission.rs`：冻结 authority、有界 snapshots、typed option、真实拒绝 flush、focused tests。
- `src-tauri/src/agent/codebuddy/prompt.rs`：注入已核验 row 的 workspace/mode。
- `src-tauri/src/agent/codebuddy/protocol.rs`：snapshot byte budget 与 malformed structured field 输入校验。
- `src-tauri/src/agent/codebuddy/mod.rs`：注册策略模块。
- `src-tauri/src/workspace_path.rs`：绝对路径安全映射、冻结根身份、macOS alias、Windows prefix identity 与 fixtures。
- `src-tauri/src/agent/codebuddy/client_tests.rs`、`execute/tests.rs`：旧 permission 测试显式冻结 authority 与注册新集成测试。
- `src-tauri/src/agent/codebuddy/execute/permission_policy_tests.rs`、`src-tauri/tests/fixtures/codebuddy_execute_child.rs`：真实 pipe + SQLite 的 allow/read_only reject、Activity、terminal、containment evidence、claim release 集成测试。
- `docs/technical-design-multi-agent-provider-codebuddy-v0.1.md`、`docs/codebuddy-permission-mode-validation.md`、本文：当前合同、历史阶段区分和验证。
- `.trellis/tasks/09-30-codebuddy-permission-policy/`：需求、设计、执行记录。

Host 提供的 permission.rs、prompt.rs、workspace_path.rs 三个 SHA256 在改动前均匹配。

## 最终验证

| 验证 | 结果 |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml codebuddy -- --test-threads=1` | PASS：134 passed / 0 failed / 2 ignored；48.13s（不含编译） |
| `cargo test --manifest-path src-tauri/Cargo.toml workspace_path::tests -- --test-threads=1` | PASS：3 passed / 0 failed |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | PASS |
| `cargo check --manifest-path src-tauri/Cargo.toml` | PASS |
| `git diff --check` | PASS |
| Windows 原生 path/junction/Job 真机验证 | NOT_RUN |
| 真实 CodeBuddy 模型任务 E2E、两个 ignored CLI smoke | NOT_RUN |

覆盖 Edit inside/outside/read_only、所有明确路径字段和 Diff/locations、Fetch、rustc --test workspace source -o OS temp（含 macOS /tmp）、开发/只读命令矩阵、shell syntax 拒绝、cd 后 symlink、唯一 AllowOnce 与 Reject fallback、malformed IDs/fields、有界 snapshots、partial updates/外来身份/terminal 删除。原 auto mode 测试及 macOS Runtime/Recovery 集全部通过。

初轮两个路径失败定位到 macOS `/var` 与 `/private/var` 的系统别名；补入有限平台别名后通过。新增集成测试先暴露测试 API 误用及即时 terminal 与 Activity tick 的竞态；修正 history.events 访问并在隔离 peer 等待允许后真实 Activity 落库再返回终态。没有弱化正常 allow Activity 断言；reject 保持原清空旧 Activity 语义。链接器报告非阻塞 `__eh_frame section too large` warning，测试退出码为 0。

## 边界与 Host 后续验收

这是 deterministic Host Permission Policy，**不是 OS sandbox**。被批准的构建/安装/测试命令可执行仓库脚本、插件、工具配置与子进程，依然可能访问 workspace 外或网络；路径检查也不能消除批准之后的 TOCTOU。长期需要独立 Sandbox Layer。macOS process group / Windows Job containment 没有降低，Codex 行为没有改变。

真实 CodeBuddy E2E 仍需 Host 编译并重启新二进制后验证。建议重跑原 rustc --test + temp 输出案例，确认实际选择 AllowOnce、无 CODEBUDDY_PERMISSION_DENIED、测试产物与结果正确，再验证 read_only 写入/高风险命令拒绝。当前 fixture PASS 不等同于真实模型任务或 Windows 真机 PASS。

## Permission Authority Cutover（2026-10-01）

Host 提供的 Fresh、Continue、permission_mode_tests 三个 SHA256 在修改前全部匹配。工作区已有未提交变更，本轮只修改默认权限模式与对应测试/文档，不提交 Git，不 reset/checkout，不修改用户配置或 Codex 行为，不增加 CLI flags/env，不做 OS sandbox。

Host 真实证据：最新 Permission Policy 已进入 App，Read/Write 成功；CodeBuddy 2.160.0 被设置 auto 后，Bash/rustc 先进入自身 auto-mode security classifier，当前 session 连续记录 `auto mode classifier timed out after 60000ms`，多次重试仍未生成 /tmp 产物。普通 session 曾发送 `session/request_permission`。这支持由 SerenaDesktop 本地 deterministic policy 处理普通模式权限请求，并取消产品主动加入的重复 classifier authority。

Fresh/Continue 在没有显式 `desired.mode` / `desired.option` 时，不再填充 auto，不发送 `session/set_mode`，保留 new/load 的当前值和有效 replay 更新。删除 `SessionCatalog::default_permission_mode`。Continue source identity、model/reasoning profile、Provider health/config catalog 不变；auto 缺失或不选择 auto 都不是 unavailable 条件。显式 mode/option 仍受 advertised typed ACP 配置、合法 ACK、replay、confirm 和 acceptance-ready 重验约束。

Host Permission Policy 实现保持原样：普通 mode 的 request_permission 继续按冻结 workspace/mode + exact tool snapshot 选择 advertised AllowOnce/RejectOnce；允许路径不生成 CODEBUDDY_PERMISSION_DENIED，read_only 与危险命令仍拒绝。

本轮文件：

- `fresh.rs`、`continued.rs`：取消默认注入，保留显式配置路径。
- `execute/permission_mode_tests.rs`：modes/config advertise auto 和未 advertise auto 的 Fresh/Continue 请求顺序、source identity、profile/admission、默认 permission-response。
- `client_tests.rs`：显式 desired.mode=auto 的 SessionCatalog ACK gate、错误/超时/malformed、ACK 后 mode rollback/revoke/remove 与 confirm 回归；原 typed transport 对象 ACK 测试保留。
- `execute/permission_policy_tests.rs`、`tests/fixtures/codebuddy_execute_child.rs`：默认不发送 set_mode、AllowOnce 无 denied、read_only RejectOnce、workspace_write 危险命令 RejectOnce。
- `fresh/tests.rs`：Windows 默认 prepare 不发送 set_mode 的预期，保留显式 mode/option 与故障矩阵。
- 技术设计 §18 与本页、mode validation、macOS validation：当前合同与历史记录区分。

| 本轮验证 | 结果 |
| --- | --- |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | PASS |
| `cargo check --manifest-path src-tauri/Cargo.toml` | PASS |
| `cargo test --manifest-path src-tauri/Cargo.toml permission_mode -- --test-threads=1` | PASS：5 passed / 0 failed |
| `cargo test --manifest-path src-tauri/Cargo.toml permission_policy -- --test-threads=1` | PASS：8 passed / 0 failed |
| `cargo test --manifest-path src-tauri/Cargo.toml codebuddy -- --test-threads=1` | PASS：138 passed / 0 failed / 2 ignored；45.43s（不含编译） |
| `git diff --check` | PASS |
| Windows 真机 | NOT_RUN |
| 重新 build/restart 后真实 CodeBuddy E2E | NOT_RUN |

完整 CodeBuddy 集包含最终 permission_mode/policy 场景、typed ACK guard、Provider configuration catalog/health、Continue source identity、model/reasoning 与 Runtime/Recovery 回归。两个真实 CLI smoke 保持 ignored / NOT_RUN。测试链接器仍报告非阻塞 `__eh_frame section too large` warning，退出码为 0。

兼容边界：尊重 Provider/user 当前模式，因此已配置或已加载的 auto Session 仍可能运行 CodeBuddy classifier；本次不会强制切换用户配置或现有 Session。默认普通模式的外部行为取决于 Provider 是否发出 request_permission，该接口继续由未变的 Host Policy 处理。显式 auto 仍可触发 Provider 自己的 classifier。

必须重新 build 并 restart App 后，才能验证真实 Host E2E。需在普通 current/default mode 的 Fresh/Continue 重跑 rustc --test + /tmp 输出，核对 request_permission → advertised AllowOnce、没有 CODEBUDDY_PERMISSION_DENIED、产物/结果正确，再核对 read_only/危险命令 RejectOnce。原生 fixture PASS 不替代真实 CodeBuddy 或 Windows 真机验收。
