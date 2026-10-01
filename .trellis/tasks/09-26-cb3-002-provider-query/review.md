# CB3-002 独立交付审查

- review mode: CHILD_AGENT
- strategy: FULL_SCOPE / Tier 3 / Standard Mode
- coverage: COMPLETE
- gate: PASSED
- Host Gate: PENDING；本结果不代替 Host acceptance。
- frozen target identity: code-hashes.json SHA256 `1e5638e896409d7787820c7ce69c2456e7de4fd6fa2c7b4a8b735667a7569965`。

## Findings (fixed)

无。本轮只读审查，未修改任何冻结代码或证据。

## Findings (not fixed)

本卡没有发现需要修复的缺陷。已知任务外 Clippy blocker 保持不变：`src-tauri/src/agent/store/usage_tests.rs:987` MutexGuard 跨越 `:1012`、`:1016` await。用户明确禁止扩范围修复。

## Coverage and evidence

逐项读取并审查 code-hashes.json 全部 8 个代码文件的交付改动和 5 个 JSON artifacts；独立重算全部 13 个 SHA256，全部匹配。读取 check.jsonl、prd/design/implement/review-context/evidence，核验两份设计/任务文档 SHA256 与 Host 引用一致。复核设计 §7、§7.1、§26 和 CB3-002 卡。

三个原有脏文件以 task/baseline 副本为差分基线，副本字节 hash 与 baseline-hashes.json 匹配。84 个旧脏文件中仅 product.rs、provider/mod.rs、provider/registry.rs 发生本卡配套变更，其他 81 个 hash 不变；三个文件仅增加 JsonSchema derive/import 和 cfg(test) 注入构造器。没有覆盖旧实现。

- `mcp/orchestration/dto.rs`：Providers 空 struct variant 保留 tagged enum deny_unknown_fields；只接受 action；parse 将 providers 的非法字段映射到 INVALID_PARAMS。QueryData 直接承载 Product ProviderCatalogSnapshot，没有复制 Authority。
- `mcp/orchestration.rs`：查询在原有 agentEnabled/Product 初始化门禁之后直接 provider_catalog，提前返回现有 query envelope；不进入 workspace resolution、Start、dispatch 或 health probe。读取失败仅返回安全 AGENT_OPERATION_FAILED，没有暴露内部错误。
- `agent/product.rs` 及两个 provider 文件：schema derive 不改变序列化/业务行为。读取链仅复制 config、枚举 descriptor、读取 cached health/capabilities 和 roleRouting；不写 Registry、policy 或 Store。cfg(test) constructor 不扩大生产 API。
- `mcp/provider_query_tests.rs`：完整读取 4 个新增测试；50 个额外字段/值组合同时验证 parser 拒绝与实际 schema rejection；真实 Streamable HTTP rmcp list/call_tool 返回 Product JSON。已有 Execution/Claim 完整行、Runtime 表、配置内存/磁盘 bytes、Unavailable health、空 workspace registry 和 binding 在成功/失败之后保持一致；Provider lifecycle 和 Runtime connect 使用 panic guards。受控 descriptor 身份错误使真正读取路径失败，并验证脱敏 envelope。真实 Codex 测试与 CB3-001 fixture exact equality。
- `mcp/orchestration_tests.rs`：新 output schema 负例覆盖必需 Provider 字段和 health/enabled 类型。两处 list fixture 从固定旧 Provider 字面值调整为已验证 detail 的 Provider 投影，与任务前 Product 已存在的 version 行为一致；完整 list equality 保留，get/list/observe 回归日志通过。
- `mcp/registry.rs`：仅同步 agent_query descriptor hash 和 action 数量。Remote 固定 registry 表面没有新增 mutation；agent_execute 旧 hash 保持 `d889e222c362c1e898a5aabe1897d8e4abb3c8e95962b5d0daf1c352be5f02da`，Start 不接受 taskRole/providerId。
- 五份 JSON artifacts：独立比较 descriptor 的 inputSchema/outputSchema 与两个 schema 文件；descriptor/input/output 与 orchestration-final2.log 中实际打印值语义相等。规范化 descriptor SHA256 独立重算为 `96bf06b880b5ec4972a42dd83cb1dc7e8f1f8c797f760dbad24c333466d68ae0`，与 Rust gate 一致。providers 分支只有 action 属性、required action、additionalProperties=false。

## Verification

cwd: `E:\wx_lifeilin\github.com\lifei6671\serena-desktop`；Native Windows PowerShell。

已读取最终原始日志，未重复运行会创建构建/测试输出的验证：

- `cargo test --manifest-path src-tauri/Cargo.toml mcp::orchestration_tests --lib -- --nocapture`：PASS，14 passed，0 failed。
- `cargo test --manifest-path src-tauri/Cargo.toml mcp::registry::tests --lib`：PASS，20 passed，0 failed。
- `cargo test --manifest-path src-tauri/Cargo.toml mcp::orchestration::dto --lib`：PASS，1 passed，0 failed。
- `cargo test --manifest-path src-tauri/Cargo.toml agent::product::provider_catalog_tests --lib`：PASS，4 passed，0 failed。
- TypeCheck `cargo check --manifest-path src-tauri/Cargo.toml`：PASS（日志完成 dev profile）。
- Lint `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`：FAIL，仅上述已知任务外 blocker；没有将其记作 pass。
- 本 reviewer 独立执行 `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` 和 `git diff --check`：PASS；Git 仅 CRLF 提示。
- 本 reviewer 只读 Python hash/JSON 一致性核验：PASS。首次读取中文 artifact 使用系统 GBK 导致 UnicodeDecodeError，明确指定 UTF-8 后通过；未修改 artifact。

总计 39 项相关测试通过。无必要新增 spec convention；已有冻结设计足以描述本卡契约。未执行 Linux、远端部署或真实 CodeBuddy Runtime 验收；这些不在本卡范围。现有全局 agentEnabled=false 时仍隐藏工具/拒绝调用，沿用已有门禁。剩余外部步骤仅 Host 独立 acceptance。
