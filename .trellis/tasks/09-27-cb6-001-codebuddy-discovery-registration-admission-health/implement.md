# CB6-001 Implementation Plan

1. 新增 `agent/codebuddy` 模块，定义 default descriptor、resolved LaunchSpec、无进程 discovery 与 fail-closed Provider skeleton。
2. 在 `AgentTaskManager` bootstrap 注册 CodeBuddy，并让 health refresh 支持 Codex/CodeBuddy，保持公共错误分类不变。
3. 添加 discovery、provider、Registry/Product/TaskManager focused tests；更新默认 catalog fixture。
4. 运行 focused tests、受影响 Rust 检查和 `git diff --check`；不运行真实 CodeBuddy/ACP。
5. 冻结最终 scope/hash，交给独立只读 reviewer；若修复 review finding，重新验证并重新冻结。

## Verification boundary

- Windows host Rust 验证只证明本卡 discovery/registration/admission；不作为 Linux evidence。
- 不执行任何 CB5 harness mode、CodeBuddy binary、ACP handshake、Runtime 或 Session。
- 已知无关 Clippy baseline 不在本卡修复；如阻断则按首个 material error 如实报告。
