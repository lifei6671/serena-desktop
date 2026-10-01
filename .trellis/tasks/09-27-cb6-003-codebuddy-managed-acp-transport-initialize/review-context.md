# Final delivery review context (pending final verification/freeze)

Delivery: SAME CB6-003 task; original clean baseline a9eee0b495b74dc5247771e1fe671b0c53cb6c5f on feat/codebuddy. No commits, staging, push, real CodeBuddy/probes or subsequent-card implementation authorized.

Rules: root AGENTS.md (Chinese function/core comments; narrow scope); .trellis/workflow.md; user frozen CB6-003 acceptance in prd.md; code-delivery-review SKILL.md with review-protocol/change-scope/change-surfaces/verification/severity and Rust profile. Tier 3 (concurrency, JSON-RPC, process ownership). Strict independent read-only CHILD_AGENT full-scope review; reviewer must not have implemented target. User additionally requires P2 repairs.

11 executable target files: Cargo.toml/lock, codebuddy mod/client/protocol/runtime and client_tests/runtime_tests, three tests/fixtures/codebuddy_* files. Complete sorted paths and byte hashes in final-target.json (once frozen). Original tracked modifications are only manifest/lock/mod; remaining new files were created by this delivery. Task-local prose, result logs and hashes are context/evidence; historical blocked-handoff target is NOT production approval.

Requirement / implementation / evidence:
- Official SDK external managed streams and exact SDK request id authority: client.rs ByteStreams/ConnectionTo/Dispatcher; client_tests exact_id_out_of_order_unknown_duplicate_and_notification and native Runtime fake.
- Raw + typed version 1, missing capability and health separation: protocol.rs incoming/Failure.health_change; client.rs initialize; sanitized initialize fixture + initialize/mismatch/malformed/capability tests. Six public caps stay false. No Registry mutations or product dispatch wiring.
- Bounded NDJSON guard and SDK channel admission: client.rs GuardedRead/Write/request; frame/pending/backpressure/outgoing size tests. SDK RawJsonRpcMessage validation prevents pre-handler schema drops; unknown notifications never receive responses.
- Bounded early/routed session queue, TTL, exact route replay and isolation: protocol.rs Shared; queue/replay/TTL tests; synthetic early config_option_update fixture backed only by previously observed Host ordering.
- Server permission cancelled / unsupported request -32601 / no notification responses: Dispatcher and server_request_permission_and_notification_baseline.
- Stable close/timeout/invalid I/O and all pending completion: shared first-failure/watch, RequestLifetime/driver; EOF/timeout/invalid/shutdown/dropped-future and closed-pipe tests.
- Private bounded stderr: StderrTail, Runtime drain; bounded_stderr_tail + native fake emits 20 KB but retains 64 bytes. No raw provider payload in Failure, no SDK tracing subscriber output.
- First-runnable process ownership and cleanup: existing CB6-002 launcher unchanged; runtime.rs owns full CreatedChild/LaunchError.created Job; native fake startup membership plus exact ActiveProcesses==0 on success/mismatch/timeout/EOF/drop/post-create/transport failure.
- Shared dependency change: official ACP/derive2.2.0/schema1.9.1; futures/tracing; tokio-util compat feature. Existing package versions not changed. Focused existing Codex runtime/launcher tests cover shared runtime feature effects; Codex source untouched.

Exclusions: no durable StateStore/schema/recovery/evidence persistence, no real session/new/prompt/workspace writes, acceptance/dispatch, UI/remote, auto-install, cancel/continue/usage. Session-new strings occur in synthetic fixtures only. No Linux validation claim. Material Contract Difference: NONE_IDENTIFIED; historical environment blocker RESOLVED_BY_HOST.

Checkpoint f18f8623540e8d5f8b4855d48a16a1bfab041cb9baca9e8a78c1859da39cdda5 found one P2 (SDK pre-handler consumed frames could stall guard). Repaired using SDK raw-envelope validation and no-id unsupported notification admission skip; two regression tests pass in prior focused run. Checkpoint is superseded by final full review after current fixes.
