# Phase 2A：macOS Runtime 进程契约设计

## 1. 目标与边界

本任务采用 A+ 方案：新增独立 `macos_launcher.rs` 与 `macos_runtime.rs`，只实现当前 Host 连续持有 ownership 时可证明的 macOS Runtime 进程契约。Windows `windows_launcher.rs`、`runtime.rs` 及其现有行为保持不变。

本任务不做以下工作：

- 不引入跨平台 Runtime trait 或统一 Windows/macOS 的内部类型。
- 不接通 macOS Provider、Pool、StateStore 或 Workspace Claim。
- 不设计数据库 migration、Startup Recovery 或跨重启自动收口。
- 不把 Process Group 描述成 Windows Job Object 的等价物。
- 不承诺约束主动调用 `setsid()` 或 `setpgid()` 脱离 containment 的后代。

Phase 2A 的输出是一个经过真机测试的 macOS 私有进程模块和被冻结的证据边界。Phase 2B 才决定这些证据如何持久化以及何时允许释放 Claim。

## 2. 模块结构

```text
macos_runtime.rs
  ├── 创建/持有 MacosRuntime
  ├── shutdown 状态收口
  └── 只在完整观测后构造 MacosTerminationEvidence
            |
            v
macos_launcher.rs
  ├── LaunchRequest 输入校验
  ├── fixed executable + argv + piped stdio
  ├── pre_exec setsid 与 SID/PGID/PID 校验
  └── 私有 identity adapter（libproc/libc）
```

`agent/codex/mod.rs` 只在 `target_os = "macos"` 时声明这两个模块。现有非 Windows unavailable Provider/Pool/Discovery 继续生效，因此 Phase 2A 代码不会被产品路径提前调用。

## 3. Launcher 契约

### 3.1 输入

`MacosLaunchRequest` 只接受：

- absolute executable；
- absolute、存在且为目录的 cwd；
- `Vec<OsString>` argv；
- 非空且有界的 Runtime ID。

launcher 在 spawn 前按 Unix 原始字节检查 executable、cwd、Runtime ID 和每个 argv：

- 任一值包含 NUL 时返回 `CODEX_LAUNCH_INPUT_INVALID`；
- Runtime ID 不得包含路径分隔符；
- executable、argv 与终止 NUL 的总字节数不得超过应用固定上限 `128 KiB`；
- cwd 不存在或不是目录时返回同一输入错误，不把该情况延迟到 child。

`128 KiB` 是 Serena Desktop 自身的确定性输入上限，不对外宣称等于 Darwin `ARG_MAX`。环境大小和内核最终 `exec` 失败仍由 spawn 错误映射为 `CODEX_PROCESS_CREATE_FAILED`。

### 3.2 创建与 stdio

使用 `std::process::Command`：

- `Command::new(executable)` 与逐项 `.args(argv)`，禁止 shell command string；
- stdin/stdout/stderr 全部使用独立 pipe；
- 不主动继承 Terminal stdio；
- 不新增环境清洗、目录 canonicalization 或额外文件描述符框架。

Rust 标准库创建的 pipe 和 exec error channel 继续由标准库管理。本任务只保证受管 stdio 明确为三条 pipe，不宣称关闭调用方可能自行创建且缺少 `CLOEXEC` 的未知文件描述符。

### 3.3 Session/Process Group

在 `CommandExt::pre_exec` 中执行以下顺序：

1. 调用 `setsid()`；
2. 读取 `getpid()`、`getpgid(0)`、`getsid(0)`；
3. 只有 `setsid` 返回值、SID、PGID、PID 四者一致才允许 `exec`；否则使 spawn 失败。

父进程在 spawn 返回后再次通过私有 identity adapter 读取 leader 的 PID、PGID、SID 与启动时间。父侧观测不满足 `SID == PGID == PID` 时，launcher 返回带 created ownership 的身份失败，调用方必须收口该进程，不能丢弃 ownership。

## 4. 私有进程身份适配器

`MacosProcessIdentityAdapter` 只存在于 macOS 模块内部。它可以使用：

- `proc_pidinfo(PROC_PIDTBSDINFO)` 读取 PID、PGID 和启动时间；
- `getsid(pid)` 读取 SID；
- `getpgid(pid)` 对 PGID 做独立核验。

adapter 内部的 Darwin C struct、常量和 FFI 声明均为私有实现细节。上层只接触：

- leader PID；
- containment PGID；
- opaque process-start token；
- 匹配/不匹配/不可观测结果。

启动令牌由内核报告的启动秒与微秒编码而成，但其字符串格式不是公共持久化协议。Phase 2B 如需持久化，必须重新定义 versioned storage contract，不能直接依赖 Phase 2A 的内部编码。

身份验证必须同时满足：

- observed PID 等于创建时 PID；
- observed PGID 与 SID 均等于创建时 containment PGID；
- observed start token 与创建时 token 完全一致。

PID 相同但 token 不同、SID/PGID 不一致、leader 不可观测，都不能被当作原 Runtime。

## 5. Runtime ownership 与 shutdown

### 5.1 Ownership

`MacosRuntime` 唯一持有：

- Runtime ID；
- 直接 child handle；
- stdin/stdout/stderr pipe；
- leader PID；
- containment PGID；
- 创建时 opaque start token；
- 私有 identity adapter。

创建成功后，ownership 必须从 launcher 明确移动给 Runtime。任何已创建 child 后发生的身份读取或构造失败，都返回仍携带 created ownership 的失败结果。

### 5.2 业务取消与进程收口分层

业务 Execution cancellation 继续使用现有 Codex `turn/interrupt` 协议。`MacosRuntime::shutdown` 不发送 `SIGINT`，也不替代、模拟或推断业务取消结果；它只在上层已经决定收口 Runtime 时执行 OS 级终止。

### 5.3 有界 shutdown

shutdown 采用固定阶段：

1. 验证当前 leader 身份仍匹配创建时 PID/PGID/SID/token。
2. 向负 PGID 对应的整个受管 Process Group 发送 `SIGTERM`。
3. 在调用方给定且有最大值限制的 grace deadline 内轮询：直接 child 状态和 Process Group 成员。
4. 若两者均退出，回收直接 child 并生成完整 evidence。
5. 若宽限期结束仍有成员：leader 仍存在时再次验证身份；leader 已因本次 `SIGTERM` 退出时，只有在初始身份已验证且 grace 期间对原 PGID 的连续观测从未出现空组、身份冲突或观测失败，才允许向同一 Process Group 发送 `SIGKILL`。
6. 在 kill deadline 内等待直接 child 回收并验证 Process Group 为空。
7. 只有两个条件都成立时构造 `MacosTerminationEvidence`；否则返回 `unknown` failure，并把 `MacosRuntime` ownership 交还调用方重试或人工收口。

`ESRCH` 只表示某次 signal 时目标不存在，不能单独成为完整 evidence。完整证据必须来自同一次 shutdown 中的直接 child 观测和 group-empty 观测。

### 5.4 leader 提前消失

- 首次 signal 前 leader 已消失但 Process Group 仍存在时，不能验证启动令牌和 containment；返回 `unknown` 并保留 ownership，不向该组发送 signal。
- 首次 signal 前或 shutdown 过程中，当前 Host 连续持有 ownership，且直接 child 已被回收、Process Group 也确认为空时，可以构造本次 live Runtime 的完整 group-empty evidence。
- leader 在身份已验证并发送 `SIGTERM` 后退出、但原 group 被连续观测为非空时，当前 live shutdown 可以对同一 PGID 执行 `SIGKILL` 升级；任一观测失败、空组后再次出现成员或身份冲突都会使本次收口转为 `unknown`。
- Host 重启后的任何 leader/group 判断不属于 Phase 2A。Phase 2B 必须把 PID/token 不匹配、leader 消失但组仍存在或任意证据不足映射为持久化 `unknown`，保留 Workspace Claim。

## 6. 终止证据

`MacosTerminationEvidence` 只在 macOS Runtime 模块内构造，包含：

- Runtime ID；
- leader PID；
- containment PGID；
- opaque process-start token；
- Host 连续 ownership 标记；
- group-empty 观测时间；
- 直接 child 已回收事实。

该结构只证明：当前 Host 自创建以来连续持有的受管 Runtime，在观测时直接 child 已回收且原 Process Group 无成员。它不证明主动脱离 group 的后代已经退出，也不提供 Windows Named Job 的跨重启等价语义。

## 7. 错误与失败所有权

错误码保持 macOS 私有、稳定且可测试：

- `CODEX_LAUNCH_INPUT_INVALID`
- `CODEX_PROCESS_CREATE_FAILED`
- `CODEX_PROCESS_IDENTITY_FAILED`
- `CODEX_PROCESS_GROUP_SIGNAL_FAILED`
- `CODEX_RUNTIME_TERMINATION_UNCONFIRMED`

在 child 尚未创建时，错误不携带 Runtime ownership。child 创建后发生的任何失败必须携带可继续收口的 created child 或 `MacosRuntime`，不得依赖 `Drop` 伪造终止证据。

## 8. 测试设计

所有真实进程测试仅在 `target_os = "macos"` 编译和运行，并使用测试二进制自身或固定系统 executable，不通过 launcher 构造 shell command string。

### 8.1 Launcher

- absolute executable、argv 和含空格/中文 cwd 原样传递；
- stdin/stdout/stderr 均为 pipe；
- 相对 executable/cwd、缺失 cwd、NUL、空 Runtime ID 和超过 `128 KiB` 的 argv 在 spawn 前拒绝；
- child 内部与父侧 adapter 均观察到 `SID == PGID == PID`；
- 创建后立即 shutdown 不遗留受管 child/grandchild。

### 8.2 Identity

- 同一进程的重复观测产生相同 opaque token；
- 注入 token、PGID 或 SID 不匹配时身份验证失败；
- leader 不可观测时不能产生匹配结果；
- identity adapter 测试在最低目标 macOS 12 或等价可信真机环境执行。

### 8.3 Shutdown

- child/grandchild 正常响应 `SIGTERM` 时，在 grace 内完成且不发送 `SIGKILL`；
- child/grandchild 忽略 `SIGTERM` 时，在有界 grace 后由 `SIGKILL` 收口；
- 直接 child 已退出且 group 为空时，当前 Host ownership 可以产生完整 evidence；
- 首次 signal 前 leader 已退出但仍有 group member 时返回 `unknown`，保留 Runtime ownership，测试必须显式清理 fixture；
- leader 响应 `SIGTERM` 退出而 grandchild 留在已连续观测的原 group 时，live shutdown 可以升级 `SIGKILL` 并完成收口；
- group 查询失败、身份不匹配或 deadline 超时均不能构造完整 evidence。

### 8.4 回归

- macOS 全量 `cargo fmt/check/clippy/test` 通过；
- diff 中不得修改 `windows_launcher.rs` 与 `runtime.rs`；
- Windows 现有行为由既有 Windows CI 保持，Phase 2A 不弱化或删除任何 Windows Gate。

## 9. 兼容性与回滚

- 新模块在 macOS 产品路径尚未启用，对现有用户行为没有可见变化。
- 不修改数据库，因此回滚只需移除两个 macOS 模块及模块声明。
- 若真实 macOS 测试表明 libproc 身份字段在目标版本不稳定，停止在 Phase 2A，不把未经验证的 token 接入 Phase 2B。
- 若无法在 leader 存活时安全验证 containment，保持 unavailable 后端，不以直接 child PID 或 `kill(pid, 0)` 降级替代。
