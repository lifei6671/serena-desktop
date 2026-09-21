# Design — P2A2-006 Execution Workspace Generation Persistence / Migration Compatibility

## Boundary

This task is persistence and compatibility only. It validates the internal frozen generation field carried by persisted records and does not construct a `WorkspaceSnapshot`, select a workspace, or route a request. P2A2-007 solely owns Start Authority cutover; P2A2-008 solely owns Continue inheritance.

## Persistent model

Schema v7 adds non-null `workspace_generation INTEGER NOT NULL DEFAULT 1 CHECK(workspace_generation >= 1)` to `executions` and `work_runs`. Historical rows define path-authority baseline generation `1`. The migration makes no inference from the current registry and does not update hashes or evidence.

## Identity and idempotency

Execution input/record and work-run record carry a nonzero `u64` generation. Persisted-record consistency compares the frozen identity members without obtaining a snapshot from an ambient authority.

The frozen `execution-request-v2` tuple contains generation. Stored historical hash compatibility remains guarded by the persisted row's generation and existing identity conditions; this task validates that storage-level compatibility only and does not own a continuation route.

## Exclusions

No Begin, Start, or Continue Workspace snapshot construction; no Global ActiveWorkspace, DesktopSelectedWorkspace, session, or last-request fallback; no public DTO/source schema, WorkspaceResolver, routing Authority, Registry generation rules, claim key, runtime/evidence/recovery behavior, or dependency changes.
