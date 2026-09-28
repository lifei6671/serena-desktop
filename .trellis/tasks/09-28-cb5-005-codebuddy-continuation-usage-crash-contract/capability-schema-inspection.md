# Capability / Schema Inspection

## Provider and LaunchSpec provenance

- Installed package: `@tencent-ai/codebuddy-code 2.158.0`。
- LaunchSpec: absolute `C:\nvm4w\nodejs\node.exe` + installed `bin\codebuddy` + `--acp`。
- Node SHA256: `3602f2bb1a10f2cbab4c36886218a33c1ab3db87290e73b033c46c77147d0237`。
- CodeBuddy script SHA256: `426186a82e36466d5be485f335ced1b0d664539c53b72a5f10ba3a267e60dab6`。
- Package JSON SHA256: `8fb1a1e781ba5e16741b943745f0a6e69177f20f4479212c038c27345eeea85b`。

完整 path/hash/count 在 task-local inspection JSON。Environment provenance 只保存 allowlist facts，不保存 PATH、环境值、token 或 prompt answer 正文。

## Recovery API

Pinned ACP v1 schema 提供 `agentCapabilities.loadSession` 与 typed `session/load({sessionId,cwd,mcpServers})`。Attempt 4/5 Host initialize 均实际 advertise `loadSession=true`，并成功调用：

```text
session/load({ sessionId: S1, cwd: exactWorkspace, mcpServers: [] })
```

因此唯一 recovery method 固定为 `session/load`。Harness 没有 `session/resume` request、fallback method 或第二方法试错。Attempt 5 的非空 replay 全部属于 exact S1，P3 semantic lineage 同时证明历史 marker 与 exact normalized cwd。

## Usage wire boundary

Attempt 4 真实 wire 有 9 条 `usage_update`，字段为 `used`、`size` 与 `_meta`。但是事件没有 exact Prompt identity binding，scope/reset/terminal/late 均为 unknown。Provider event existence 与 SerenaDesktop public Usage contract 是不同结论：Provider 有事件，但 initial release 的公共 Usage 明确 unsupported，`tokenUsage=false`、值为 unknown/null。

## Current StateStore schema inspection

- StateStore 当前 schema version 13。
- `schema_v13.sql` 的 `codebuddy_execution_state` 已包含 `session_id`、`conversation_request_id`、`provider_request_id`/source、`prompt_rpc_id`、typed terminal fields，以及限定为 `session/load` 的 recovery method/state/runtime/timestamps。
- 通用 `executions` 已包含 `workspace_id`、`canonical_workspace_root`、`workspace_generation` 与 `parent_execution_id`。
- `codebuddy/store.rs` 已提供 private state create/read/mutate 与 exact session/request identity checks；这些 private fields 不投影为公共 DTO，也不构成 Claim authority。

结论为 `EXISTING_FIELDS_SUFFICIENT`；字段映射和不新增项见 `dcr.md`。
