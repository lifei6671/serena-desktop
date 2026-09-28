# CB8-003 CodeBuddy Continuation

用户已明确授权创建并实施本卡。基线为 `feat/codebuddy` / `7760c11eb3b670b8146b0c472d6184356995f7ac`，起始工作区 clean。只做 CB8-003，不 commit/push，不进入 CB8-004。

## 权威合同

- `docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md` 的 CB8-003。
- `docs/technical-design-multi-agent-provider-codebuddy-v0.1.md` §16、§10.1。
- CB5-005 final `decision.md`、`dcr.md`、`review-final.md`。
- Continue=`PROVEN_SUPPORTED`；唯一恢复方法 `session/load`；Result Recovery 仍为 `partial`；Usage 仍 unsupported / `tokenUsage=false`；v13 `EXISTING_FIELDS_SUFFICIENT`。

## 必须行为

1. generic TaskManager 继续拥有 admission、child Execution、parent lineage 和 Workspace authority；CodeBuddy 不重做公共 Continue 编排，也不绕过 disabled/current policy。
2. child `parent_execution_id` 触发 continued path；读取并验证 source Execution 与 CodeBuddy PrivateState，取得 exact source `session_id=S1`。
3. Continue 创建独立新 Runtime 和 child private identity；不复用 parent Runtime，不继承 `conversation_request_id`、`prompt_rpc_id`、`provider_request_id`。
4. R2 initialize 后必须具备 `loadSession` capability，只发送 typed `session/load({sessionId:S1,cwd:exact projected child workspace,mcpServers:[]})`。
5. load 前建立 exact-S1 route 与有界 replay 接收；wrong session、response mismatch、missing/empty/unusable replay 一律 fail closed。
6. exact lineage 与 recovery 验证完成后，才允许沿用 durable prompt send-intent/acceptance 顺序发送 child prompt。
7. Continue 的 terminal、Cancel、Permission、Activity、Job termination/reconcile、Claim release 全部复用生产路径；`session/load` 不授权任何 Claim release，也不把 Result Recovery 提升为 complete。
8. Windows 完整实现和验证后才开放 `canContinue=true`；非 Windows 保持 false；`tokenUsage=false`。

## 禁止项

- `session/resume`、continuation 中的 `session/new`、第二方法 fallback。
- replay parent prompt、复制聊天文本模拟 continuation、fresh fallback、复用旧 Runtime。
- 新 schema/migration、跨 Runtime prompt identity 复用、猜测/复制 provider request identity。
- 进入 CB8-004、真实 Provider 模型调用、commit、push。

## 验收

覆盖用户 A-J 矩阵：R1→R2 success；session/cwd/replay/lineage/private-state failures；generic policy pre-child rejection；identity isolation；Cancel/Permission/Activity 回归；Runtime/Claim 独立；catalog/Product gating。输出 continuation implementation matrix、fake request sequence 与显式 no-new/no-resume/no-parent-replay assertions。

最终冻结全部本卡差异，由未参与实现的独立只读 `CHILD_AGENT` 做 `FULL_SCOPE` review。P0/P1/P2 必须修复并重审。
