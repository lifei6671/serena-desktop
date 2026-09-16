# P2A2-003 Workspace Discovery API

## Goal

Verify and complete the frozen MCP workspace_list/workspace_get discovery contract without creating Workspace authority, bindings, or runtime work.

## Requirements

- `workspace_list` returns the Registry catalog and `registryRevision` without
  validating Roots, starting a process, activating a Workspace, or creating a
  binding.
- `workspace_get(workspaceId)` reads one registered catalog entry with the
  same no-mutation behavior; unknown IDs return `WORKSPACE_NOT_FOUND`.
- Keep the established Registry entry representation (`id`, `name`, `root`,
  `generation`) as the catalog's Workspace identifier and Root display data.
  This task does not invent a new `rootStatus` wire enum: the frozen design
  names the missing-root state but provides no complete public enum/schema.
- Input validation remains at the schema boundary: missing `workspaceId` is
  `WORKSPACE_CONTEXT_REQUIRED`; wrong-type, empty, and blank values are
  `INVALID_PARAMS`.
- Do not alter Activate/current/deactivate behavior, create a Workspace
  binding, query/prepare a Provider, warm a Runtime, or make later tools
  implicitly depend on Discovery.

## Acceptance Criteria

- [x] MCP schemas expose read-only `workspace_list` and `workspace_get`; the
  latter requires top-level `workspaceId`.
- [x] List/get return Registry catalog data and catalog revision; a missing
  Root remains reportable rather than being resolved or deleted.
- [x] Discovery does not mutate Desktop selection, legacy active state,
  Registry entries, or later request authority.
- [x] Focused MCP contract tests, locked library checks, scoped formatting,
  diff checks, and final review pass.

## Verification Evidence

- `cargo test --locked --lib workspace_discovery` — PASS, 2 passed: MCP
  schema/annotation contract and Registry-only list/get behavior with no
  selection, active binding, Root validation, or implicit later authority.
- `cargo test --locked --lib
  workspace_get_validates_required_context_before_registry_lookup` — PASS,
  1 passed: missing vs malformed `workspaceId` boundary errors.
- `cargo check --locked --lib` — PASS. Existing `replace_workspaces` dead-code
  warning remains outside this task's ownership.
- `rustfmt --edition 2024 --check --config skip_children=true src/mcp/mod.rs
  src/mcp/registry.rs` — PASS.
- `git diff --check` and `git diff --cached --check` — PASS.
