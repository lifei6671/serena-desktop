# CB7-001 Descriptor / Capabilities

权威：当前用户矩阵；docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md CB7-001；technical-design §7、§33；冻结 CB5/CB6 evidence。

范围：审计 production，已有行为正确则仅补测试和 evidence。id=codebuddy、displayName=CodeBuddy；version 仅 product_version。Windows canRecover=true，非 Windows=false；canExecute/canCancel/activity/canContinue/tokenUsage 全 false。health、enabled 独立且 availableForNewExecution 恒 false。

验收：精确 descriptor（有/无 product_version、base/hash/path 不泄漏）、capabilities；found/missing CLI；Registry registered 与 admission 区分；disabled catalog 保留 descriptor/recovery 信息；refresh 重建保留 Store/owner authority；Codex catalog 回归。平台适配 fixture 不混淆 Windows 与非 Windows事实。

禁止：execute/session/new/prompt/terminal/result/Claim 实现，schema/migration/runtime recovery、UI/MCP 字段、真实 CodeBuddy/CB5 probe、commit/push、CB7-002。

验证：native Windows focused provider/registry/catalog/admission tests；现有 CB6-005 refresh authority 测试；cargo fmt/check/clippy；精确记录 usage_tests await_holding_lock baseline；git diff --check/scope。非 Windows compile 语义静态审查；Linux 执行仅允许项目 Docker runner，未发现 runner 则报告环境不可用，不使用 WSL。最终冻结源码 SHA256，独立只读 review；全部本卡 Gate 满足才 completed。
