# Codex 协议兼容检测统一化

## Goal

让 Windows 与 macOS 共用基于 JSON Schema 必要契约子集的 Codex app-server 兼容检测，取消版本和哈希的严格准入，不执行业务 RPC。

## Requirements

- Windows x86_64 与 macOS ARM64 必须复用同一个 Codex app-server JSON Schema 兼容判定器。
- 兼容判定必须覆盖 Serena Desktop 实际使用的 client requests、client notifications、server notifications、server requests，以及解析这些消息所依赖的关键字段和类型。
- 候选协议新增方法、新增定义或新增可选字段时必须允许通过；缺少必要方法、缺少必要字段或关键字段类型改变时必须拒绝。
- Codex version、binary SHA-256 与完整 schema SHA-256 继续作为运行时身份和诊断证据保存，但不得再作为准入白名单。
- 兼容检测阶段只执行 `--version` 与 `app-server generate-json-schema` CLI，不启动 app-server，也不发送任何 JSON-RPC 请求。
- Windows Job Object、macOS Process Group、架构预检、canonical executable、超时、输出上限和二进制文件句柄保留等现有安全与生命周期约束保持不变。
- 正式 Runtime 启动后的 `initialize / initialized` 流程保持不变；本任务不改变业务 RPC、事件解析或恢复语义。

## Acceptance Criteria

- [x] 同一份兼容 schema 在 Windows 与 macOS 验证路径中调用同一个共享校验函数。
- [x] 版本号、binary SHA 或完整 schema SHA 与旧冻结值不同，但必要协议契约完整时不再被拒绝。
- [x] schema 新增方法或新增可选字段时通过兼容检测。
- [x] schema 缺少 Serena Desktop 使用的必要方法、必要字段或关键字段类型不兼容时返回 `CODEX_APP_SERVER_INCOMPATIBLE`。
- [x] 无效 JSON 或不符合 Codex schema 顶层结构的输入稳定失败，不发生 panic。
- [x] macOS 兼容探针不再启动 app-server 或发送 `initialize`；正式连接流程仍执行初始化。
- [x] 相关 Rust 单元测试、格式检查和当前主机可执行的最小回归验证通过。

## Notes

- Intel macOS/Rosetta 仍不在本任务支持范围内，架构边界由现有 macOS preflight 保持。
- 本任务不引入版本范围、远程 allowlist、缓存、自动重试或兼容 fallback。
- 当前开发机未安装 `x86_64-pc-windows-msvc` Rust 标准库，Windows 交叉类型检查无法执行；Windows 接入由共享 validator 单测、CodeGraph 调用点审查和 rustfmt 语法解析覆盖。
