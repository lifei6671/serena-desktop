# Delivery Review Context

- Scope: CB2-003 only; user explicitly authorized planning and implementation. No commit.
- Starting HEAD: a3ed7962d0cfdcf213de7bcfd02cf48bdf195aef; no staged changes.
- Starting dirty files and exact bytes: baseline-files.json; baseline.patch excludes pre-existing untracked control.rs, whose bytes are included in JSON.
- Baseline excluded from delivery ownership: CB2-002 and autostart/store validation fixes. Inspect only interactions and preserve them.
- Verified source SHA256: task breakdown 7e74f8ac766daf1c0c0e5c3cdaa68fee1db666ff8085084b18c5a24ea91e02a1; technical design f9f416b22a74b86f842349824009ef4a32da9e21054fb023387d0cde5d77a32b.
- Review strategy: FULL_SCOPE CHILD_AGENT, Tier 3 for concurrency/persistence/local IPC contracts. Review all delivery-owned hunks and new files, not unrelated baseline changes.
- Focus: validation, null clearing, unknown valid provider preservation, serialization shape, lock ordering, atomic save before memory publish, admission authority updates, restart, health probe isolation, Remote non-exposure and no runtime/session/claim creation.
- Applicable rules: AGENTS.md; task prd/design/implement and original user requirements; code-delivery-review Rust profile, change surfaces (API/config/tests), generic correctness/concurrency/error/compatibility/scope lenses.
- Verification: implementer records exact Windows native commands/results in verification.md. Linux NOT_RUN: no project Docker runner found; WSL prohibited. Existing usage_tests.rs await_holding_lock must be recorded, not repaired or suppressed.
- Final read-only reviewer must return hashes/target identity, full coverage, findings and PASSED/BLOCKED/UNAVAILABLE. Any repairs invalidate affected review until reverified/refrozen.
- Trellis spec-sync assessment: this task implements the already frozen §26 contract. No new project-wide convention is planned; record any actual new knowledge in this task before deciding whether shared specs need changes.
