# CB3-004 Host finding 修复审查

Host 发现旧 rejected-dispatch fixture 会执行真实 Codex，完整 MCP 为 300 passed / 3 failed / 4 ignored。上一轮对该 fixture 的 PASSED 结论已作废；本记录为修复后的独立复审，不代表 Host Gate。

## 最终目标与范围

- 模式：CHILD_AGENT，/root/review（trellis-check），全程只读；主 Agent 记录结果。
- 本轮范围：agent/product.rs 和 agent/product/tests.rs，相对 fixture-repair-baseline 的增量为 +125/-6；其余生产 Authority/locking/routing 不变，原三条 MCP 断言未修改。
- 修复 manifest SHA256：d76a00400ed5350e449616bde02ba91ca76c781a79a3169c8c2957eb5cc332e5，2/2 文件匹配。
- 修复 diff SHA256：f9e840a2909a5780e1931fca3f2cac0f23df2cf728bc5d71d5fd61509d6ee23b。
- 累计 implementation-manifest.json SHA256：e91055b9f0d2d29d249d33cef779e000485d89b3808657ad31df4de7677f12e7，12/12 文件匹配。
- 累计 implementation.diff SHA256：4db4ed121271c4c8647aab8c363e4e139cfd9816940f4aeb4401904f07127768。
- 本轮修复审查：PASSED；Coverage COMPLETE；Freshness FRESH；无剩余修复 findings。

## 已确认的修复

测试构造器直接创建 Registry 并注册 RejectedDispatchProvider；不调用 build_registry/discovery，也不实例化真实 Codex adapter。AgentTaskManager 构造仅初始化字段和 Pool，后续 registry() 命中受控 Registry。fake 为 Available、canExecute=true，execute 在 accepted() 前固定返回 TEST_DISPATCH_REJECTED，无 CLI、文件或 Runtime 操作。

新增测试使用会 panic 的 acceptance sink，并通过真实 Product 创建路径验证 AGENT_OPERATION_FAILED、requestAccepted=true、Execution 已持久化及 Runtime/attempts 行数为零。原三项测试断言未弱化。此前错误投影及 Authority 审查不作为此次 fixture 正确性的替代证据。

## 本轮实际验证

原生 Windows。审查方直接核对 fixture-repair-A1 至 H 日志，没有重复或并行执行 Cargo。

| 检查 | 结果 |
|---|---|
| Host 原三条失败测试 | PASS，各 1 passed / 0 failed |
| 新 fixture 防回归 | PASS，1 / 0 |
| start_routing_tests | PASS，8 / 0 |
| 完整 mcp:: | PASS，303 / 0 / 4 ignored |
| Product | PASS，132 / 0 / 5 ignored |
| cargo check / fmt / git diff --check | PASS，exit 0 |
| Clippy | FAIL，exit 101；仅既有 usage_tests.rs:987 await_holding_lock（await :1012/:1016），按用户例外保留 |

没有本轮遗留缺陷；Host 将独立复跑 Gate。未归档任务、未提交 Git、未进入 CB3-005。
