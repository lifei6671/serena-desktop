# Design boundary and unresolved dependency inspection

## Confirmed from current repository

- CB6-002 `CreatedChild` contains owned stdin/stdout/stderr Files, process handle and Job handle. `LaunchError.created` retains the owner after CreateProcessW succeeds. `LaunchedChild` adds creation FILETIME; this card must not reinterpret it as durable termination evidence.
- CB5-002 harness uses `ByteStreams::new(outgoing, incoming)` and `Client.builder().connect_with(...)`, sending `InitializeRequest::new(ProtocolVersion::V1)` through `cx.send_request(...).block_task().await`. External streams are a frozen proven capability, not a reason for a custom NDJSON client.
- Probe adapter's unbounded diagnostic capture and direct-child cleanup are probe-only; they are not a production transport/Job cleanup implementation to copy.
- Current production Cargo.toml has no ACP SDK dependency. Frozen probe Cargo.toml pins 2.2.0 without default features; its evidence records schema 1.9.1.

## Intended design, pending source inspection

Keep launcher as process authority. Runtime must retain Job/process ownership separately from SDK byte streams throughout initialize, cancellation of the initialize future, failure and shutdown. Scope excludes durable evidence and startup recovery.

Use SDK protocol client and inspect its actual 2.2.0 hooks for response correlation, request/notification dispatch, lifecycle and transport backpressure. Add only the necessary bounded transport/route policy around supported SDK APIs. Do not duplicate the entire client or infer API behavior from the initialize-only probe. Raw capability retention must remain private and bounded; compatibility is strictly negotiated protocol v1.

Before writing production code, resolve SDK hooks for bounded admission, raw envelope validation, exact id diagnostics, early-session replay and shutdown completion. If actual APIs cannot safely express the frozen contract, stop that affected work and document a Material Contract Difference. Dependency download failure alone does not prove an API difference.

## Historical environment stop — RESOLVED_BY_HOST

At the initial attempt, the official SDK was absent from the local Cargo registry source/cache and offline index query. Cargo and official HTTPS download both fail at Windows TLS credential acquisition. At that checkpoint, no SDK runtime API assessment beyond the existing harness was claimed and no Cargo manifest/lockfile edits were made. The resumed source assessment below supersedes this historical stop.

## SDK source assessment and selected implementation

Directly inspected official SDK 2.2.0 src/jsonrpc.rs, jsonrpc/incoming_actor.rs, transport_actor.rs, protocol_compat.rs and component.rs. ByteStreams uses futures AsyncRead/AsyncWrite; Client.builder().with_handler(...).connect_with(...) accepts them. HandleDispatchFrom<Agent> receives untyped Dispatch::{Request,Notification,Response}; Responder sends results/errors, ResponseRouter forwards to SDK pending waiter. SDK assigns UUID request ids and removes exact id from PendingReplies. Unknown ids are ignored by SDK, so byte boundary adds stable unmatched (unknown/duplicate) diagnostics. EOF closes pending replies.

SDK channels are internally unbounded. Bound externally by admitting one complete validated input frame at a time, releasing input only after handler completion (server request: after output flush); bound request admission before cx.send_request. No raw SDK ConnectionTo escapes this module. Explicit frame limit precedes SDK BufReader.lines; bounded early/routed session queues share count/byte budget and expiry. No general replacement JSON-RPC client: SDK owns serialization, pending reply correlation and responder/response routing.

Error output is stable enum codes; suppress SDK tracing for the driver future to prevent raw message logging. Successful raw initialize version is checked before typed schema conversion; all public capabilities remain untouched. Runtime holds Job/process separately from File streams; failure/Drop terminates entire Job and closes owner, without claiming durable recovery evidence. Protocol health returns only an optional incompatibility classification; no Registry mutation is required by this card.

No Material Contract Difference identified in the inspected APIs. Begin implementation under original authorization.

