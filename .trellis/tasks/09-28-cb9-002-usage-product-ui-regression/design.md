# Design

## 既有数据流

`execution_usage` 是 provider-neutral public historical truth。`product_read` 在初始 Execution query 中 `LEFT JOIN` 公共行，经 `UsageSnapshot` 校验后交给 `UsageProduct::project`；无行使用稳定的 unknown/null DTO。Product 不应访问 `codex_execution_usage_state` 或 `codex_thread_usage_epochs`。

Provider catalog capability 表达当前 adapter 能否生产 Usage；它不应反向隐藏已经持久化的、identity 匹配的公共历史行。因此 CodeBuddy 当前 `tokenUsage=false` 时：无公共行返回 unknown/null；若未来或历史数据库已有合法 `provider_id='codebuddy'` 公共行，Product 仍按公共 Store contract 展示该行，但 catalog 继续声明 false。

## 测试设计

在 Product regression fixture 中同时创建：Codex complete/partial、CodeBuddy running/terminal unknown、CodeBuddy public historical row、historical unknown Provider no-row/public-row。向一个 CodeBuddy Execution 人工插入合法形状但语义非法的 Codex private state 污染行，冻结其原始内容与 lifecycle/actions，再通过 detail/observe/list 和 Store reopen 证明公共结果不受污染。

frontend 沿用既有 `ExecutionView` fixture，不新增 UI。专项测试同时输入 Codex complete/partial 与 CodeBuddy unknown，断言 unknown/null 呈现为 `—`、不出现伪造 0，并证明 action 仍只由 `availableActions` 控制。

## 生产修改门

先只新增/强化测试。若所有测试通过，production diff 保持 0；只有测试暴露真实 Product/UI bug 时才进入最小修复，且不得新增 `provider == "codebuddy"` Usage 分支。

