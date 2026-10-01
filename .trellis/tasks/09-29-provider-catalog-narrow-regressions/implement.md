# Implementation plan

1. Capture the current HEAD and dirty-worktree inventory; read the frontend/UI and Rust-adjacent project rules.
2. Repair CodeBuddy mode ACK reconciliation and migrate the two obsolete ExecutionProfile tests.
3. Repair AgentPanel catalog retry/success caching and add the reject-then-resolve/default-preservation test.
4. Narrow Broker management lock scope around canonical workspace resolution and add a blocking-catalog concurrency test.
5. Fix the <=980px grid placement and add a lightweight CSS contract test.
6. Run the six required validation commands, classify every result, and inspect the final delivery-owned diff.
7. Freeze the final target and run independent Trellis check/review coverage. Repair only confirmed in-scope P0/P1 findings, then revalidate affected checks.

## Guardrails

- Do not change Codex provider/app-server logic or production execution/store/profile schema.
- Do not redesign role defaults, Provider Catalog, ExecutionProfile, or Provider behavior.
- Do not overwrite, reset, commit, push, or broadly format the dirty worktree.
