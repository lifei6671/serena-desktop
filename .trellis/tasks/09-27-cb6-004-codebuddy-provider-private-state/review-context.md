# CB6-004 closeout review context

## Authority / scope

Current user explicitly resumes inherited dirty CB6-004 against HEAD b40d67b392669a087dcd4d6d4e59065c233f292b. Preserve the reviewed design. Same task; no commit/push, no CB6-005. Initial dirty files are delivery-owned by user instruction; closeout-baseline.json records them. Original baseline.json remains untouched.

Rules: root AGENTS.md / user instructions; code-delivery-review SKILL.md and Rust, migration, verification, severity/review protocol references. Deep persistent-data review, STRICT independent read-only FULL_SCOPE. No implementation agent may act as final reviewer. Reviewer must inspect every executable file in executable-target.json against HEAD and actual content, including untracked SQL/tests/fixture. No sampled-diff-only approval.

## Change map / required invariants

1. schema_v13.sql + store.rs: isolated new private table, no backfill; one IMMEDIATE historical migration transaction, version gate, rollback/FK/index/CHECK/RESTRICT. v12 bytes/hash frozen.
2. codebuddy/store.rs: pinned SDK schema::v1 RequestId/StopReason, typed domain API, UUIDv7 atomic local reservation, initial Prepared never null conversation, string/i64 RPC only.
3. store/codebuddy.rs: SQL remains private to StateStore, IMMEDIATE writes, provider/runtime/generic binding+revision/private revision guards, no upsert, missing/corrupt fail-closed. MarkSent requires runtime/protocol/session. Sent->Uncertain; exact terminal from Sent/Uncertain; conflicts reject and terminal identity freezes. R2 distinct CodeBuddy runtime; inspection outcomes only partial/unknown/material_difference; no generic result/release/termination/Claim changes.
4. tests + frozen v12 fixture: cover all above and frozen historical Codex execution/runtime/usage/private values unchanged. Include existing migration version assertions updated to v13.
5. Provider/MCP tests: public Provider/Product/Work/MCP read projections exclude private session/conversation/provider request/RPC/private revision. Actual populated private rows must not change public response values.

All files are unstaged. No dependency/config changes intended. No usage ledger implementation, network session/new/prompt behavior, ProviderAcceptance, startup reconcile/termination evidence, Claim release, capability/public DTO changes.

## Verification authority

See verification.md and closeout-*.log for actual command results. Native Windows only, cwd src-tauri, C:\Users\lifei\.cargo\bin\cargo.exe. No Linux runner exists; Linux NOT_RUN. User explicitly permits precise recording of the sole pre-existing usage_tests.rs:987 await_holding_lock baseline, while requiring all CB6-004 lints fixed. This does not turn raw Clippy FAIL into PASS.

## Reviewer output

Return mode CHILD_AGENT, matching target hash, complete file/requirement coverage, concrete P0/P1/P2 findings with source locations and impact, and terminal PASSED/BLOCKED/UNAVAILABLE. Review is read-only: no edits, tests or generated files. If no findings, say so; do not invent cleanup or new requirements. Root fixes P0/P1/P2 and re-freezes/reviews as needed.
