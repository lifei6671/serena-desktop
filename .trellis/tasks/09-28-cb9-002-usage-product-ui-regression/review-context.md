# Independent FULL_SCOPE review context

## Authority and target

- Delivery unit：CB9-002 Usage Product/UI Regression Gate。
- Baseline：`feat/codebuddy` / `c631a6707d2da0d3e0c8dbf440701e30e5739461`，起始 clean。
- Review mode：独立、只读、FULL_SCOPE；reviewer 未参与实现，禁止修改文件。
- Frozen target identity：`review-target.sha256` 中列出的全部 delivery-owned tracked/untracked 文件内容，加上 baseline 到当前工作区的完整 diff。
- Review context：本目录 `prd.md`、`design.md`、`implement.md`、`evidence.md`、`verification.md`，以及三份 authority。

## Required review lenses

1. 是否新增任何 Product/frontend production bug 或无必要复杂度。
2. 是否出现 `provider == "codebuddy"` 的 Product Usage 业务分支。
3. 是否把 null/unknown 转为 0，或从 breakdown 推导 total。
4. 非法 `codex_execution_usage_state` 是否可能污染 CodeBuddy/historical Provider。
5. `tokenUsage` capability 是否与公共历史数据展示错误耦合；CodeBuddy 必须保持 false。
6. Codex complete/partial/zero 是否回归。
7. frontend 是否制造 `0 token` 假象或隐藏 lifecycle actions。
8. tests 是否真正覆盖 detail/observe/list、restart、Claim/status/actions 与 provider identity，而不是只断言 helper。
9. 是否越界触碰 Runtime、protocol、recovery、projector、writer、schema/migration、Codex private semantics 或 Phase10。

## Required output

返回 review mode、reviewed target identity、完整覆盖范围、按 P0/P1/P2/P3 分类且带证据的 findings、`PASSED | BLOCKED | UNAVAILABLE` gate 与 remaining risk。P0/P1/P2 任一存在都视为需要修复并重审；没有 finding 可明确写 `No findings`。

