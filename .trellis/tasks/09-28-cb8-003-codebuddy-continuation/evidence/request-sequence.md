# Fake request sequence evidence

证据来自 native managed child fixture；没有真实 CodeBuddy 模型调用。

## Success

按 wire 顺序：

1. `initialize`
2. `session/load`
3. `session/prompt`

`session/load.params` 精确断言：

```json
{
  "sessionId": "exact-session",
  "cwd": "<projected child workspace>",
  "mcpServers": []
}
```

fixture 在 load response 前发送 exact-S1 parent history sentinel。生产路径验证后丢弃该历史；child `session/prompt` 只包含 `child prompt`，测试显式断言不含 `parent prompt sentinel`。

## Forbidden methods

- continuation 请求日志精确等于 `initialize, session/load, session/prompt`。
- `session/new` 和 `session/resume` 在 fixture 中会写 forbidden marker 并立即失败；成功与全部负向测试均确认 marker 不存在。
- 不存在第二 recovery method 或 fresh fallback。

## Fail-closed matrix

| 模式 | wire 截止点 | 结果 |
|---|---|---|
| wrong-session replay | initialize → load | `CODEBUDDY_CONTINUATION_VALIDATION_FAILED`，未 accepted/prompt |
| load response sessionId mismatch | initialize → load | 同上 |
| missing history | initialize → load | 同上 |
| unusable catalog-only history | initialize → load | 同上 |
| empty object / empty text / whitespace / malformed typed text | initialize → load | 同上 |
| missing loadSession capability | initialize | 同上，未发送 load |
| parent/cwd/generation drift | 无 wire | 同上，未创建 Runtime |

所有已创建 R2 的负向 case 都由既有 Job convergence 得到 `terminated + complete termination evidence`；这不是 `session/load` 直接授权。

R1 terminal fixture 还冻结 exact `codebuddy.ai/requestId=source-provider-request`，R2 terminal 不返回该字段；测试证明 child `provider_request_id` 不继承 source。
