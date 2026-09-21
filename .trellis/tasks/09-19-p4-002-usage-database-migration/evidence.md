# P4-002 Evidence

## Migration identity

- Authority: `docs/technical-design-agent-platform-v0.2.md` §37–§38,
  SHA-256 `42c0f86bb294dc0425c5a8deee5d9f3f36586704218a832e8a9fe89e5f1d9f19`.
- Task card: `docs/implementation-task-breakdown-agent-platform-v0.2-revision003.md`
  P4-002, SHA-256
  `d86aedbe60ac8701fbcbff06369566d33f282454c6eef5f4aae198db1908132a`.
- Pre-change baseline rechecked: `schema_v8.sql`
  `58cf8dd501f318a511517daae348497459d330549655caf773c2e6530c149633`,
  `store.rs` `0a42a4b370fecffad20286cc5742d24701c64dd321cfd6b7dd71c7f28325382d`,
  `usage.rs` `9ffafcd7463323e14dddb089d8dbaa673568f38b185133f1041737655020ef4f`.

## Delivered behavior

- `schema_v9.sql` creates only `execution_usage`,
  `codex_thread_usage_epochs`, and `codex_execution_usage_state`.
- `StateStore::migrate` accepts 0 through 9 and applies v9 inside its existing
  immediate transaction. v8 rows receive no Usage or Codex-private backfill.
- The public read helper returns `UsageSnapshot` only from a stored public row;
  missing rows stay absent. Codex records remain inside `store::usage`.
- The migration failure test injects invalid v9 SQL and proves version 8 plus
  no v9 table state before retrying successfully to 9.

## Verification

| Command | Result |
| --- | --- |
| `cargo test --locked agent::store::tests --lib` | PASS: 30 passed, 0 failed, 1048 filtered out. |
| `cargo check --locked` | PASS; 6 existing unrelated dead-code warnings. |
| `cargo fmt --all -- --check` | PASS. |
| `git diff --check` | PASS; only pre-existing CRLF advisory warnings. |

## Scope confirmation

No DCR conflict was found. No P4-003 parser, P4-004 delta/baseline/update,
terminal lifecycle mutation, Product, MCP, UI, commit, or push was performed.
