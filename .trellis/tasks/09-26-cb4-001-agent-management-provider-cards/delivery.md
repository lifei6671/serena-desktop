# CB4-001 交付证据

仅实施 CB4-001。Agent 管理页面在 Composer 前按 Product Catalog 动态渲染 Provider cards；保留现有任务操作。新增 local-only `agent_provider_catalog_get`，直接复用 Product 的只读 `provider_catalog()`。未进入 CB4-002，未提交 Git，任务保留 `in_progress` 等待 Host Gate。

卡片独立显示 enabled、health、runtime presentation、activeExecutions。活动数按已加载 rows 的冻结 `provider.id` 和既有活动状态集合统计；不代表全局任务数或 OS 进程状态。停用且仍有活动任务显示“正在停用”；Idle 使用中性 stopped。协议统一“—”，未知 Provider 和缺失名称/版本安全降级。未新增编辑角色控件。

## 验证

全部命令 cwd：`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`。Rust 为原生 Windows `x86_64-pc-windows-msvc`，不是 Linux 验证。没有运行 WSL。

| 命令 | 结果 |
| --- | --- |
| `node --test src/AgentPanel.test.mjs src/App.test.mjs` | PASS，95 passed / 0 failed |
| `npm test` | PASS，150 passed / 0 failed |
| `npm run build` | PASS，含 `tsc` typecheck 和 Vite build |
| `npm run lint` | PASS，ESLint |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib agent::product::provider_catalog_tests -- --nocapture` | PASS，4 passed / 0 failed |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib mcp::registry::tests:: -- --nocapture` | PASS，20 passed / 0 failed |
| `cargo check --manifest-path src-tauri/Cargo.toml --locked` | PASS |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | PASS |
| `git diff --check` | PASS |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings` | FAIL，exit 101，仅既有 `usage_tests.rs:987 await_holding_lock`；未修改该文件 |

其余成功命令 exit 0。初轮 Rust getter 错误转换编译失败已最小修复；初轮前端 3 个旧文案/静态断言失败已修复，日志保留。原生 Tauri/真实浏览器视觉验收与 Linux 验证：NOT_RUN；本次 DOM 证据来自 JSDOM。

## 证据索引

- [精确命令、结果与日志](evidence/validation.json)
- [DOM 状态矩阵](evidence/dom-state-matrix.md)
- [Catalog 示例 fixture](evidence/provider-catalog-example.json)，非实时安装状态
- [11 个代码/测试文件的前后 SHA256](evidence/changed-files.json)
- [仅本任务增量](evidence/implementation.diff)，以任务启动时快照为基准，排除前序修改
- [基线保留和 Remote 契约](evidence/baseline-preservation.json)：924 个基线文件无丢失、无任务外变化；Remote registry/orchestration/DTO 字节未变
- [独立检查](evidence/review.md)

两份权威文档 SHA256 与 Host 给定值一致。`agent_query` frozen hash 为 `96bf06b880b5ec4972a42dd83cb1dc7e8f1f8c797f760dbad24c333466d68ae0`；`agent_execute` 为 `d9f562430843fcb9ae663cff5b895afef93ab45c23bd29b4af16cf85fd199fa1`，registry regression 均通过。

本次规则已由 task design 描述，没有形成需补充到通用 spec 的新架构约定。未更改 Runtime、Routing、role policy、config 或 Remote MCP 生产契约。

独立交付检查：APPROVED；CHILD_AGENT / FULL_SCOPE，PASSED，11/11 COMPLETE，FRESH，无 P0/P1，review repair rounds 0。目标清单 SHA256：`e0129b939944087e77f572d027745d46b8ce59c965bb0c6137db75fc38c9f4b0`。此检查不替代 Host Gate。

临时清理限制：自动审批以 `blocked by policy` 拒绝递归删除 `C:\Users\lifei\AppData\Local\Temp\cb4-001-baseline`。目录仍保留，未声称清理完成；完整增量和哈希已持久化在本任务 evidence。
