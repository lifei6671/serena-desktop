# Implementation and verification

1. 冻结 baseline、权威 hash、范围排除和现有覆盖；建立 A-I gap matrix。
2. 在 `usage_projection_tests.rs` 补充 provider-neutral JSON matrix、private pollution、lifecycle/actions isolation 与 reopen persistence；必要时强化现有 Codex complete/partial assertions。
3. 复用 `provider_catalog_tests.rs` 的真实 CodeBuddy catalog snapshot，补足与本卡有关的 capability assertion，保持 `cfg!(windows)`。
4. 在 AgentPanel tests 增加 CB9-002 frontend fixture；证明 unknown/null 不显示 0，partial/complete 与 backend actions 稳定。除非测试失败，不改 production。
5. 运行 Rust targeted Usage/Product/catalog tests、Product full module、frontend对应 node tests并记录实际测试数；再运行 cargo fmt/check、clippy --lib、git diff --check。
6. 证明 Runtime/protocol/recovery/schema/migration diff=0，复核三份 authority SHA-256，产出 Product/UI matrix 与验证证据。
7. 冻结 tracked/untracked 完整 target 与 hash，交未参与实现的独立只读 FULL_SCOPE reviewer；修复 P0/P1/P2 后重验重审，最多三轮。

