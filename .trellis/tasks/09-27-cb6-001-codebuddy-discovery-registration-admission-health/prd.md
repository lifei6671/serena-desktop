# CB6-001 CodeBuddy Discovery Registration Admission Health

## Goal

Register a capability-conservative CodeBuddy provider skeleton and implement no-process Windows discovery, registration bootstrap, and admission health without starting ACP or runtime.

## Requirements

- 编译期注册 capability-conservative `CodeBuddyProvider` skeleton；`canExecute`、`canContinue`、`canCancel`、`canRecover`、`activity`、`tokenUsage` 全部保持 `false`。
- 实现不会创建进程的 CodeBuddy Code ACP CLI discovery；缺失时 Desktop 与 Codex 正常，CodeBuddy 仍以 registered + unavailable 出现在 Registry。
- discovery 来源顺序固定为 crate-private explicit PATH 输入、Desktop 当前进程 PATH、HKCU Path、HKLM Path、safe common dirs；展开 `%VAR%`，按 Windows 大小写不敏感去重并保留首次出现顺序。
- 只接受 `.exe/.com/.cmd/.bat` 候选；拒绝 `.ps1` 和无扩展 Unix shim。`buddycn` 仅产生脱敏诊断提示，不能成为 ACP executable。
- npm wrapper 必须解析为真实 executable + argv；典型安装解析为 `node.exe`、已安装 `@tencent-ai/codebuddy-code/bin/codebuddy` 和 `--acp`，wrapper 本身不能成为最终 executable。
- product/base version metadata 仅 best-effort 诊断；缺失或异常不阻断 discovery，不建立版本/hash whitelist。
- resolved LaunchSpec、内部 PATH projection 与 Release-owned default launch descriptor 分离；不得把内置 command/args 持久化为用户配置。
- `refresh_provider_health` 支持 CodeBuddy admission discovery，使用 provider-neutral health/error 分类，且不创建 ACP、Runtime、Session、Execution 或 Claim。
- 诊断不记录完整 PATH/env、token、credential 或 wrapper 正文；只保留必要的安全 provenance。
- 不运行任何 CB5 probe 或真实 CodeBuddy CLI；不实现自动安装、`npx` 下载、StateStore migration、Continue/Usage/Cancel、UI 或远程协议。

## Acceptance Criteria

- [x] missing、found、buddycn-only、metadata missing/malformed 均有 focused tests。
- [x] stale process PATH + registry refresh、来源优先级、变量展开、大小写去重和扩展过滤均有 deterministic tests。
- [x] npm wrapper 解析为真实 `node.exe + script + --acp`，不把 `.cmd/.bat` 当最终 executable。
- [x] resolved LaunchSpec 与 default descriptor 分离，诊断不暴露完整 PATH/env。
- [x] 默认 Registry 同时包含 Codex 与 CodeBuddy；CodeBuddy unavailable 不影响 Codex。
- [x] `refresh_provider_health(codebuddy)` 不创建 Runtime/Session/Execution/Claim。
- [x] 最小充分 Rust 验证、scope 检查和独立/隔离 review 通过。

## Notes

- 权威需求：`docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md` CB6-001。
- 合同依据：`docs/technical-design-multi-agent-provider-codebuddy-v0.1.md` §6.1、§14.1～§14.2.1、§29、§31。
- 发现 Material Contract Difference 时停止受影响实现，不扩大配置模型或弱化边界。
