# Codex 协议兼容检测统一化设计

## 背景

当前 Windows 与 macOS 都会导出 `codex_app_server_protocol.schemas.json`，但最终仍按 version、binary SHA 与完整 schema SHA 的精确组合放行。任何不影响 Serena Desktop 的增量协议变化都会被误判为不兼容；macOS 还额外启动 app-server 执行 `initialize` 探针，导致两个平台的准入逻辑不同。

## 边界

- 保留现有 executable、架构、进程收口、超时与输出大小检查。
- 兼容阶段只运行 CLI 并读取 schema 文件，不产生 JSON-RPC 流量。
- 正式 Runtime 的初始化、业务方法、通知解析和恢复流程不变。
- version 与三个 SHA 字段仍保存在 `CompatibilityIdentity`，用于证据、日志和运行时绑定，不再用于判断兼容。

## 共享契约校验

`src-tauri/src/agent/codex/compatibility.rs` 提供唯一的 schema 校验入口，接收导出的 schema 字节并返回现有 `protocol::Result<()>`。

校验分两层：

1. 检查顶层 `definitions`、`ClientRequest`、`ClientNotification`、`ServerNotification`、`ServerRequest` 与相关协议定义存在且结构正确。
2. 对 Serena Desktop 实际使用的消息建立小型静态需求表：所属 union、method、可解析为对象的 params schema、必要 definition、必要字段和 JSON 类型。候选 schema 必须包含这些要求，但可以重命名内部定义、折叠 `v2` 命名空间，也可以增加 union branch、definition、property、非必要字段和 enum 值。

不实现通用 JSON Schema subsumption，也不复制整份冻结 schema。校验器只回答“当前应用依赖的可观察 wire contract 是否仍存在”，避免把无关协议扩展误判为破坏。

错误统一使用 `CODEX_APP_SERVER_INCOMPATIBLE`，消息指出首个缺失或改型的 contract facet，便于定位升级影响。

## 平台接入

### Windows

`app_server/managed.rs::verify` 保留 canonical absolute path、只读文件句柄、binary digest、`--version`、schema 导出与 schema digest。删除旧 binary allowlist 与 `CompatibilityIdentity::check(Target::WindowsX86_64)`，读取 schema 字节并调用共享校验器。

### macOS

`app_server/macos_managed.rs::verify` 保留 ARM64 preflight、隔离 CLI Runtime、Process Group 收口、binary digest、`--version`、schema 导出与 schema digest。删除 version/hash 精确比较、平台 identity allowlist 和 `app_server_contract` 探针，读取同一 schema 文件并调用共享校验器。

### 旧冻结常量

保留测试 fixture 和现有诊断仍需要的冻结 identity 常量，但移除其“准入白名单”语义。`CompatibilityIdentity::check`、`Target` 与 `check_entry` 若无剩余真实调用方则直接删除，不保留空 wrapper。

## 测试策略

- 先为共享校验器写失败测试：完整最小契约通过、额外方法/可选字段通过、可解析的 params 定义重命名通过、必要方法缺失失败、params 引用不可解析失败、必要字段缺失失败、关键字段类型改变失败、无效 JSON 失败。
- 平台验证路径通过已有测试或新的小型 seam 证明不再比较冻结 version/hash，并证明两个平台都调用共享入口。
- macOS 删除 initialize 探针后更新对应生命周期测试，只保留 CLI probe 的 bounded cleanup 契约。

## 风险与回滚

主要风险是假阴性：静态需求表遗漏应用实际依赖的消息字段。通过从当前 client 构造、response parser、notification parser 和 server-request handler 汇总需求并用单元测试冻结来控制。若上线后发现遗漏，可恢复上一提交的精确 allowlist；持久化 schema 与数据库均未变化，不需要数据迁移。
