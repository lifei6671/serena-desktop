# Phase 1：macOS 可编译基线设计

## 1. 目标与边界

本任务只建立 macOS arm64 可编译、可测试的跨平台模块边界。它不实现 Codex macOS launcher、process group、启动令牌、终止证据、数据库 migration 或崩溃恢复；这些能力分别属于 Phase 2A 和 Phase 2B。

Phase 1 完成后：

- Agent 产品层、Work 层和 TaskManager 作为跨平台业务模块存在。
- Windows 继续使用现有 Codex Runtime、Named Job、终止证据和恢复实现。
- macOS 可以初始化产品层、读取历史和执行纯 Store 操作，但所有需要 Codex Runtime 的动作通过稳定 unavailable 边界失败。
- macOS 不创建虚假 Runtime、终止证据或 Claim 释放证据。

## 2. 模块结构

```text
Product / Work / TaskManager（跨平台）
                  |
                  v
          ProviderRegistry（跨平台）
             /                \
 Windows Codex backend      macOS unavailable backend
 discovery/provider/pool    discovery/provider/pool
 runtime/recovery/launcher  no process, no evidence
```

### 2.1 跨平台业务模块

在 `agent/mod.rs` 中移除 `product`、`work`、`task_manager` 的整层 `cfg(windows)`。这三个模块保持单一实现，不复制 macOS 版本。

模块内部只有真正依赖 Windows Runtime 的实现和测试继续受平台条件限制。纯解析、状态投影、Store 查询、历史分页、人工收口和 Work 逻辑在 macOS 编译并运行测试。

### 2.2 Codex 平台后端

`agent/codex/mod.rs` 保持现有公共模块路径，但在模块声明处选择平台实现：

- Windows 继续指向现有 `discovery.rs`、`provider.rs`、`pool.rs`、`runtime.rs` 和 `windows_launcher.rs`。
- macOS 指向小型 unavailable 实现，保持 TaskManager 已依赖的最小契约。
- `protocol` 和不启动进程的 App Server 数据类型保持跨平台；Windows-only managed transport 不被 macOS unavailable 路径调用。

macOS unavailable 后端只承担以下职责：

- discovery 返回以 `BACKEND_UNAVAILABLE` 开头的固定平台诊断。
- 注册 `codex` descriptor，并将 Registry health 设置为 `Unavailable`。
- Provider capabilities 明确不支持 execute、cancel、continue 和 recover。
- pool 只保留 TaskManager/Product 编译所需的停止令牌、幂等 shutdown 和只读状态接口；不持有 child、Runtime 或恢复失败所有权。

不为 macOS 复制 Windows `RuntimeFailure`、Named Job 或终止证据语义。

### 2.3 恢复边界

Windows 保留现有 `task_manager::recovery`。macOS 启动时仍注册可发现但 health 为 unavailable 的 Codex Provider；由于其 `can_recover` 为 false，`AgentTaskManager::reconcile_startup` 不进入 Runtime 恢复。

macOS unavailable 路径不得：

- 调用 Windows recovery observation；
- 把 PID 不存在推断为原 Runtime 已终止；
- 写入 complete termination evidence；
- 自动释放无法证明安全的 Workspace Claim。

因此，历史中的未知 Runtime/Claim 保持原状态，等待 Phase 2B 的平台真实恢复契约或已有 Local Human Authority 人工收口。

### 2.4 Serena 与 Source Write 编译边界

`serena_capability` 仅在 Windows 导入和使用 `contain_process`、`terminate_managed_job`。Phase 1 不为 macOS 伪造 Job 等价物；任何尚未具备完整进程树所有权的长期 Runtime 启动都必须返回既有稳定失败边界。Unix index 的现有独立 process group 不在本任务中重写。

`source_write_atomic_replace` 将 `ErrorKind` 放到实际跨平台使用范围，保留 Windows `Read` 和摘要相关导入的原条件编译。该修改只修正编译边界，不改变 no-clobber 或原子替换语义。

## 3. 数据流与错误契约

### 3.1 启动

1. `AgentProductService` 在 macOS 正常创建 Store 和 TaskManager。
2. Codex discovery 返回固定 `BACKEND_UNAVAILABLE` 诊断，写入现有 `backend_error` 字段。
3. TaskManager 注册 `codex` Provider，但 Registry health 为 `Unavailable`。
4. 启动 reconciliation 枚举 descriptor 后发现 `can_recover == false`，不读取或伪造 Runtime 终止证据。

### 3.2 读取操作

List、Observe、History 和 Local Human Authority 人工收口继续直接使用 Store。它们不依赖 Codex child，因此保持可用。

### 3.3 Runtime 操作

Runtime 操作继续沿用各自现有的产品层失败投影，不强行统一为一个错误码：

- Start 先持久化 `dispatch_pending` Execution，在 backend discovery 失败时返回 `BACKEND_UNAVAILABLE`。
- ResumePending 复用已持久化 Execution，在同一 discovery 失败边界返回 `BACKEND_UNAVAILABLE`。
- Continue 必须先通过 Store continuation preflight；unavailable Provider 的 `can_continue=false`，因此对合法 source 返回 `AGENT_CONTINUE_NOT_ALLOWED`。
- Cancel 通过已注册但 unavailable 的 Provider 进行能力判定，返回 `AGENT_PROVIDER_UNAVAILABLE`。

四条错误路径都不得启动进程或创建 `runtime_instances`。Start 已持久化的 Execution 在缺少真实终止与释放证据时保留 Workspace Claim；ResumePending 和 Cancel 的失败不得伪造证据或释放该 Claim，Continue 不得创建 child Execution。

## 4. 测试设计

### 4.1 新增覆盖

- 编译期覆盖：macOS 能解析 `product`、`work`、`task_manager` 及其上层 commands/MCP 调用。
- unavailable discovery 返回固定 `BACKEND_UNAVAILABLE` 前缀。
- Registry 中 `codex` descriptor 可见、health 为 unavailable，执行解析返回 `AgentProviderUnavailable`。
- macOS 产品层读取类操作继续使用 Store；Start/ResumePending 返回 `BACKEND_UNAVAILABLE`，Continue 返回 `AGENT_CONTINUE_NOT_ALLOWED`，Cancel 返回 `AGENT_PROVIDER_UNAVAILABLE`。四条路径均无 Runtime 记录或伪造证据，缺少终止证据的 Claim 保留。
- macOS 启动 reconciliation 不释放无法证明安全的 Claim。
- Unix Source Write 的 AlreadyExists 映射测试继续通过。

### 4.2 现有测试边界

- 只依赖业务或 Store 的测试移出不必要的 Windows 限制。
- 使用 Win32 handle、Job Object、Windows launcher、真实 Runtime fixture 或 Windows recovery evidence 的测试保持 `cfg(windows)`。
- 不删除、跳过或弱化仍然有效的 Windows 测试；只修正其平台归属。

### 4.3 验证 Gate

按项目清单执行：

```bash
npm ci
npm run lint
npm run build
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo check --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

本任务只有在 macOS 主机全部通过后才满足退出条件。Windows 行为仍由现有 Windows CI 和后续双平台 Gate 验证。

## 5. 兼容性与回滚

- 不修改数据库 schema、现有 Windows error code 或 Windows Runtime 文件内容，除非是平台声明所需的最小条件编译调整。
- 不改变 Tauri command、MCP wire contract 或前端数据形状。
- 新增代码必须包含中文函数注释和关键逻辑注释。
- 若 unavailable facade 需要复制大段 TaskManager/Product API，应停止并回退设计，而不是扩大 stub；正确边界是 Codex 平台后端。
- 回滚时移除 macOS unavailable 文件并恢复模块声明即可，不需要数据迁移。
