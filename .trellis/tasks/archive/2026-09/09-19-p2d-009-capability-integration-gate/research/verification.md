# P2D-009 验证记录

## Preflight

- `remote::manager::boundary_tests::metadata_and_401_without_working_handler_cannot_be_ready`：稳定在 `boundary_tests.rs:614` 的五秒等待超时；只涉及 Remote manager probe 状态机。
- `agent::product::tests::orchestration_tests::public_vertical_work_source_start_continue_acceptance_e2e`：稳定在 `orchestration_tests.rs:540`，旧 public call 缺 `workspaceId`，得到 `WORKSPACE_CONTEXT_REQUIRED`；不改变 Agent lifecycle/Activity/Usage。

## P2D-009 Remote Cutover

- Registry schema 为 required `workspaceId`、`query` 和 optional `maxFiles`；root/canonicalRoot/path 不被接受为 authority。
- CodeGraph success wire shape 固定为 `{workspace:{id,generation},text,truncated:false}`；output schema 不含 root，成功与错误结构化内容均不暴露 root、PID、port、RuntimeHandle 或 raw stderr。
- 已解析 Lease 后的 Busy、NotPrepared/PreparationRequired、StartFailed、RuntimeLost 和 compatibility contract error 在 Adapter 边界投影为安全 `error` object；Manager 不含 providerId 特判，workspace 允许为 null 以处理 Remove race。
- Transport test 覆盖 tools/list、missing、unknown 和 valid workspaceId；fresh root 的 valid request 返回结构化 `CODEGRAPH_NOT_INITIALIZED`，不执行 init/sync/index。
- CodeGraph status runner 显式 pipe stdio，避免 `which_command` 产生未配置 stdout/stderr 的 child panic。

## Focused verification

- `cargo test --locked --lib mcp::registry::tests -- --nocapture`：16 passed。
- `cargo test --locked --lib mcp::server::quick_tunnel_transport_tests::stateless_candidate_returns_json_without_sse_for_broker_requests -- --exact --nocapture`：1 passed。
- `cargo test --locked --lib p2d_009_ -- --nocapture`：3 passed（success provenance envelope、A/B Lease routing、safe structured capability errors）。
- `cargo test --locked --lib mcp::integration_tests -- --nocapture`：24 passed, 3 ignored (external LAN/Serena fixtures).
- `cargo test --locked --lib codegraph_capability::tests -- --nocapture`：11 passed。
- `cargo test --locked --lib workspace_capability::tests -- --nocapture`：57 passed。
- `npm test`：121 passed；`npm run lint`、`npm run build`：passed。
- `cargo check --locked`、`cargo fmt --all -- --check`、`git diff --check`：passed。

## Full lib regression

`cargo test --locked --lib --quiet` runs 1040 tests: 1010 passed, 3 failed, 27 ignored. It reproduced three Remote/Agent failures: the two preflight baselines plus `remote::manager::boundary_tests::quick_manual_probe_failure_hides_url_and_retry_restores_ready` (`boundary_tests.rs:1065`: fixture drop still allows probe). All three are outside P2D-009's MCP/Capability/Health integration scope; no P2D test failed.
