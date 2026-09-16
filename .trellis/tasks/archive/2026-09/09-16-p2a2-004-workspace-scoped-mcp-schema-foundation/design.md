# Design — P2A2-004 Workspace-scoped MCP Schema Foundation

## Boundary

This task owns only the reusable MCP schema and parameter-validation
foundation. It does not expose a new Tool-family request schema. The shared
layer distinguishes absent context from malformed input before a valid value
reaches `WorkspaceResolver`, and exposes the Lease provenance Schema that
later atomic cutovers can reuse.

## Frozen request chain

```text
public request parameters
    -> schema / parameter validation
    -> validated workspaceId
    -> WorkspaceResolver
    -> WorkspaceLease
```

The public Tool-family adoption order is fixed: P2A2-007 handles Work / agent
start plus Execution snapshot; P2A2-009 handles Git plus Lease-rooted route;
P2A2-010 disables CodeGraph until a Workspace-scoped Adapter exists;
P2A3-010 handles Serena semantic routing; P2A3-011 handles the four Source
Tools via the Serena compatibility facade. `agent_query`, cancel, and continue
remain keyed by their established execution/work identities and never accept a
second Workspace authority.

## Error and compatibility boundary

- missing or null `workspaceId`: `WORKSPACE_CONTEXT_REQUIRED`;
- wrong type, empty, or blank `workspaceId`: `INVALID_PARAMS`;
- syntactically valid unknown ID: `WORKSPACE_NOT_FOUND` from Resolver.

The Foundation does not add a caller-provided Root, create an implicit
session/selection binding, or route a handler through a legacy ActiveWorkspace
fallback. It cannot be used to publish a Tool schema whose handler does not
use the resulting Lease for Root authority.

## Exclusions

No Execution Workspace Generation migration (P2A2-006), Tool-family handler
backend cutover, filesystem operation, Provider/Runtime lifecycle work, public
path field rename, or CodeGraph/Serena/Source Runtime work belongs here.
