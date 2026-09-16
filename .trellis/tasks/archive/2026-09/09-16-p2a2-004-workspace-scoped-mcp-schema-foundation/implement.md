# Implementation Plan — P2A2-004 Workspace-scoped MCP Schema Foundation

1. Add one reusable `workspaceId` parameter/DTO parser that preserves the
   missing, malformed, and syntactically-valid-unknown error split.
2. Connect only a validated ID to `WorkspaceResolver` and expose the resulting
   Lease provenance Schema for use by later cutovers.
3. Do not alter a Tool-family descriptor or its externally observable
   Authority behavior in this task.
4. Add focused parameter, Resolver, provenance-Schema, and no-new-unsafe-
   descriptor contract tests.
5. Record schema diff/hash and run focused contracts, locked library check,
   scoped formatting, diff checks, then full-scope review of this task only.
