# P2A1-003 Workspace Registry Service and Read API

## Goal

Implement the ManagerConfig-backed Workspace Registry service, read-only list/get API, and reusable serialized persist mutation foundation required by P2A1-003.

## Requirements

- `ManagerConfig.workspaces` is the sole Workspace Registry authority. The
  Registry service must never read Serena registry, `.serena`, Git, or
  CodeGraph state.
- Add a dedicated reusable Registry service backed by the `ManagerConfig`
  held by `SupervisorState`. Its read model exposes ordered `Workspace`
  entries and the current registry revision.
- `list` returns the complete ordered registry snapshot without transforming
  `id`, `name`, `root`, or `generation`. `get(id)` returns the exact entry or
  stable `WORKSPACE_NOT_FOUND` for an unknown non-empty id. Neither operation
  activates, binds, or otherwise changes workspace state.
- Provide the P2A1-006/007/008 foundation for a serialized clone-validate-
  persist-publish Registry mutation. It must use the existing
  `SupervisorState.operation` mutex; publish only after `config::save`
  succeeds; increment `workspace_registry_revision` exactly once for a
  successful structural change; and skip persistence and revision changes for
  a no-op.
- Reads must safely return either a complete prior or complete committed
  snapshot. Do not hold the runtime/config mutex while the config file is
  written when the existing architecture permits clone-persist-publish.
- Do not implement product CRUD, inspector/picker, MCP schema/routing,
  Provider/Source/Git/CodeGraph/Agent behavior, Serena-sync changes, or
  StateStore/schema changes. Do not recanonicalize persisted roots or mutate
  Workspace generations.

## Acceptance Criteria

- [x] Focused service tests cover ordered list/get, generation preservation,
  unknown-ID error, no binding/Serena dependency, atomic successful persist
  and reopen, no-op bytes/revision preservation, persist failure rollback, and
  concurrent complete old/new read snapshots.
- [x] Existing P2A1-001 migration and P2A1-002 root helper tests remain
  passing, as do `cargo check --lib`, target-file rustfmt checks, and
  `git diff --check`.
- [x] Final source diff is limited to P2A1-003 implementation/tests plus this
  Trellis task metadata; no commit or push is made.

## Notes

- Contract authority is
  `docs/implementation-task-breakdown-agent-platform-v0.2-revision003.md`
  P2A1-003 and `docs/technical-design-agent-platform-v0.2.md` sections 7.1–7.3,
  8 Workspace Discovery, 10.2, and 52 Phase 2A.1.
