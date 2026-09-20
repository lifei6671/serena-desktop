# P6-RG-13A source read workspace ID

## Goal

Repair the public vertical Work E2E fixture so its two `source_read_file` requests explicitly carry the configured workspace identity.

## Requirements

- Modify only `src-tauri/src/agent/product/orchestration_tests.rs`, specifically `public_vertical_work_source_start_continue_acceptance_e2e`.
- Add `"workspaceId":"W"` to the first `read_args` request while preserving `relative_path`, `start_line`, `end_line`, and `max_bytes`.
- Add `"workspaceId":"W"` to the second full-source `source_read_file` request.
- Do not modify production code, `active_with_read_file`, Agent/Work state machines, context structures, other test assertions, or Remote tests.
- Do not commit or push.

## Acceptance Criteria

- [ ] The named E2E test passes three consecutive runs.
- [ ] `agent::product::tests::orchestration_tests` passes.
- [ ] `cargo fmt --all -- --check`, `cargo check --locked`, `cargo clippy --locked --all-targets -- -D warnings`, and `git diff --check` pass.
- [ ] A read-only P0/P1/P2 review covers the final delivery-owned diff.
