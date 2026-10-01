# CB6-003 — Managed ACP Transport / Initialize

## Authority and scope

User explicitly authorized this independent delivery on 2026-09-27, starting at clean `feat/codebuddy`, `a9eee0b`. Authoritative task: `docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md`, CB6-003; the current user contract is stricter where specified. Do not reuse another Agent Thread or run any real CB5/CodeBuddy probe.

Deliver managed ACP transport and protocol-v1 initialize using official `agent-client-protocol = 2.2.0` (frozen probe schema 1.9.1). SDK must receive external owned pipes, never spawn authority. Keep CB6-002 Job-at-creation ownership intact, including post-CreateProcessW failures.

Allowed implementation: CodeBuddy client/protocol, necessary runtime/module wiring, minimal official SDK dependencies, focused fixtures/tests, narrow Registry health seam if needed. No durable private state, recovery, real session/new, prompt, workspace writes, acceptance/dispatching, cancellation/continuation/usage, UI/remote, auto-install/npx, Codex semantic changes, commit or push.

## Acceptance

- Exact JSON-RPC response id correlation; multiple pending requests, out-of-order responses, unknown/duplicate ids and interleaved notifications handled deterministically.
- Separate responses, server requests and notifications. Permission requests fail closed; unsupported request with id receives -32601; notification without id receives no response; do not advertise elicitation.
- Exact session routing and bounded early-session queue; original order replay after route registration, isolation by session id, cleanup on shutdown. Only synthetic session/new in tests.
- Bound pending count, early count/bytes/TTL, single frame size, request timeout and private stderr tail. Explicit overflow/failure; no stranded waiters.
- Invalid NDJSON, malformed JSON-RPC, EOF, timeout and shutdown converge deterministically.
- Raw and typed negotiated protocolVersion must equal 1. Only deterministic mismatch yields CODEBUDDY_ACP_INCOMPATIBLE and may set Registry Unavailable. Missing capability/method leaves capability false and health unchanged. EOF/timeouts/I/O/create/start failures remain operation-local. Product version/hash and error text are not compatibility authority.
- All six public skeleton capabilities stay false even after initialize success. No stderr/raw provider diagnostic leakage into public errors.
- Initialize failure terminates/converges the owned Job, including any post-create ownership in LaunchError; never just main PID kill.
- Focused tests and launcher ownership regression; fmt, clippy, necessary check, diff/scope checks; independent read-only final review with frozen target, coverage and freshness. Repair P0/P1/P2 before a successful delivery.

## Current state

The initial environment blocker is RESOLVED_BY_HOST (historical failures retained in research/dependency-environment.md). Implementation is now in progress using inspected official SDK 2.2.0 sources. No Material Contract Difference identified. Delivery remains incomplete until product verification and independent final review pass.

