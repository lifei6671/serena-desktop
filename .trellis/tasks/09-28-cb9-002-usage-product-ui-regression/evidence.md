# CB9-002 Evidence

## Product/UI Usage matrix

| Provider | Public `execution_usage` row | Codex private state | Catalog `tokenUsage` | Expected Product JSON | Expected UI | Evidence |
|---|---|---|---|---|---|---|
| Codex | complete row | allowed but Product does not read it | current catalog contract unchanged | exact persisted fields, `completeness=complete` | formatted total, including real `0` | `usage_product_defaults_and_persisted_values_preserve_null_zero_and_completeness`; AgentPanel Phase 5 + CB9-002 tests |
| Codex | partial row with missing fields | allowed but Product does not read it | current catalog contract unchanged | `completeness=partial`; missing fields remain `null` | total plus `统计不完整`; no inferred fields | same tests |
| CodeBuddy running | no row | none | `false` | `unknown`; all token/context fields `null`; revision `0`; updatedAt `null` | `—`; Cancel remains controlled by backend action | `cb9_provider_neutral_usage_matrix_ignores_codex_private_pollution_after_restart`; AgentPanel CB9-002 test |
| CodeBuddy durable terminal | no row | deliberately polluted historical row | `false` | still `unknown/null`; polluted `900/999` never appears | `—`; Continue remains controlled by backend action | same tests |
| CodeBuddy historical/future public truth | matching valid partial row | irrelevant | still `false` | exact public row is displayed | normal partial rendering if presented | Rust CB9-002 matrix + existing CodeBuddy catalog snapshot |
| Historical unregistered Provider | no row | none | not registered | persisted Provider ID retained; `unknown/null` | Provider ID fallback; `—` | Rust CB9-002 matrix + existing historical frontend tests |
| Historical unregistered Provider | matching valid complete row | none | not registered | exact public row and Provider ID retained | normal complete rendering | Rust CB9-002 matrix |

## F — CodeBuddy public row conclusion

当前公共 Store contract 把 `execution_usage` 作为 provider-neutral historical truth：`product_read` 只通过一次 `LEFT JOIN execution_usage` 读取与 Execution identity 匹配的公共行，再交给公共 `UsageSnapshot`/`UsageProduct` 投影；读取路径没有 capability gate，也不得进入 Codex private state。

因此本卡冻结结论是：匹配 `provider_id='codebuddy'` 的合法公共历史行应显示；这不代表当前 adapter 能生产新 Usage，也不允许把 `tokenUsage` 改为 true。Rust matrix 以 `codebuddy-public` 明确证明“显示公共行”，`codebuddy_catalog_and_refresh_preserve_capability_truth` 同时证明 capability 仍为 false。若公共行 provider identity 与 Execution 不匹配，既有 fail-closed 测试继续拒绝整个 Product read。

## Serialized snapshots

无公共行：

```json
{
  "inputTokens": null,
  "cachedInputTokens": null,
  "cacheWriteInputTokens": null,
  "outputTokens": null,
  "reasoningTokens": null,
  "totalTokens": null,
  "modelContextWindow": null,
  "completeness": "unknown",
  "usageRevision": 0,
  "updatedAt": null
}
```

CodeBuddy 合法公共历史行（capability 仍为 false）：

```json
{
  "inputTokens": 31,
  "cachedInputTokens": null,
  "cacheWriteInputTokens": 2,
  "outputTokens": 7,
  "reasoningTokens": null,
  "totalTokens": 40,
  "modelContextWindow": 200000,
  "completeness": "partial",
  "usageRevision": 8,
  "updatedAt": 80
}
```

## A-I coverage

- A/B：Codex complete/partial 在 detail、observe、list 三条路径逐字段断言；null、0、complete、partial 均保留。
- C/D：CodeBuddy running 与 durable terminal 无公共行均 unknown/null；terminal fixture 含合法形状的污染 private row，读取前后及 Store reopen 后值不变。
- E/F：historical Provider 与 CodeBuddy 公共历史行均由 public row 决定，Provider identity 不丢；capability 不随行存在而变化。
- G：完整矩阵在同一数据库关闭并重开 `StateStore` 后再次跑 detail/observe/list。
- H：AgentPanel CB9-002 test 精确断言 unknown Token `<code>` 为 `—`，Codex complete/partial 文案保持正确，没有新增 UI。
- I：Rust 比较有/无 Usage 的同生命周期 CodeBuddy `status/dispatchState/attention/availableActions`；运行中 Claim 在读取及 restart 前后不变；catalog snapshot 继续使用 `cfg!(windows)` 且 `tokenUsage=false`。

## Scope result

- Product/frontend production code：0 diff。
- CodeBuddy Runtime/protocol/recovery：0 diff。
- Usage projector、`execution_usage` writer、Codex private semantics：0 diff。
- schema/migration：0 diff。
- 真实 bug：未发现；本卡只新增/强化 executable regression tests 与 task-local evidence。

