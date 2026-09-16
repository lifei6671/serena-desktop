# P2A2-004 Workspace-scoped MCP Schema Foundation

## Goal

Build the shared MCP `workspaceId` parameter/DTO/Schema, `WorkspaceResolver`
connection, public contract helper, and Workspace provenance Schema foundation.
This task must not publish a new Tool-family `workspaceId` Schema until that
Tool can use the request Lease as its actual Root authority in the same atomic
change.

## Requirements

- Missing `workspaceId` returns `WORKSPACE_CONTEXT_REQUIRED`; wrong type,
  empty, and blank values return `INVALID_PARAMS`; a syntactically valid but
  unknown value returns `WORKSPACE_NOT_FOUND` through the existing Resolver
  boundary.
- Provide a shared provenance Schema for `{ id, generation }` associated with
  a resolved Workspace Lease.
- Do not add a new public `workspaceId` Schema to Source, Git, CodeGraph,
  Serena semantic, Work start, or agent start in this task. Their atomic
  Authority cutovers are P2A3-011, P2A2-009, P2A2-010, P2A3-010, and P2A2-007
  respectively.
- Do not add `workspaceId` to query, cancel, or continue. Do not alter handler
  backends, execution-generation persistence, path resolution, or
  Runtime/Provider behavior.
- Never use Global ActiveWorkspace, DesktopSelectedWorkspace, session,
  last-request, or validation-only fallback as request authority.

## Acceptance Criteria

- [ ] Shared `workspaceId` parameter parsing, Resolver connection, and
  provenance Schema are available to later atomic Tool-family migrations.
- [ ] Contract tests cover the missing/malformed/unknown split and provenance
  Schema shape.
- [ ] No Tool family that still relies on Global ActiveWorkspace receives a
  newly callable `workspaceId` Schema; no implicit binding or fallback is
  introduced.
- [ ] Schema diff/hash, focused contract tests, locked library check, scoped
  formatting, diff checks, and final review pass.

## Resolved boundary decision

The previous schema-only plan was rejected because it could validate
`workspaceId=A` while a legacy handler used an unrelated global Root. This
Foundation task is therefore deliberately shared-only. Each Tool family adopts
the shared contract only in its own Authority cutover task, or is explicitly
disabled there when no Lease-scoped route exists.

## Verification evidence

- `cargo test --locked --lib workspace_id_foundation`: PASS — 2 passed,
  including malformed/missing parsing and valid unknown ID routed through
  `WorkspaceResolver` to `WORKSPACE_NOT_FOUND`.
- `cargo test --locked --lib mcp::registry::tests`: PASS — 14 passed,
  including the no-new-unsafe-Schema and existing Git provenance contracts.
- `cargo test --locked --lib git_dispatch_uses_only_each_explicit_workspace_lease`:
  PASS — 1 passed; the existing Git path still uses the request Lease after
  taking the shared Foundation connection.
- `rustfmt --check --edition 2024 src/mcp/registry.rs src/mcp/mod.rs`: PASS.
- `cargo check --locked`: PASS; pre-existing `replace_workspaces` dead-code
  warning remains.
- `git diff --check`: PASS. Public descriptor coverage confirms that Source
  and CodeGraph receive no new `workspaceId` input Schema in this task.
