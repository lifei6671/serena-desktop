# CB3-002 implementation evidence

状态：Implementation verified；独立 review / Host Gate pending。仅 CB3-002，无 commit。

执行环境：Native Windows PowerShell；cwd `E:\wx_lifeilin\github.com\lifei6671\serena-desktop`。未使用 WSL，未声称 Linux 或已部署 Remote endpoint 验收。

## 行为与范围

- `agent_query` 增加 `Providers {}`，serde `deny_unknown_fields` 与生成 schema 同时拒绝额外字段。
- `QueryData::Providers(ProviderCatalogSnapshot)` 直接复用 CB3-001 Product DTO 和 `provider_catalog()`。仅增加 DTO、ProviderId、ProviderHealth 的 JsonSchema derive，以及 cfg(test) manager 注入构造器。
- router 直接读取 Product，不经过 Workspace resolution、Binding、execution dispatch 或 health refresh。Product 读取失败使用既有 query failure envelope 的安全 `AGENT_OPERATION_FAILED`。
- 保留既有全局 agentEnabled gate：false 时 Remote discovery 隐藏 orchestration，调用保持 AGENT_DISABLED。本卡未扩大总开关契约。
- 没有 Provider mutation tool、Start DTO/routing、UI、CodeBuddy discovery/runtime 改动。

## JSON 与 descriptor

真实 loopback Streamable HTTP `rmcp::call_tool` 输入见 [mcp-input.json](mcp-input.json)，输出见 [mcp-output.json](mcp-output.json)。输出 data 与原 CB3-001 `src-tauri/src/agent/product/fixtures/provider_catalog_codex.json` exact equality，原 fixture 未修改。

```json
{"action":"providers"}
```

```json
{"ok":true,"data":{"providers":[{"id":"codex","displayName":"Codex","version":"codex-cli 0.153.4","enabled":true,"health":"available","availableForNewExecution":true,"capabilities":{"canExecute":true,"canContinue":true,"canCancel":true,"canRecover":true,"activity":true,"tokenUsage":false}}],"roleRouting":{"analysis":"codex","development":"codex","general":"codex","review":"codex","testing":"codex"}}}
```

完整实际 descriptor：[agent-query-descriptor.json](agent-query-descriptor.json)。输入/输出 schema 分别见 [agent-query-input-schema.json](agent-query-input-schema.json)、[agent-query-output-schema.json](agent-query-output-schema.json)。由运行测试打印的实际 descriptor 提取，无手写第二套 schema。

providers input branch：`type=object`，`properties={action:{const:providers,type:string}}`，`required=[action]`，`additionalProperties=false`。只读 annotations：readOnlyHint=true、destructiveHint=false、idempotentHint=true、openWorldHint=false。

沿用 `registry::tool_contract_hash`（name / description / inputSchema / outputSchema / annotations，递归 key 排序后 SHA256）：

| Tool | 旧 hash | 本卡 hash |
|---|---|---|
| agent_query | e9d3bdabccec0ec8771cd551d2dc7d37d4ea61ef60a25721df42e8a761434027 | 96bf06b880b5ec4972a42dd83cb1dc7e8f1f8c797f760dbad24c333466d68ae0 |
| agent_execute | d889e222c362c1e898a5aabe1897d8e4abb3c8e95962b5d0daf1c352be5f02da | d889e222c362c1e898a5aabe1897d8e4abb3c8e95962b5d0daf1c352be5f02da |

## 测试覆盖

新增测试均位于 `mcp::orchestration_tests::provider_query_tests`：

1. `providers_strict_dto_schema_and_descriptor_contract`：合法 action-only input；10 个额外字段 × 5 种 JSON 值（包括 null）全部 DTO INVALID_PARAMS 和 schema rejection；缺少/null/错误大小写 action 的 schema rejection；实际 annotations。
2. `providers_remote_registry_has_no_mutation_and_start_contract_is_unchanged`：enabled/disabled registry 无 Provider mutation tool；set_enabled / set_role_route / refresh_health / refreshHealth 不可调用；taskRole/providerId 仍被 Start schema 拒绝。
3. `http_providers_product_json_strict_input_and_read_failures_are_side_effect_free`：真实 HTTP tool list + call_tool、精确 Product JSON、直接 Broker router equality；8 种额外字段 INVALID_PARAMS error envelope；受控 Registry read failure；所有 response 经实际输出 schema 校验。种入既有 Execution/Claim 后比较三张表完整行、内存 config、磁盘 config bytes、原 Unavailable health、空 Workspace registry 和无 active binding。Provider 所有 lifecycle 入口与 Windows Runtime connect hook 均 panic，查询不得触达 ACP/Session/Runtime。
4. `http_providers_reuses_cb3_001_codex_product_fixture_without_runtime`：真实 Codex Product fixture 通过 HTTP 返回；Runtime connect panic guard；Execution/Claim/Runtime 表仍空；无 binding。

既有回归包含 `http_agent_query_compact_views_preserve_revision_persisted_result_and_execute_receipts`（get/list/observe、revision、result、无持久化修改）、`typed_registry_validation_rejects_cross_action_fields_and_preserves_context_authority`、`four_tools_enforce_disabled_and_uninitialized_policy_and_legacy_is_unknown`，以及 descriptor/schema/hash gates。每条测试完整名称见对应日志。

## 最终验证

| 命令（cwd 如上） | 状态 / 数量 | 日志 |
|---|---|---|
| `cargo test --manifest-path src-tauri/Cargo.toml mcp::orchestration_tests --lib -- --nocapture` | PASS，14 passed / 0 failed / 0 ignored，exit 0 | logs/orchestration-final2.log |
| `cargo test --manifest-path src-tauri/Cargo.toml mcp::registry::tests --lib` | PASS，20 passed / 0 failed / 0 ignored，exit 0 | logs/registry-final2.log |
| `cargo test --manifest-path src-tauri/Cargo.toml mcp::orchestration::dto --lib` | PASS，1 passed / 0 failed / 0 ignored，exit 0 | logs/dto-final.log |
| `cargo test --manifest-path src-tauri/Cargo.toml agent::product::provider_catalog_tests --lib` | PASS，4 passed / 0 failed / 0 ignored，exit 0 | logs/catalog-final.log |
| `cargo check --manifest-path src-tauri/Cargo.toml` | PASS，exit 0 | logs/cargo-check.log |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | PASS，exit 0 | logs/fmt-final.log |
| `git diff --check` | PASS，exit 0（仅 Git CRLF 提示） | logs/diff-check.log |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | FAIL，exit 101；仅已知任务外 await_holding_lock | logs/clippy.log |

合计 39 项相关测试通过。Windows linker 创建 import library 提示不是 test failure。

Clippy 唯一 blocker 为 `src-tauri/src/agent/store/usage_tests.rs:987` 持有 MutexGuard 跨越 `:1012` / `:1016` await。遵守任务明确禁令，未修改、忽略或弱化该检查。没有本卡新增 Clippy error。

## 初始失败与修复记录

- `cargo test --manifest-path src-tauri/Cargo.toml mcp::registry::tests::orchestration_fingerprints --lib -- --nocapture`：0 passed / 1 failed，exit 101。预期的 agent_query descriptor hash 变化，见 logs/initial-hash.log；更新既有 fixture，最终 registry gate PASS，agent_execute hash 保持原值。
- 初次 fmt check 提示新增 Rust 代码需格式化（logs/initial-fmt.log）；仅对本卡 dto.rs/provider_query_tests.rs 执行 `rustfmt --edition 2024 --config skip_children=true`，最终 fmt PASS。
- `cargo test --manifest-path src-tauri/Cargo.toml providers_ --lib -- --nocapture`：6 passed / 1 failed，exit 101（logs/providers-tests.log）。新增 Ajv input validator 未注册既有 uint32 format；补齐同项目既有 validator 的 format 定义，未弱化 schema。该轮还报告 stop Result 未消费 warning，已改为 unwrap。
- 首轮完整 orchestration：13 passed / 1 failed，exit 101（logs/orchestration-final.log）。既有两处 list fixture 省略 provider.version；task-start baseline `baseline/product.rs:137` 已用 descriptor.version，`:1136` 已从 Registry 读取 descriptor，因此不是本卡引入行为变化。两处期望改为已与 Product 精确对比的 detail.data.provider，完整 list equality 保留。最终 14/14 PASS。
- 首轮 registry：19 passed / 1 failed，exit 101（logs/registry-final.log）。既有 action branch 数量为 3；本卡新增 providers 后显式更新为 4，最终 20/20 PASS。

## 变更归属与保留基线

8 个交付代码文件及 5 个实际 JSON contract/evidence artifacts 的最终 SHA256 见 code-hashes.json（manifest 本身不自哈希）：

- src-tauri/src/mcp/orchestration.rs
- src-tauri/src/mcp/orchestration/dto.rs
- src-tauri/src/mcp/orchestration_tests.rs
- src-tauri/src/mcp/provider_query_tests.rs（新增）
- src-tauri/src/mcp/registry.rs
- src-tauri/src/agent/product.rs
- src-tauri/src/agent/provider/mod.rs
- src-tauri/src/agent/provider/registry.rs

原 84 个脏文件中，仅上述 Product/provider 3 个必要配套文件 hash 改变，其余 81 个保持 task-start hash。这 3 个文件的本次修改前完整字节保存在 baseline/，复制品 SHA256 与 baseline-hashes.json 完全一致；本卡仅增加 derive/import/cfg(test) 构造器。所有其他旧修改完整保留。

本 implementer 交付 artifacts：本 evidence.md、code-hashes.json、agent-query-descriptor.json、agent-query-input-schema.json、agent-query-output-schema.json、mcp-input.json、mcp-output.json、baseline/{product.rs,provider-mod.rs,provider-registry.rs}、logs/ 下上述验证记录。主协调创建/维护的 prd/design/implement/review-context/task/baseline-hashes 等 artifacts 不由本 implementer 覆盖。
