# Root Thread title capability

Only active Codex Provider executions enable `codex_app.set_thread_title`. Serena
does not read AGENTS.md or choose titles. Recovery/contract-only clients advertise
no title tool. There is no generic Desktop tool registry or new database state.

## Fixed 0.153.4 evidence

Repository evidence root: `docs/tasks/evidence/TASK-005/codex-0.153.4/`.
`identity.json` and `src-tauri/src/agent/codex/protocol.rs` pin:

- Source commit: `3d2ee51ca2d5db578f328aa75e20aa22c0197c9a`.
- Binary SHA256: `444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B`.
- Combined schema SHA256: `B06F77062369D481A59CC70720C12B89CB9DD49C385863923262102D3AD6C978`.

The exported experimental schema supports these exact shapes:

| Evidence file under `schema/` | Contract |
| --- | --- |
| `v2/ThreadStartParams.json` | `dynamicTools` array of tagged specs; namespace has `type`, `name`, `description`, `tools`; nested function has `type`, `name`, `description`, `inputSchema` |
| `ServerRequest.json` | `item/tool/call` request with JSON-RPC request `id` |
| `DynamicToolCallParams.json` | Required `threadId`, `turnId`, `callId`, `tool`, `arguments`; optional nullable `namespace` |
| `DynamicToolCallResponse.json` | Required `success`, `contentItems`; text item is `{type:"inputText", text:string}` |
| `ClientRequest.json`, `v2/ThreadSetNameParams.json` | `thread/name/set`, params `{threadId, name}` |
| `v2/ThreadSetNameResponse.json` | Object response |
| `v2/ThreadNameUpdatedNotification.json` | `threadId`, nullable/optional `threadName` |

Local fixed-source `history-recovery-verification/source/thread_processor.rs`
lines 639–659 and 1775–1808 show metadata update, response, then name notification.
[Fixed normalize_thread_name](https://raw.githubusercontent.com/openai/codex/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/core/src/util.rs)
lines 95–102 trims and rejects empty names; upstream has no maximum here.
The tool adds an explicitly advertised product bound of 200 Unicode scalar values
after trimming and rejects control characters. This is not an upstream limit.

[Fixed session initialization](https://raw.githubusercontent.com/openai/codex/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/core/src/session/mod.rs)
lines 686–690 restores dynamic tools from conversation history when none are
supplied. Threads created with the capability inherit it on resume. Old Threads
created without it are not retrofitted by this change.

## Ownership and failure behavior

The Provider enables the handler for one immutable Execution id on its Client.
For each call the handler reads that Execution's existing persisted binding and
compares the wire `threadId` and `turnId` with the exact Root Thread and Turn,
and the bound Runtime with this transport's Runtime. Missing/unbound identity,
child/unowned/stale Turn, terminal or cancelling Execution, unsupported tool,
and invalid arguments return bounded `success:false` text. Calls cannot select
the rename destination through arguments. A call racing ahead of durable Root
Turn binding fails explicitly; it never supplies lifecycle authority itself.

The existing bounded server-request worker services the call, outside the
Provider lifecycle event queue. It does not persist arguments or errors. Existing
`thread/name/updated` and final recovery metadata maintain product `threadName`.
`Notification::Other` remains dropped before queue retention.

Rename RPC rejection/timeout does not fail the Client. Timeout means unknown
outcome, without replay. At most one unresolved rename is retained until its exact
late response or Runtime termination; further title requests fail locally while
it is unresolved. Malformed JSON-RPC/known notification/rename result and actual
transport failures retain the existing fail-closed transport behavior.

## Verification commands

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked agent::codex::app_server::
cargo test --manifest-path src-tauri/Cargo.toml --locked agent::codex::protocol::
cargo test --manifest-path src-tauri/Cargo.toml --locked agent::codex::provider::
cargo test --manifest-path src-tauri/Cargo.toml --locked agent::product::
cargo check --manifest-path src-tauri/Cargo.toml --locked
git diff --check
cargo test --manifest-path src-tauri/Cargo.toml --locked real_fixed_root_title_smoke -- --ignored --nocapture --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --locked real_fixed_coding_skill_command_failure -- --ignored --nocapture --test-threads=1
```

Real tests use temporary workspaces and the verified fixed binary. The title smoke
explicitly requests a fixed title, so it tests the actual model-to-tool-to-RPC-to-
product path without assuming what title arbitrary global instructions will choose.
Neither a fixture nor an ignored test declaration constitutes real PASS evidence.
