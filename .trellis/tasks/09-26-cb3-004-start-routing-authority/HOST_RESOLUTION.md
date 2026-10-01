# CB3-004 Host Resolution

Status: DESIGN_BLOCKER resolved by Host design review. The race identified in DESIGN_BLOCKER.md is valid, but the minimal fix does not require moving Provider Authority into serena.rs.

## Existing synchronization facts

Official production mutation paths already serialize through `Broker.management`:

- save_config / agentEnabled: `management -> Supervisor.replace_config() -> operation`
- provider enabled / role route: `management -> Supervisor.mutate_provider_settings() -> operation`
- provider health refresh: `management -> AgentTaskManager.refresh_provider_health()`

Start currently does not hold `management`, which creates the identified TOCTOU.

## Frozen solution

Keep routing/provider Authority in Agent/Product/TaskManager. Reuse existing locks; add no mutex and no second Authority.

Final Start sequence:

1. Parse / validate DTO and Work/context preflight without holding management.
2. Immediately before authoritative routing/create, acquire `Broker.management`.
3. While holding management:
   - re-read current ManagerConfig (`agentEnabled`, roleRouting)
   - re-read current Provider Registry/admission facts
   - resolve explicit or legacy routing with frozen error ordering
   - call existing `Supervisor.create_workspace_start()`; it acquires `Supervisor.operation` and performs Workspace Lease + Execution/Claim creation.
4. Once create returns, release `Broker.management`.
5. Only then perform existing handoff / Provider dispatch.

Existing lock order remains:
`Broker.management -> Supervisor.operation`.

This serializes Start authority/create against all official provider policy, health refresh, and agentEnabled mutation surfaces, while avoiding holding configuration management across Runtime/provider handoff.

## Required adapter shape

The Product work adapter may receive a crate-private creation authority/guard reference from Broker so it can acquire `management` only at the final create boundary after Work/context preflight. A minimal MCP/local adapter change is authorized.

Do not:
- modify Provider policy mutation behavior;
- move routing logic into serena.rs;
- add new locks;
- hold management through Provider runtime acceptance;
- create then compensate;
- fallback Provider/Role.

## Race tests

Tests must prove:
- policy/role mutation waits while final Start authority+create owns management;
- health refresh waits across the same boundary;
- agentEnabled save cannot slip between validation and create;
- management is released before Provider handoff/dispatch;
- Workspace operation/lease semantics remain unchanged.

If implementation cannot satisfy this with existing management + operation locks, return DESIGN_BLOCKER with concrete lock-order evidence.
