# CB9-002 Usage Product/UI Regression Gate

用户已明确授权创建并实施本卡。基线为 `feat/codebuddy` / `c631a6707d2da0d3e0c8dbf440701e30e5739461`，起始工作区 clean。只做 CB9-002，不 commit/push，不进入 Phase10。

## 权威合同

- `docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md` 的 CB9-002。
- `docs/technical-design-multi-agent-provider-codebuddy-v0.1.md` §20.1、§33、CB-009。
- `.trellis/tasks/09-28-cb9-001-codebuddy-usage-skip/decision.md`：首版 `tokenUsage=false`，CodeBuddy public Usage unknown/null，`SKIPPED_UNSUPPORTED`。

## 必须行为

1. Codex complete/partial 公共 Usage 继续按既有字段投影；partial 缺失字段保持 null，不能补 0。
2. CodeBuddy 当前 `tokenUsage=false`。无 `execution_usage` 行时 detail/observe/list 返回 `unknown` 和全 nullable token/context 字段；不得读取、创建或更新 Codex private Usage state。
3. 非法或历史 `codex_execution_usage_state` 污染行不得影响 CodeBuddy Product 结果。
4. 未注册历史 Provider 无公共行时 unknown/null；有合法 provider-neutral 公共行时按行投影且 Provider identity 不丢失。
5. 现有公共 Store contract 将 `execution_usage` 作为 provider-neutral historical truth；因此匹配 CodeBuddy identity 的合法公共行应显示，但 `tokenUsage` capability 仍为 false。
6. Usage 缺失或出现不得改变 lifecycle、status、Claim、control/activity revision 或 available actions。
7. Store 重开后 unknown/public Usage 投影保持稳定。
8. AgentPanel/Usage presentation 对 unknown/null 显示破折号而不是 0；Codex partial/complete 仍正确。不得为本卡新增 UI。
9. Windows CodeBuddy catalog 保持当前实现：`canExecute/canContinue/canCancel/canRecover/activity=true`，`tokenUsage=false`；非 Windows 继续由 `cfg!(windows)` 保守投影。

## 范围

允许：Product/frontend tests、fixtures/snapshots、任务证据；只有测试证明 Product/UI bug 时才做最小 production 修复。

禁止：CodeBuddy Runtime、protocol、recovery、Usage projector、`execution_usage` writer、schema/migration、Codex private Usage semantics、Phase10、commit、push。

## 验收

- [ ] A-I matrix 均有可执行测试或明确复用证据。
- [ ] 序列化 JSON 明确保留 `unknown`、null 与真实 0 的区别。
- [ ] Provider catalog capability 与 Usage 数据展示不耦合。
- [ ] Rust targeted、Product full module、frontend实际非零测试、fmt/check/clippy/diff checks 有诚实记录。
- [ ] Runtime/protocol/recovery/schema/migration diff 为 0，权威文档 hash 不变。
- [ ] 完整差异冻结后，由未参与实现的独立只读 reviewer 做 FULL_SCOPE review；P0/P1/P2 必须修复并重审。

