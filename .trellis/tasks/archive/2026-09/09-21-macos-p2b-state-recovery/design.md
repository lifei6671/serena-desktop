# Phase 2B：State Store 与恢复集成设计

## 1. 目标与边界

Phase 2B 把 Phase 2A 的 macOS 私有进程身份与 Process Group containment 契约接入 StateStore 和 startup recovery。核心安全条件是：只有能够证明“当前观察到的 leader 就是原 Runtime leader，且仍处于原 containment scope”时，恢复逻辑才可以发信号；只有能够证明该 scope 已为空时，才可以形成完整终止证据并释放 Claim。

本阶段不启用 macOS Codex 执行能力，不引入跨平台 Runtime trait，不修改 Windows Runtime、Windows launcher 或 Windows recovery 语义。Process Group 只代表当前 Host 管理的 macOS containment scope，不等价于 Windows Job Object，也不约束主动通过 `setsid`/`setpgid` 脱离的后代。

## 2. 方案选择

### 2.1 采用：平台判别列 + 平台私有实现

在共享 `runtime_instances` 表中增加平台和 containment 判别列，复用现有 Runtime/Execution/Claim 事务；macOS 的 Store adapter 与 recovery 保持独立文件。这样可以沿用现有 Claim authority 与原子释放事务，同时避免把 Windows Job 和 macOS Process Group 抽象成语义不真实的统一 Runtime。

### 2.2 不采用：重建 Runtime 表

重建表可以把所有条件写成新的 table-level `CHECK`，但会扩大迁移风险，并要求复制所有历史数据、索引和外键。v10 只新增列并通过触发器验证新旧行，改动更小，失败时由现有单事务 migration 自动回滚。

### 2.3 不采用：复用 Windows recovery

Windows recovery 依赖 named Job 的 durable handle 与 active-process evidence；macOS recovery 只能重新观察 PID/PGID/SID/start token。两者的可证明事实不同，复用会迫使公共接口表达错误等价关系。因此新建 macOS 私有 recovery，不改 Windows 路径。

## 3. Schema v10

### 3.1 新增字段

`runtime_instances` 增加以下字段：

| 字段 | 含义 |
| --- | --- |
| `runtime_platform` | `windows` 或 `macos`；历史行默认 `windows` |
| `containment_type` | `windows_job` 或 `macos_process_group`；历史行默认 `windows_job` |
| `process_identity_scheme` | `windows_filetime_v1` 或 `darwin_proc_bsd_start_v1`；历史行默认 Windows 方案 |
| `containment_process_group_id` | macOS PGID；Windows 行为 `NULL` |
| `containment_session_id` | macOS SID；Windows 行为 `NULL` |
| `containment_verified_at` | `PID == PGID == SID` 最近一次被验证的时间 |

现有 `codex_pid` 与 `codex_process_start_token` 继续保存 leader PID 和 start token，不重复增加同义字段。macOS token 持久化格式固定为 `darwin_proc_bsd_start_v1:<seconds>:<microseconds>`；解码必须拒绝未知版本、字段缺失、非数字和越界值。`proc_pidinfo` 原生结构仍封装在 `macos_launcher.rs` 的私有 identity adapter 内，不进入共享 Store contract。

### 3.2 条件约束

由于 SQLite 不能直接给现有表追加跨列 `CHECK`，v10 创建 `BEFORE INSERT` 与相关列的 `BEFORE UPDATE` 触发器，拒绝不完整组合：

- Windows 行必须使用 `windows_job` 与 `windows_filetime_v1`，macOS PGID/SID 字段必须为 `NULL`，既有 Job policy 约束保持有效。
- macOS 行必须使用 `macos_process_group` 与 `darwin_proc_bsd_start_v1`，Windows Job 字段必须为 `NULL`。
- macOS `preparing`/`unknown` 可以尚无身份；一旦 PID、token、PGID、SID、verified-at 任一字段存在，整组字段必须齐全，且 `PID == PGID == SID`。
- macOS `starting`、`running`、`terminating`、`terminated` 必须具有完整身份组。
- `terminated` 且终止证据为 complete 时，Windows 只接受既有 Job evidence；macOS 只接受 `macos_live_process_group_empty` 或 `macos_recovered_process_group_empty`。

migration 最后对历史行执行一次无语义变化的验证性 `UPDATE`，确保触发器会检查迁移后的全部数据。任何异常都使同一 migration transaction 回滚，`user_version` 保持 9。

### 3.3 v9 fixture

新增冻结的 v9 SQL fixture，包含完整 Windows Runtime、Execution 与 Claim 关系，而不是调用当前建库代码动态伪造旧库。测试覆盖：

1. 从 v9 fixture 升级后，历史行被标记为 Windows Job，数据、关系与既有证据不变；
2. fixture 中增加一个只在验证性 `UPDATE` 时失败的旧触发器，确认 migration 失败后新增列和 v10 触发器不存在、数据未变、`user_version == 9`。

## 4. macOS live Runtime 持久化

新增 `macos_runtime_store.rs`，只编译于 macOS，负责准备、启动、进入 terminating、记录 unknown 和完成 Runtime。Windows `runtime_store.rs` 不变。

`MacosRuntime::create` 在 Phase 2A launch 成功后写入：

- `runtime_platform = macos`；
- `containment_type = macos_process_group`；
- PID、PGID、SID，且三者相等；
- 编码后的版本化 start token；
- `containment_verified_at`。

spawn 之前的 Store 失败直接返回错误；spawn 之后的任何 Store 失败必须把仍持有 direct child 的 `MacosRuntime` 随错误返回，保证调用方不会失去 shutdown ownership。

live shutdown 沿用 Phase 2A 的 `SIGTERM → bounded grace → SIGKILL`。只有 direct child 已被回收、Process Group 查询成功且为空时，Store 才写入 `macos_live_process_group_empty`。信号失败、group 查询失败或证据提交失败都写入或保持 `unknown`；不得形成 complete evidence。

业务 Execution cancellation 仍走既有 turn/interrupt；本文件只处理 Runtime ownership 与 Host shutdown。

## 5. macOS startup recovery

新增 `macos_recovery.rs`，区分跨 Host 恢复和 live `MacosRuntime`：新 Host 没有 direct child handle，因此不能复用 live evidence，也不能声称连续持有 ownership。

### 5.1 恢复前验证

对每条待恢复的 macOS Runtime：

1. 读取并验证平台、containment、identity scheme 与完整字段组；
2. 解码版本化 start token；
3. 通过 macOS identity adapter 重新观察 leader；
4. 要求 PID、PGID、SID、start token 全部精确匹配持久化记录；
5. 再查询 Process Group 成员，并确认 leader 仍在该 group 中。

任一步失败、leader 已不存在或任一值不匹配，都不发送信号，Runtime/Execution 进入 `unknown`，Claim 保留。恢复开始前 leader 已消失时，即使 group 当前为空，也缺少进程身份连续性，不能补造完成证据。

### 5.2 有界终止

验证通过后向记录的 Process Group 发送 `SIGTERM`，在 bounded grace 内轮询 group：

- group 为空：写入 `macos_recovered_process_group_empty`；
- leader 消失但 group 仍存在：立即停止，进入 `unknown`，不发送 `SIGKILL`；
- 查询失败：进入 `unknown`；
- grace 到期且 leader 仍以相同 PID/PGID/SID/token 存在：才允许发送 `SIGKILL`。

发送 `SIGKILL` 后仍必须观察到 group 为空，才能写入 recovered complete evidence；否则进入 `unknown`。

### 5.3 Claim 恢复矩阵

startup recovery 继续使用现有 Claim authority 与事务：

| 当前状态/证据 | 动作 | Claim |
| --- | --- | --- |
| `PendingExplicitResume` | 保持既有显式恢复语义 | 保留 |
| 已终态且已有合法 complete release evidence | 执行既有幂等清理 | 释放或保持已释放 |
| `Pending`/`Unknown`，关联完整 macOS Runtime | 执行 macOS recovery | 取决于结果 |
| 缺失 Runtime、旧 Windows Runtime 或证据不足 | Execution 标记 `unknown` | 保留 |
| macOS recovered group-empty evidence 已原子提交 | 通过既有 `ResumeRecovery`/reconcile 流程完成 interrupted finalization | 原子释放 |

orphan macOS Runtime 可以独立恢复，但不得改变无关联 Execution 或 Claim。最终 release gate 必须同时验证 runtime platform、containment type、identity scheme 与平台对应的 complete evidence kind，不能只依据 `terminated` 状态。

## 6. Provider 接入

macOS 继续注册 unavailable provider，使 execute/continue/cancel 和 discovery 都维持 false/Unavailable；仅在 macOS 构建中保存 startup recovery 所需的 Store 与 owner，并把 `can_recover` 设为 true。其 `startup_reconcile` 调用独立 `macos_recovery`。

Linux/其他非 Windows 平台继续使用当前全 unavailable 行为。`AgentTaskManager` 已按 `can_recover` 调用 provider 的 startup reconciliation，因此不新增调度抽象，也不改变 Windows 注册路径。

## 7. 错误与 fail-closed 规则

- 身份、containment 或 Store evidence 任何一项不可证明，都不得杀进程或释放 Claim。
- PID/token/PGID/SID 不匹配视为身份冲突，不自动重试，不尝试猜测新 leader。
- group-empty 查询错误不等于 group empty。
- Store 写 complete evidence 失败不允许继续 Claim release。
- Claim finalization/release 失败保持 Claim；下一次 startup recovery 可按已提交事实幂等重试。
- `unknown` 是稳定状态，只能由后续足够证据或既有 Local Human Authority 收口。

## 8. 验证策略

### 8.1 Store 与 migration

- v9 fixture 成功升级及历史 Windows 语义保持；
- 失败注入完整回滚；
- Windows/macOS 字段组合、状态和 evidence kind 触发器矩阵；
- macOS token 编解码和非法输入拒绝。

### 8.2 live Runtime

使用现有 macOS child fixture 扩展真实进程测试：

- 身份持久化；
- 正常退出后 group-empty complete；
- SIGTERM 收口；
- grace 超时后的 SIGKILL；
- group 查询失败和 Store 提交失败时 unknown；
- spawn 后 Store 失败仍返回 Runtime ownership。

### 8.3 startup recovery

使用固定 fixture 构造可观察 Process Group：

- Host crash/restart 后身份完全匹配，终止并释放 Claim；
- leader 消失但 group 仍存在；
- PID 复用或 token/PGID/SID 任一不匹配，且无关进程保持存活；
- TERM 后 leader 消失但 group 仍存在，不发送 KILL；
- group 查询、evidence commit、final release 分别失败时 fail-closed；
- 连续执行两次 startup recovery 的幂等性。

最后运行受影响的 Rust tests、完整 Rust test suite、前端 test suite 和 `git diff --check`，确认 Windows 回归不变。

## 9. 回滚与后续阶段

代码回滚时移除 v10 逻辑和 macOS recovery 接线即可；已经升级到 v10 的数据库不做自动降级，符合项目现有单向 migration 模型。

Phase 2B 完成后，后续阶段再处理 macOS CLI discovery、环境继承和真实 provider 执行路径；这些能力不得提前混入本任务。
