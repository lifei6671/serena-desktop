# P0-007 与 P2D-006 执行计划

1. 读取权威 design/task breakdown 与 Batch B 实现，定位 Provider descriptor、RuntimeSlot API、MCP registry/router 和现有 CodeGraph protocol helper。
2. 用独立临时 A/B roots 和隔离用户目录执行 P0-007；将命令、identity、root、marker、latency、RSS 和结论持久化到 `research/p0-007-codegraph-runtime-evidence.md`。
3. 若证据完整，冻结三项 runtimePolicy 并先实现/测试 CodeGraph Provider 的 readiness、start/call/stop opaque handle；否则停止。
4. 接入现有 registry/manager/MCP 路由，按权威 Remote gate 选择恢复或维持 disabled；不接回旧 `mcp/codegraph.rs` Binding。
5. 增加 deterministic tests：barrier/channel/permit，不使用 sleep 证明并发或取消。
6. 运行 targeted Rust tests、`cargo check --locked`、owned rustfmt、diff check；Linux 只用项目 Docker runner。
7. 完成只读交付评审，报告 evidence/policy/implementation/tests；不 commit/push，不进入后续 P2D。
