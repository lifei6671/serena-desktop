# CB5-003 independent delivery review

## Identity and verdict

- Reviewer: independent `CHILD_AGENT`, `/root/review_probe`; did not implement or modify the reviewed target.
- Mode: Strict independent review; Tier 3 protocol/process boundary.
- Target: `337157d9e3c9105bde3fae8396f29da74b61032dded731e64a310b087ccfd7eb`.
- Product HEAD: `5179248cfa30096df912c6567aee2e80e3bf4352`.
- Coverage: COMPLETE for all 25 files in `review-target.json`; individual SHA256 values independently matched, 0 mismatches, including after verification.
- Harness implementation / evidence-integrity review gate: **PASSED**. No findings.
- Real contract: **BLOCKED** at `session/new`; formal contract delivery verdict: **UNABLE_TO_VERIFY**. Required real prompt/session/terminal facts remain NOT_PROVEN. This review does not grant canExecute or Host Gate acceptance.

## Findings (fixed)

None. Review was read-only; only this review record was written.

## Findings (not fixed)

No evidence-backed P0/P1/P2 defects found in the frozen target. Authentication is an environmental contract blocker, not a harness defect. No authenticate call, real rerun, permission policy, or CB5-004 work was requested or performed by the reviewer.

## Coverage and requirement trace

| Partition | Review evidence |
| --- | --- |
| Authority and scope | Read task check/implement manifests, PRD, design, implementation plan, task metadata, current user requirements, and the applicable CB5-003 / design sections 15, 19, 21, 28.1. Both source-document SHA256 values match the Host-provided hashes. |
| Rust package / lock | Reviewed isolated workspace declaration, exact SDK 2.2.0 dependency, disabled default features, schema 1.9.1 lock entry, and parsed the complete 152-package lock. No product Cargo changes or path dependencies. |
| Process and SDK | `main.rs::probe` owns canonical Node child and pipes; SDK receives external Tee ByteStreams. Fresh temp cwd is used for child and NewSessionRequest. Official cached schema confirms NewSessionRequest/Response and PromptRequest/Response fields and metadata support. Read/write prompt text targets only relative paths in the temp cwd. |
| Lifecycle and errors | Protocol timeout / EOF / invalid identity / SDK error / permission notification leave through the common cleanup path. `transport.rs` bounds final wait and stderr join/abort; kill errors do not skip wait. No Windows Job-at-creation or tree-containment assertion. Python outer watchdog uses bounded taskkill and wait. |
| Wire and identity | Tee records actual successful read/write bytes. Per-direction NDJSON framing preserves observed complete-frame sequence. RPC matching uses the actual request id. Empty identity fails closed; schema rejects missing/non-string identity. Raw and SDK typed DTOs remain distinct. Secret-key/private-metadata redaction and non-JSON byte-only reporting were inspected. Actual 8 frames require no redaction and parsed rawLine equals the recorded message. |
| Workspace proof | Full recursive manifest includes hidden files and directories; rejects symlinks. Delta compares path sets and bytes. SHA256 is calculated by Python from captured actual bytes before deleting the workspace, not expected marker content. Real read seed hash/length match; real write has no output.txt and is not accepted. |
| Activity / terminal | Captures raw updates and prompt terminal by RPC identity without projecting Serena ActivityEvent. Fake success test checks update sequence before terminal, actual stopReason and unknown extension preservation; malformed terminal type remains raw evidence and fails SDK processing. |
| conversationRequestId | Official PromptRequest metadata serializer is used. UUID is harness-generated; A omits metadata. Unit test compares A/B prompt payload and the exact extension key. Real B UUID was generated but never sent because session/new failed; all behavioral conclusions remain NOT_PROVEN. |
| Evidence/report integration | Read all evidence JSON/JSONL/logs, both complete scenario results, sdk-details, verification, baseline and scope verification. 8 real wire frames are exactly two initialize exchanges and two session/new auth-error exchanges. Process/report summaries agree; no prompt ids, session ids, updates or terminals were fabricated. |
| Boundaries and docs | Existing task and .zed remain local/untracked; no production changes. No new frozen product contract exists to sync into .trellis/spec. Cache/build output and unrelated tasks are excluded as declared. |

## Verification

Reviewer cwd: `E:\wx_lifeilin\github.com\lifei6671\serena-desktop`.
`TASK=.trellis/tasks/09-26-cb5-003-fresh-session-prompt-activity-contract`.
Cargo commands used `CARGO_HOME=TASK/.cargo-cache` and the existing offline source replacement: `--config 'source.crates-io.replace-with="task-official"' --config 'source.task-official.registry="sparse+http://127.0.0.1:18742/"'`.

| Check | Independent result |
| --- | --- |
| `cargo ... check --offline --locked --manifest-path TASK/harness/Cargo.toml` | PASS, exit 0 |
| `cargo ... test --offline --locked --manifest-path TASK/harness/Cargo.toml` | PASS, exit 0; 6 unit + 7 integration = 13 passed, 0 failed/ignored/filtered |
| `cargo fmt --manifest-path TASK/harness/Cargo.toml -- --check` | PASS, exit 0 |
| `cargo clippy ... --offline --locked --manifest-path TASK/harness/Cargo.toml --all-targets` | PASS, exit 0, no warnings. First invocation placed --config before clippy and failed dependency lookup; placing the same configuration after clippy resolved the invocation issue. |
| Python `compile()` of run_probe.py | PASS, exit 0; no generated pycache |
| Frozen target SHA256 | PASS, 25/25 match |
| Product baseline SHA256 | PASS, 319/319 match, 0 delta |
| `git diff --check` / `git rev-parse HEAD` | PASS; unchanged declared HEAD |
| Real CLI | Existing first-hand evidence reviewed; not rerun |
| Linux / Job-at-creation / process-tree containment | NOT_RUN / NOT_PROVEN; outside this card |

The 13 tests check observable fail-closed outcomes, actual subprocess EOF/timeout, ordered wire, terminal extensions, unexpected hidden writes, metadata serialization, and cleanup error continuation. Their pass establishes harness behavior only.

## Required real evidence limitation

Both initialize responses negotiate JSON number 1. Both session/new responses return JSON number -32000, message `Authentication required`, data.category `auth`. The child exit code 1 does not alter the initialize compatibility result. There is no successful session/new DTO, exact sessionId, real prompt, prompt terminal, real prompt update order, successful isolated write, or real A/B metadata decision. Read workspace delta 0 before any prompt is not a successful read-only prompt proof. The current session/new error is not a safe prompt-error terminal.

Accordingly, canExecute contract evidence is BLOCKED, all missing real facts remain NOT_PROVEN, and delivery is UNABLE_TO_VERIFY despite a PASSED harness review. Stop and wait for Host Gate; do not enter CB5-004.
