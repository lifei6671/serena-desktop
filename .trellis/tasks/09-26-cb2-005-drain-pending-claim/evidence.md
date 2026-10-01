# CB2-005 实现验证证据（待独立 Review / Host Gate）

## 范围与结果

只新增 backend contract tests，未改生产语义、UI、Provider routing、Runtime/Claim/Recovery；未进入 Phase 3、未提交 Git。实现侧六项契约检查 PASS，Host Gate 尚未执行。保留进入本卡前全部 dirty 文件；主会话负责与 baseline hashes/patch 独立核对。

代码增量：
- `src-tauri/src/provider_policy_drain_tests.rs`：新增 3 个 Windows focused tests，复用现有 Supervisor/Broker fixture 和 `Client::product_test_transport`、Manager `test_client`。
- `src-tauri/src/provider_policy_tests.rs`：仅追加 cfg(windows) 子模块声明，既有测试保持原样。
- 本目录验证日志、此 evidence.md、test-inventory.txt、matrix.jsonl、code-hashes.json。

没有新增 Product/helper 或依赖。测试中文注释说明外部边界和安全证据。

## 实际验证命令

Rust cwd = `E:\wx_lifeilin\github.com\lifei6671\serena-desktop\src-tauri`；全部 Windows 原生。每条 cargo test 另有 main.rs 0 tests，不计入测试总数。

| 命令 | 结果 | exit | 实际执行数量 | 日志 |
| --- | --- | --- | --- | --- |
| `cargo test --locked cb2_005_ -- --nocapture` 首轮 | FAIL | 101 | 0（编译失败） | focused-new.log |
| 同命令第二轮 | FAIL | 101 | 3：2 PASS / 1 FAIL | focused-new-r2.log |
| 同命令修正后 | PASS | 0 | 3 / 3 | focused-new-r3.log |
| `cargo test --locked commands::provider_policy_tests:: -- --nocapture` | PASS | 0 | 11 / 11（含新增 3） | regression-1.log |
| `cargo test --locked agent::task_manager::tests:: -- --nocapture` | PASS | 0 | 29 / 29 | regression-2.log |
| `cargo test --locked explicit_resume_concurrent_callers_share_one_first_dispatch_and_complete -- --nocapture` | PASS | 0 | 1 / 1 | regression-3.log |
| `cargo test --locked startup_guard_product_and_provider_agree_on_persisted_runtime_attempt -- --nocapture` | PASS | 0 | 1 / 1 | regression-4.log |
| `cargo check --locked` | PASS | 0 | 非测试 | check.log |
| `cargo fmt --all -- --check`（最终代码） | PASS | 0 | 非测试 | fmt-final.log |
| `cargo clippy --locked --all-targets -- -D warnings` | FAIL（已知 task 外 blocker） | 101 | 非测试 | clippy.log |
| `git diff --check`（cwd 仓库根目录） | PASS | 0 | 非测试 | diff-check-final.log |

最终回归集 42 个不同测试全部通过；加上新测试单独成功运行，共 45 次成功执行。失败尝试另列，不混入最终 PASS。test-inventory.txt 保存全部 42 个精确测试全名及结果。

首轮测试构造器调用了 private `Client::transport`（E0624）；已改用既有 test-only `product_test_transport`。第二轮 fake wire 只发送 turn/start ACK，未发送 turn/started，故运行态等待超时；真实 Provider 按设计只在通知后转 running。补齐协议 fixture 后通过，未改状态机。

Clippy 唯一错误为既有 `src/agent/store/usage_tests.rs:987` 的 `clippy::await_holding_lock`，await 在 1012/1016 行。按用户约束未修复。链接器有既有 informational warning；cargo check exit 0。Git 输出既有 LF→CRLF warning，无 whitespace error。

## 六项契约与准确测试名称

1. running survives disable、2. 新 Start 零副作用拒绝、3. pending Claim 保留、5. 同一 Execution 重新开启恢复：
   `commands::provider_policy_tests::drain::cb2_005_pending_reenable_resume_and_running_drain_matrix`
   - 从实际 Store 等待 running + dispatched 后才 disable。
   - disable 前后完整 Execution、WorkspaceClaimRecord、RuntimeRecord 相等；interrupt=NULL；worker 未结束；wire 后续正常完成，没有 turn/interrupt 或重复 thread/start / turn/start。
   - 两次新 requestKey Start 均 AGENT_PROVIDER_DISABLED，SQL Execution/Runtime/Claim/attempt 数量不增加。
   - pending Resume 拒绝后完整 Execution 不变；Claim owner 不变；无 runtime bind/attempt。
   - 将 review route 改为未注册 future-provider 后重新开启 codex，仍恢复原 Execution 的 codex/review，Runtime/Thread/Turn 各一次。
2. 4. cancel registration-only：
   `commands::provider_policy_tests::drain::cb2_005_disabled_unavailable_cancel_preserves_persisted_route`
   - 实际 Codex Provider 注册仍在但 health unavailable + enabled false + role route 已指向 future-provider。
   - 原 pending Execution cancel 成为 cancelled 并由既有安全事务释放 Claim；无 Runtime。既有 task_manager test 还覆盖 unknown 保留与 fake Provider 调用计数。
3. 6. disabled startup：
   `commands::provider_policy_tests::drain::cb2_005_disabled_startup_reconcile_claim_matrix`
   - 直接调用真实 Manager.reconcile_startup→registered CodexProvider.startup_reconcile→真实 recover_claims。
   - 4 个独立 Store case：无尝试 pending 返回 ExecutionPendingExplicitResume；持久化 attempt 无 bind 变 unknown；unknown 缺证据保留 Claim；已安全取消但遗留 Claim 由恢复释放。
   - safe-terminal 证据由真实 cancel 事务生成，仅将原 Claim 重新插入模拟历史残留，不手写 ReleaseBasis。

## State / Claim Matrix

来源：最终 `regression-1.log` 的 `CB2-005` JSON，完整原值提取在 matrix.jsonl；下表只将随机 Execution ID 简写为 E / C / P / A / U / S。每行 provider=`codex`，taskRole=`review`。所有 interrupt 均 NULL。
计数列顺序 = executions / runtime_instances / workspace_claims / execution_runtime_attempts。

| 阶段 | Execution | status | dispatchState | claimOwner | Runtime / Thread / Turn | has_runtime_attempt | 计数 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| before-disable pending | E | dispatch_pending | not_dispatched | E | NULL / NULL / NULL | false | 1/0/1/0 |
| after-disable pending | E | dispatch_pending | not_dispatched | E | NULL / NULL / NULL | false | 1/0/1/0 |
| rejected Start + Resume | E | dispatch_pending | not_dispatched | E | NULL / NULL / NULL | false | 1/0/1/0 |
| re-enable-resume / before-disable running | E | running | dispatched | E | runtime-E / THREAD / TURN | true | 1/1/1/0 |
| after-disable running | E | running | dispatched | E | runtime-E / THREAD / TURN | true | 1/1/1/0 |
| rejected running Start | E | running | dispatched | E | runtime-E / THREAD / TURN | true | 1/1/1/0 |
| 正常 drain 收敛 | E | completed | dispatched | NULL | runtime-E / THREAD / TURN | true | 1/1/0/0 |
| disabled + unavailable cancel 前 | C | dispatch_pending | not_dispatched | C | NULL / NULL / NULL | false | 1/0/1/0 |
| cancel 后 | C | cancelled | not_dispatched | NULL | NULL / NULL / NULL | false | 1/0/0/0 |
| disabled reconcile pending 前→后 | P | dispatch_pending→dispatch_pending | not_dispatched | P→P | NULL / NULL / NULL | false | 1/0/1/0 |
| disabled reconcile attempt 前→后 | A | dispatch_pending→unknown | not_dispatched | A→A | NULL / NULL / NULL | true（reserved-runtime） | 1/0/1/1 |
| disabled reconcile unknown 前→后 | U | unknown→unknown | uncertain | U→U | NULL / NULL / NULL | false | 1/0/1/0 |
| disabled reconcile safe terminal 前→后 | S | cancelled→cancelled | not_dispatched | S→NULL | NULL / NULL / NULL | false | 1/0/1/0→1/0/0/0 |

运行中的 RuntimeRecord.state=`running`、termination_evidence_state=`unknown`，disable 前后完整记录相等。主 wire test 复用历史 origin-ID `runtime-E` 契约，所以 `has_runtime_attempt=true`，explicit attempt 表为 0；不是缺少记录被隐瞒。独立 attempt case 验证 explicit reservation 表为 1。

## 证据边界 / 风险

这是 contract-test，不是真实 Codex CLI/Windows Job 生命周期验收。test_client 边界创建持久化 Runtime 行，duplex wire 提供对端；实际 Client、Codex Provider、admission、local mutation、Store/Claim 事务都执行。未启动 OS 子进程，故不声称实测 Job kill/exit。测试观察到完整 Runtime 记录不变、无 interrupt、wire 正常持续完成；同时当前 mutation 源码 `commands.rs::agent_provider_set_enabled_impl`→`SupervisorState::mutate_provider_settings`→`ProviderAdmissionPolicy::commit` 仅保存配置/发布策略，没有 cancel、Runtime terminate 或 Claim release 调用。

wire test 完成后 Runtime 行仍 running（mock Runtime 没有 OS shutdown），Claim 释放依据真实 provider terminal + same-runtime background cleanup；不将 terminal 本身当释放授权。没有用该行证明 Runtime 已停止。

真实 CLI/Job、Linux、UI、Phase 3 均 NOT_RUN（本卡范围外）。没有发现生产契约违例；独立 Review 和 Host Gate 仍由主会话 / Host 判断。

## Baseline 保留与冻结身份

`provider_policy_tests.rs` 去掉本卡唯一 cfg(windows)/path/mod 接入块后，UTF-8 SHA256 为 `74FD37574B00B7A1FB10F4220E7AF55FBCD104510B6597764E5AD36D7D252143`，与 baseline-hashes.json 同文件原 hash 完全一致。主会话另已核对其余 50/51 baseline 文件字节不变。

最终代码 hash：
- provider_policy_drain_tests.rs：`6DACC140E720BEF7A86A353E9521BC97845B7CAFBCC44676E1BC89CD0D0C93F8`
- provider_policy_tests.rs：`6CDEE480972979FA290008C20173B73DA8C145CC2CF5B7FF45A9E521E40C48CD`

实现侧代码与 evidence 已冻结，交主会话独立审查。此记录不代表 Host Gate 通过。
