# Codex 协议兼容检测统一化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Windows 与 macOS 使用同一份 JSON Schema 必要契约子集判定，并停止用 version/hash 精确白名单或兼容阶段 RPC 阻止可兼容 Codex。

**Architecture:** 在现有 `compatibility.rs` 中建立纯函数 schema validator；两个平台的 verify 仅负责收集 identity、导出 schema 并调用该 validator。正式 Runtime 的 initialize 与业务协议代码保持原样。

**Tech Stack:** Rust、serde_json、Tokio、Windows Job Object、macOS Process Group、Cargo test

---

### Task 1: 用测试冻结共享协议子集

**Files:**
- Modify: `src-tauri/src/agent/codex/compatibility.rs`

- [x] **Step 1: 写最小成功契约与增量兼容测试**

在 `compatibility.rs` 测试模块构造包含必要 union branch、params schema 和 definition 字段的最小 schema，断言 `validate_schema` 成功；再添加额外方法和额外可选字段，断言仍成功。

- [x] **Step 2: 写破坏性变更失败测试**

分别删除必要 method、写入不可解析的 params `$ref`、删除必要字段、改变关键字段 JSON type，并断言返回 `CODEX_APP_SERVER_INCOMPATIBLE`；另验证可解析的定义重命名仍通过，无效 JSON 同样稳定失败。

- [x] **Step 3: 运行定向测试并确认 RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml agent::codex::compatibility::tests -- --nocapture`

Expected: 因 `validate_schema` 尚不存在或未实现语义校验而失败。

- [x] **Step 4: 实现最小共享校验器**

增加 `pub(crate) fn validate_schema(schema: &[u8]) -> Result<()>`，使用静态需求表检查 method branch、可解析的 params 对象、必要 definition/property 与关键 JSON type。所有新增函数和核心遍历逻辑使用中文注释；不增加第三方依赖。

- [x] **Step 5: 运行定向测试并确认 GREEN**

Run: `cargo test --manifest-path src-tauri/Cargo.toml agent::codex::compatibility::tests -- --nocapture`

Expected: 新增测试全部通过。

### Task 2: 接入 Windows 验证路径

**Files:**
- Modify: `src-tauri/src/agent/codex/app_server/managed.rs`
- Modify: `src-tauri/src/agent/codex/protocol.rs`
- Modify: `src-tauri/src/agent/codex/app_server/tests.rs`

- [x] **Step 1: 更新测试表达新准入语义并确认 RED**

将精确 `CompatibilityIdentity::check(Target::WindowsX86_64)` 测试替换为“不同 version/hash 仍由 schema 契约决定”的测试；运行相关测试确认旧实现失败。

- [x] **Step 2: 修改 Windows verify**

保留 binary/version/schema digest 采集，移除 binary hash 和 identity allowlist 拒绝；读取 schema bytes 后调用 `compatibility::validate_schema`，再构造原有 `CompatibilityEvidence`。

- [x] **Step 3: 删除失效公共内部 API**

若 `Target`、`check_entry` 与 `CompatibilityIdentity::check` 已无真实调用方，则同步删除并更新中文注释，不保留兼容 wrapper。

- [x] **Step 4: 运行 Windows 可跨平台编译的共享测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml agent::codex::compatibility::tests agent::codex::app_server::tests -- --nocapture`

Expected: 相关测试通过；若 Cargo 不接受多个过滤参数，拆成两条等价定向命令。

### Task 3: 接入 macOS 验证路径并移除兼容 RPC

**Files:**
- Modify: `src-tauri/src/agent/codex/app_server/macos_managed.rs`

- [x] **Step 1: 更新 macOS 测试并确认 RED**

把 version/hash 必须命中及 initialize probe 预期改为 schema 契约准入，保留 path/ARM64、probe cancellation 与 Process Group cleanup 断言；运行定向测试确认旧实现失败。

- [x] **Step 2: 修改 macOS verify**

移除 version/hash 精确拒绝、`validate_identity` 和 `app_server_contract`；读取导出 schema bytes、计算 digest 并调用共享 `validate_schema`，其余 evidence 与文件句柄保留逻辑不变。

- [x] **Step 3: 运行 macOS managed 定向测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml agent::codex::app_server::macos_managed::tests -- --nocapture`

Expected: macOS compatibility 与 cleanup 测试通过。

### Task 4: 验证真实 Codex 0.155.1 与回归范围

**Files:**
- Modify: `.trellis/tasks/09-22-codex-protocol-compatibility/implement.md`（勾选执行结果）

- [x] **Step 1: 格式与静态检查**

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`

Expected: PASS。

- [x] **Step 2: 运行 Codex 模块测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml agent::codex -- --nocapture`

Expected: PASS。

- [x] **Step 3: 运行与风险相称的完整 Rust Gate**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`

Expected: PASS。

- [x] **Step 4: 用本机 Codex 0.155.1 导出 schema 并执行同一 validator 路径**

通过现有 macOS discovery/verify 测试入口或最小现有应用命令验证本机 `codex-cli 0.155.1` 不再因 version/hash 不同被拒绝；不得发送业务 RPC。

- [x] **Step 5: 收尾检查**

Run: `git diff --check && git status --short`

Expected: 无空白错误，仅包含本任务文件。

## 执行结果

- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`：通过。
- `cargo test --manifest-path src-tauri/Cargo.toml agent::codex -- --nocapture`：180 passed，0 failed，7 ignored。
- `cargo test --manifest-path src-tauri/Cargo.toml`：1071 passed，0 failed，21 ignored。
- `SERENA_CODEX_SMOKE=/Users/lifeilin/.local/bin/codex cargo test ...real_compatible_macos_arm64_schema_smoke -- --ignored --nocapture`：通过；只执行 version/schema CLI probe。
- `cargo check --target x86_64-pc-windows-msvc`：开发机未安装该 target 的 Rust 标准库，未进入项目类型检查；未安装新 target。
