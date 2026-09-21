# P6-RG-13B Remote Ready contract closure

## Goal

Update Remote Ready documentation and boundary tests so transport/auth/MCP readiness is independent from Serena capability health; no production remote changes.

## Requirements

- Update `docs/agent-quick-tunnel.md` so Remote Ready means the public transport, OAuth boundary, and MCP surface passed the probe. State that successful `tools/list` does not prove Serena, CodeGraph, or any other Capability is healthy.
- Update `docs/remote-access-ui.md` with the same Ready contract. Broker management and local capabilities remain independently available when Serena is not ready; Serena/Capability health is displayed independently.
- Replace only the two stale tests in `src-tauri/src/remote/boundary_tests.rs`: one must prove the remote fixture is not required for the broker handshake to reach Ready; the other must prove detaching and reattaching the Serena fixture does not change a manual probe's transport Ready result.
- Do not modify production `quick_tunnel.rs`, OAuth, Broker, Remote state-machine, or Capability-health implementation.

## Acceptance Criteria

- [ ] Both documents describe Ready as transport/auth/MCP readiness rather than Serena/Capability health.
- [ ] The two target boundary tests express the decoupled contract and pass three consecutive runs each.
- [ ] `remote::manager::boundary_tests` passes serially; Rust format, check, clippy, complete Rust tests, and `git diff --check` are recorded.
- [ ] A read-only P0/P1/P2 review covers the final in-scope changes; no commit or push is made.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
