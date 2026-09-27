# CB6-001 Design

## Boundary

本卡只建立 Provider presence：CodeBuddy skeleton、无进程 discovery、Registry bootstrap 和 Admission Health refresh。任何 ACP initialize/session/prompt、Agent Runtime、Windows Job、Execution/Claim mutation 均在边界之外。

## Discovery model

- `DiscoveryInput` 是 crate-private resolver 输入；生产当前不提供 explicit PATH，因此传空，不新增公共配置字段。
- PATH entry 按 `explicit > process > HKCU > HKLM > safe common` 组成内部 projection；变量展开和大小写不敏感去重发生在候选解析前。
- 候选仅为 `codebuddy.exe/.com/.cmd/.bat`。先解析 canonical CodeBuddy Code CLI；`buddycn` 只在失败诊断中记录 boolean hint。
- `.exe/.com` 可作为 resolved executable；`.cmd/.bat` 只作为 npm wrapper 输入，必须解析到真实 `node.exe` 和可读 CLI script。
- `ResolvedLaunchSpec` 保存绝对 executable、argv 与本次 PATH projection，供后续 CB6-002 消费；`DefaultLaunchDescriptor` 仅表达 Release-owned `codebuddy --acp` 默认描述，两者不互相持久化。

## Metadata and diagnostics

- npm package metadata 只读 best-effort 解析；`version` / `baseVersion` 缺失或类型异常仅形成 `missing/malformed` 状态，不改变 Available 判定。
- 成功 provenance 只包含 source enum、resolved executable、wrapper/metadata 状态和 buddycn hint；失败只返回稳定 `CODEBUDDY_BINARY_NOT_FOUND` 与 buddycn boolean。
- 不保存完整 PATH/env、registry raw value、wrapper 正文、token 或 credential。

## Provider and Registry

- `CodeBuddyProvider` 总是注册；discovery 成功为 `Available`，失败为 `Unavailable`。
- skeleton 的六项 capability 全为 false；所有 lifecycle 方法 fail closed 为 `AGENT_PROVIDER_CAPABILITY_UNSUPPORTED`，因此查询/刷新无法启动任何执行面。
- `AgentTaskManager::build_registry` 保留 Codex 注册语义，再注册 CodeBuddy；CodeBuddy 失败不能使 Desktop bootstrap 失败。
- `refresh_provider_health` 按已注册 provider 选择对应 admission resolver，再使用同一个 `replace_registered`/`ProviderHealth` 路径发布结果。

## Test strategy

- discovery 单元测试使用临时目录和显式 `DiscoveryInput`，不读取或启动真实 CodeBuddy。
- task-local discovery override 只用于 Registry/refresh tests，保证不依赖开发机安装状态。
- 产品目录 fixture 冻结 CodeBuddy registered/unavailable、全 capability false；持久化表快照证明查询/刷新无 Runtime/Execution/Claim 副作用。

## Material difference check

当前代码的 Registry、ProviderCapabilities、ProviderHealth 与设计一致；唯一已知缺口是 CodeBuddy 未注册及 refresh 的 Codex-only 分支，不构成设计冲突。公共配置没有 explicit executable/PATH 字段，本设计不新增该字段。
