# Delivery

CB8-004 Crash / Result Recovery Gate 已完成，未commit、未push、未进入Phase9。

## 实现

- `recovery.rs` 在任何R2动作前恢复并重新读取original R1 complete approved Windows Job evidence；Claim release前再次验证R1和所有已启动R2。
- 新增窄 `result_recovery.rs`：创建独立R2，wire仅允许typed `initialize -> session/load(exact S1, canonical cwd, mcpServers:[])`；不发送prompt、不创建/resume Session、不启用工具。
- exact replay只接受exact S1、local conversation request id及存在时的provider request id；bounded assistant text只能成为`Partial`，永不合成original terminal或`Complete`。
- Store复用v13字段与`BeginInspection/FinishInspection`，原子登记R2 attempt/runtime/private provenance并持久化partial/unknown；generic execution仍绑定original R1。未新增schema/migration。
- startup使用registered frozen LaunchSpec做可选inspection，但历史R1 recovery不依赖enabled/health/CLI admission；staged exact terminal直接按original authority完成，不创建R2。

## 证据

- A1/A2/B/C/D crash matrix、side-effect marker、R1/R2 evidence failures、load/identity错误、staged terminal、restart twice与orphan R2均由真实SQLite/Windows Job/native fake覆盖。
- CB8-003 continued child通过typed Store mutation形成same-runtime `Partial`后crash：已证明startup创建exactly one external R2；成功时partial/release，R2 evidence失败时unknown/Claim retained。
- R2原始method日志只有`initialize`和`session/load`；Workspace marker hash不变。
- Host follow-up最终验证：CodeBuddy 141/141、CodeBuddy Store 12/12、TaskManager相关startup 31/31、Product restart 8/8通过；此前Product full 135/135（5 ignored）保持生产代码未变的既有证据。
- `fmt --check`、`cargo check`、严格`clippy --lib -D warnings`、`git diff --check`通过。
- 独立只读FULL_SCOPE最终`PASSED`，P0/P1/P2/P3=0。

## 已知限制

- 当前CodeBuddy 2.158.0不能恢复exact typed `PromptResponse/stopReason`，因此replay正文最多`Partial`；无安全正文为`Unknown`，非exact terminal最终为`Interrupted`。
- Linux validation因项目内无Docker Desktop runner而`ENVIRONMENT_UNAVAILABLE`；未使用WSL。
- `clippy --all-targets -D warnings`仍仅命中既有`usage_tests.rs:987 await_holding_lock`。
- Codex TaskManager既有悬空Claim测试仍与当前查询行为失配；本卡未修改该路径。
