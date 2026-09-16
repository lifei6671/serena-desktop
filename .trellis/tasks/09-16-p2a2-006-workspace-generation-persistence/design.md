# Design — P2A2-006 Execution Workspace Generation Migration

## Boundary

This task extends only the internal frozen Workspace identity carried by Work and Execution. The existing entry points retain their present Authority: `WorkProductService::update(Begin)` still receives a legacy `WorkspaceSnapshot`, and no MCP Workspace routing changes.

## Persistent model

Schema v7 adds non-null `workspace_generation INTEGER NOT NULL DEFAULT 1 CHECK(workspace_generation >= 1)` to `executions` and `work_runs`. Historical rows define path-authority baseline generation `1`. The migration makes no inference from the current registry and does not update hashes or evidence.

## Identity and idempotency

`WorkspaceSnapshot`, execution input/record, and work-run record carry a nonzero `u64` generation. Work/Execution consistency compares all three identity members.

New requests hash a frozen `execution-request-v2` tuple containing generation. Stored historical v1 hashes are retried only if the row's migrated generation equals the input generation. Pre-C2 continuation fallback remains only for a parentless row and never receives the new field.

## Exclusions

No public DTO/source schema, WorkspaceResolver, routing Authority, Registry generation rules, claim key, runtime/evidence/recovery behavior, or dependency changes.
