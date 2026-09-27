# CB2-005 独立交付审查

Review mode: CHILD_AGENT（未参与实现；代码只读）。Gate: PASSED。覆盖完整 CB2-005 增量，无 P0/P1 或其他待修 findings。此结论不代替 Host Gate；Host Gate 仍待独立审查。

## 冻结身份与覆盖范围

- `src-tauri/src/provider_policy_drain_tests.rs` 全文件 SHA256：`6DACC140E720BEF7A86A353E9521BC97845B7CAFBCC44676E1BC89CD0D0C93F8`。
- `src-tauri/src/provider_policy_tests.rs` SHA256：`6CDEE480972979FA290008C20173B73DA8C145CC2CF5B7FF45A9E521E40C48CD`；只审查本卡 cfg(windows)/path/mod 接入块，既有 fixture/tests 作为上下文。
- `evidence.md` SHA256：`412AD8846968C0C283896F02395C6B933A4DE5592F53D2095C55B8BACC270B91`。
- `matrix.jsonl` SHA256：`C33BCF3EF2E9B0DE171ACD6476D663ED2370E0CA647A2D1A1FD573B21664E5FE`。
- `test-inventory.txt` SHA256：`FEA0A5C6147202B992C8FF9101B08253EEFFB4ADFC51E3EC830367E33F344568`。

已核对本任务 PRD/design/implement、jsonl context、task.json、baseline-hashes.json、code-hashes.json、验证日志、测试清单与矩阵；读取任务卡及指定技术设计章节。两份冻结文档当前 SHA256 与 Host 提供值一致。审查使用 Rust profile，重点为测试有效性、冻结身份、持久化副作用、并发/资源生命周期、失败与恢复边界、范围控制。

独立核对 baseline 51 个文件：50 个字节不变；唯一变化为 `provider_policy_tests.rs`，删除本卡接入块后的 UTF-8 hash 为 `74FD37574B00B7A1FB10F4220E7AF55FBCD104510B6597764E5AD36D7D252143`，精确匹配 baseline。没有生产代码、依赖或 schema 增量。无需同步 `.trellis/spec/`：本卡证明既有契约，不改变规范。

## 六项验收覆盖

| 要求 | 实际证据与判断 |
| --- | --- |
| Running survives disable | 新 matrix test 等待真实 Provider 写入 running/dispatched 后停用；Execution、完整 Claim 和 RuntimeRecord 均不变，interrupt 为 null，worker 未结束。之后相同 wire 正常完成并严格检查无 interrupt/重复请求。mutation 链仅保存配置并发布策略，没有 terminate/cancel/release 调用。PASS。 |
| New execution rejected | pending 和 running 两阶段分别使用新 requestKey；均返回 AGENT_PROVIDER_DISABLED，Execution/Runtime/Claim/attempt SQL 数量不变。既有 disabled Start test 补充 Provider execute_calls=0。PASS。 |
| Pending retains Claim | not_dispatched、无 Runtime/attempt 的原 Execution 被停用；Resume 拒绝且 snapshot/完整 Execution 不变。既有 disabled_resume_preserves_execution_and_claim_byte_for_byte 进一步断言完整 Claim 不变。PASS。 |
| Cancel callable | 新测试使用真实已注册 Codex Provider，disabled + unavailable，当前 Role route 指向未注册 future-provider；仍取消 persisted codex/review Execution，安全事务释放 pending Claim。既有测试补充 registration-only 调用计数及 unknown 不被猜测为安全终态。PASS。 |
| Re-enable resume | 同一 pending ID 在重新开启后经正式 admission/guard_pending_dispatch 恢复，忽略改变后的 Role route；身份仍 codex/review，只有一个 Execution/Runtime，wire 只允许一次 thread/start 和 turn/start。既有 concurrent explicit resume test 补充并发首次派发收敛。PASS。 |
| Disabled startup reconcile | 真实 Manager registration-only → Codex startup_reconcile → recovery；pending、reserved attempt、unknown、safe terminal 遗留 Claim 四个数据库场景分别证明 PendingExplicitResume、unknown 保留、unknown 保留、有安全证据才释放。安全终态由真实 cancel 生成，未伪造 ReleaseBasis。PASS。 |

已读 admission、local mutation、persisted cancel/Resume、test_client 以及 recover_claims 当前路径；断言与实际路径一致。矩阵中的 runtime attempt=true / explicit attempt 表=0 使用现有 origin-ID 兼容契约，证据已明确披露，另有 reserved-runtime 场景覆盖显式 attempt 表。

## Findings (fixed)

无。本次按只读契约未修改代码，仅创建此报告。

## Findings (not fixed)

无本卡缺陷。已知任务外 Clippy `src-tauri/src/agent/store/usage_tests.rs:987` 的 await_holding_lock 按用户要求保持原样，不计为本卡回归或阻断项。

## Verification

审查者独立执行：

- cwd=`E:\wx_lifeilin\github.com\lifei6671\serena-desktop\src-tauri`，`cargo check --locked`：PASS，exit 0。
- 同 cwd，`cargo fmt --all -- --check`：PASS，exit 0。
- cwd=仓库根目录，`git diff --check`：PASS，exit 0；仅已有 LF/CRLF warning。

完整核验实现侧实际日志，不重复无新疑点的测试运行：

- `cargo test --locked commands::provider_policy_tests:: -- --nocapture`：11 PASS。
- `cargo test --locked agent::task_manager::tests:: -- --nocapture`：29 PASS。
- `cargo test --locked explicit_resume_concurrent_callers_share_one_first_dispatch_and_complete -- --nocapture`：1 PASS。
- `cargo test --locked startup_guard_product_and_provider_agree_on_persisted_runtime_attempt -- --nocapture`：1 PASS。
- 上述共 42 个不同测试，清单逐行与四份 regression 日志一致，无遗漏/重复；每次 main.rs 0 tests 未计入。
- `cargo test --locked cb2_005_ -- --nocapture` 最终独立组：3 PASS。加上回归共 45 次成功执行；早期编译失败和 2 PASS/1 FAIL 均如实单列，未混入最终通过计数。
- `cargo clippy --locked --all-targets -- -D warnings`：FAIL，exit 101；日志只有上述已知 usage_tests.rs blocker。未声称全局 Clippy 通过。
- `matrix.jsonl` 的全部 17 条记录与 regression-1.log 对应原始 JSON 完全一致。

## 证据边界

test_client 在通过正式 admission 与 pending dispatch guard 后替代 OS Runtime 创建；Provider.run_client、Client 协议处理及 Store/Claim 事务真实执行。它不经过生产 Runtime spawn/pool 生命周期，不能证明 OS Job kill/exit；本卡结论由可观察状态不变、无取消/重放、wire 正常继续及无 terminate 调用的 mutation 路径共同支撑。evidence.md 已准确说明这一边界。

完成后 mock Runtime 仍 running，不构成真实 Runtime 已停止的证据；Claim 释放依赖同 Runtime 的 terminal 加 background cleanup，而非 terminal 单独授权。真实 CLI/Job、Linux、UI、CB3-001/Phase 3 均未执行，且不属于本卡范围。

ReviewedStateMatchesFinalState：以上代码和证据 hash 核对一致。后续若修改代码或实质证据，应对受影响部分重新审查。Host Gate pending。
