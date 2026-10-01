# Independent FULL_SCOPE review context

Mode：Host review窄补测后的严格、独立、只读 `FULL_SCOPE`。Reviewer 未参与实现，不得修改 workspace；必须完整重审整个CB8-004 delivery，不限于本轮新增测试。

Baseline：`a42176717c33ea08c7f5be3f6fc96107f85fe578`。Review target 是 baseline 到当前 workspace 的完整 CB8-004 delivery，包括 tracked diff、两个新增 Rust 文件以及本 task 的 PRD/design/evidence/verification。`review-target.sha256` 是冻结清单本身，不纳入自身 hash。

Authority：

- `docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md` 的 CB8-004。
- `docs/technical-design-multi-agent-provider-codebuddy-v0.1.md` §23。
- CB5-005 `decision.md`、`crash-recovery.md`、`review-final.md`。
- CB8-003 production `session/load` Continue 已存在，但同 Runtime continuation recovery state 与独立 Result Recovery R2 必须保持不同 authority。

Mandatory review lenses：

1. R2 绝不能先于 exact R1 ownership + complete approved durable Windows Job evidence。
2. R2 wire 只能 `initialize -> session/load(exact S1, canonical cwd, mcpServers:[])`；不能 new/resume/prompt/tool/fallback/replay prompt。
3. replay 只允许 exact local conversation identity；存在 provider request id 时也必须 exact；bounded text最多 `Partial`，不能伪装 original terminal/`Complete`。
4. R2 termination failure必须 `Unknown + Claim retained`；Claim release前必须重新断言 R1与任何已启动R2 evidence complete。
5. staged original exact terminal/result必须优先，不能被 replay 降级。
6. R2 id 必须不同于 R1；generic execution binding永远仍指 R1，private inspection provenance指 R2。
7. restart twice无额外 Provider wire、Runtime attempt、finalize或revision异常；disabled/unavailable registration-only recovery不依赖 admission health。
8. side-effect marker不变；无 schema/migration；不进入 Usage/Phase9；不修改 CB5-005/CB8-003 authority。
9. Store transaction、OCC、crash point、launch/cleanup failure与 orphan R2 是否真正 fail closed。
10. 测试是否覆盖 A-D、错误矩阵、exact/foreign identity、dual evidence 与 startup路径，而非只测试 helper。

Known unrelated signal：`task_manager::recovery::tests::authority_state_failure_uses_stable_internal_code` 在 baseline外未改动区域仍稳定失败；不要把它当作 CB8-004 行为证据，但检查本卡是否意外影响该路径。`clippy --all-targets -D warnings` 仅有既有 `usage_tests.rs:987 await_holding_lock`。

第一轮 `CHANGES_REQUIRED` 的 P2=3 已处理：fixture新增真实 A1；invalid R1 evidence原地第二次 startup断言零 wire/attempt/revision变化；新增 private provenance缺失时下一 Host provider orphan scan独立收敛 R2 且保留 Claim 的测试。第二轮确认后两项有效，但发现 A2误写成生产不可达的 `running/dispatching`，现已改为真实 `dispatch_pending/dispatching` pre-flush。请验证全部修复，并重新执行全部 mandatory lenses。

本轮Host review剩余P2只要求补一条真实跨卡状态：CB8-003 child已通过typed Store mutation形成same-runtime `RecoveryState::Partial`，之后prompt crash。新增测试必须证明startup不会把它误认成已完成external inspection，而是在R1 proof后创建exactly one `R2 != R1`，并覆盖成功partial、R2 evidence失败保留Claim与second restart幂等。另请复核`host-review-followup.md`的transaction predicate结论；没有实际字段缺口时不要要求production去重重构。

输出要求：逐项列出 findings（severity、`file:line`、机制、可观察影响、最小修复）；最后给 P0/P1/P2/P3 计数和 `PASSED` 或 `CHANGES_REQUIRED`。没有 finding 时明确写“未发现问题”。
