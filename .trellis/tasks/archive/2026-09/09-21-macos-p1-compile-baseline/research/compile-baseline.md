# Phase 1 macOS 编译基线

## 复现环境

- 主机：macOS 26.5.2，Apple Silicon `arm64`。
- Rust：`rustc 1.96.0`，`cargo 1.96.0`。
- 命令：`cargo check --manifest-path src-tauri/Cargo.toml --locked`。
- 结果：失败，23 个 Rust 错误、3 个警告，与 `docs/macos-porting-checklist.md` 的冻结基线一致。

## 根因分类

1. `src-tauri/src/agent/mod.rs` 将 `product`、`work`、`task_manager` 整体限制为 `cfg(windows)`，但 Store、Tauri commands、MCP orchestration、Serena 和应用初始化仍无条件引用这些跨平台业务模块。由此产生大部分未解析导入及后续类型推断级联错误。
2. `src-tauri/src/agent/codex/mod.rs` 将 `pool`、`provider`、`runtime`、`windows_launcher`、`discovery` 限制为 Windows；`task_manager` 同时直接依赖这些模块。macOS 需要稳定的 Provider unavailable 和恢复边界，不能直接解除所有 Windows 条件编译。
3. `src-tauri/src/serena_capability.rs` 无条件导入 Windows 专属 `contain_process`、`terminate_managed_job`；相应字段只在 Windows 存在，应将导入与调用限制在平台边界。
4. `src-tauri/src/mcp/source_write_atomic_replace.rs` 的 `ErrorKind` 只在 Windows 导入，但 Unix `create_new_file` 的 no-clobber 发布错误映射也使用它。

## 结构证据

- `AgentTaskManager` 的业务流程通过 `ProviderRegistry` 获取 Provider；Registry 已具有 `ProviderHealth::Unavailable` 和稳定的 `AgentProviderUnavailable` 错误。
- Windows Codex Provider、Runtime、Job Object 和 recovery observation 紧密绑定，不能在 Phase 1 直接移除其 `cfg(windows)`。
- `AgentProductService` 的历史、观察和人工收口直接依赖 Store，可保持跨平台；创建、继续、取消等 Runtime 动作应通过 Provider unavailable 边界失败。
- `task_manager::recovery` 当前直接依赖 Windows Runtime/Job 证据；macOS Phase 1 只能提供不伪造终止证据的显式 unavailable/recovery 边界，真实实现属于 Phase 2。

## 候选方案

### 方案 A：只平台化 Codex Runtime 与恢复后端

- `product`、`work`、`task_manager` 保持同一份跨平台业务实现。
- 在 `codex` 与 `task_manager::recovery` 的现有边界选择 Windows 实现或 macOS unavailable 实现。
- macOS 注册同一 Codex descriptor，但健康状态为 unavailable；读取类 Product 能力继续使用 Store，Runtime 动作返回稳定 unavailable。
- 优点：符合既有 Provider Registry 结构，Windows 代码改动最小，Phase 2 可替换 unavailable 后端。
- 风险：需要补齐少量 pool/recovery facade 契约，必须避免复制 Windows Runtime 语义。

### 方案 B：为 macOS 建立完整 TaskManager stub

- 整体替换 `AgentTaskManager`，为 Product 调用逐个提供 stub。
- 优点：短期容易隔离 Windows 编译。
- 风险：复制较大的产品 API，容易与 Windows 行为漂移，后续 Phase 2 需要再次重构。

### 方案 C：继续裁剪 Agent 产品层及所有上层调用

- macOS 不编译 Product、Work、TaskManager，并在 commands/MCP/UI 层大量增加 `cfg`。
- 优点：最快让局部构建通过。
- 风险：违反已确认的功能对齐和清单模块边界，扩大条件编译传播，不采用。

## 建议

采用方案 A。Phase 1 只建立平台选择与明确 unavailable 行为，不实现 process group、启动令牌、终止证据或数据库迁移；这些内容留给 Phase 2A/2B。
