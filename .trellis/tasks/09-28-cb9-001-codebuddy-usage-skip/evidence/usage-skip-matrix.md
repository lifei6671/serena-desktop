# CodeBuddy Usage Skip Matrix

Authority：CB5-005 Host attempt 4 `evidence/attempt-4/usage-analysis.json` 与最终 `decision.md` / `review-final.md`。观察到 9 条真实 `usage_update` 不等于公共 Usage contract PASS。

| 场景 | 已观察证据 | 未证明/不允许推断 | CB9-001 结论 | Public expected |
|---|---|---|---|---|
| fresh | P1 窗口有 2 条事件；可见 `used`、`size`、`_meta` | exact Prompt 未绑定；字段单位、scope、起始 baseline 与 terminal coverage 未冻结 | unsupported/inconclusive | `unknown/null` |
| multi-turn | P1/P2 均有事件；`used` 样本从 45332 变化为 26006 | 数值下降不能证明 reset，也不能区分 per-turn、session cumulative 或其他 private scope；不得猜 total | unsupported/inconclusive | `unknown/null` |
| restart | `P3_AFTER_RESTART` 有 3 条事件；`used` 样本为 26006、26006、26063 | restart 前后数值相近不能证明延续或 reset；Runtime/Session/Prompt scope 未冻结 | unsupported/inconclusive | `unknown/null` |
| continue | Attempt 4/5 已证明 `session/load` continuation，但 Attempt 5 未重跑 Usage | continuation 成功不证明 Usage 累计语义；Attempt 4 事件仍未 exact Prompt-bound，不能复用 Codex Thread epoch/baseline | unsupported/inconclusive | `unknown/null` |
| late / terminal | P1/P2/P3 的已归窗事件均在 terminal 前；分析中没有 after-terminal 样本 | 因事件未 exact Prompt-bound，不能据此证明 terminal coverage 完整或不存在 late event；逐字段均为 `unknown_unbound` | unsupported/inconclusive | `unknown/null` |
| identity | 部分 `_meta` 含 request/message/session-like identifiers | Analyzer 冻结 `exactPromptIdentity=null`、整体 `exactPromptBound=false`；未证明其等于 Serena exact Prompt identity，也不能用文本/时序猜绑定 | unsupported/inconclusive | `unknown/null` |

## Frozen outcome

- `publicUsage=EXPLICITLY_UNSUPPORTED_FOR_INITIAL_RELEASE`
- `ProviderCapabilities.token_usage=false`
- CodeBuddy public Usage：`unknown/null`
- 缺少 Usage 绝不解释为 0
- 禁止 CodeBuddy → Codex private Usage path
- 未来必须通过新的 Usage Contract Gate 后再实施 Provider-owned CodeBuddy projector
