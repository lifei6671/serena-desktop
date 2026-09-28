# Independent FULL_SCOPE review round 0

Reviewer: /root/full_scope_review, CHILD_AGENT, independent read-only; no writes/tests/builds. Target AA55DEF320B40C3BF213F022572734E52E0A9E19DF6DE2C01B8C6F8E9EE538A1. Baseline/HEAD 6749972982388cbd47633fe170e0d3797bfc05f3. Coverage COMPLETE; freshness FRESH (16 delivery +31 context hashes and 3 frozen references matched at end). Gate BLOCKED; CHANGES_REQUESTED. P0=0, P1=1, P2=0.

## P1 Contract: permission denied must have real safe Activity semantics

Primary location codebuddy/permission.rs:47. Valid permission -> typed deny physical flush -> Provider remains running emits only AgentActivityEvent::provider, hence activity.rs:124 derives provider.processing and Activity history records ordinary processing. CODEBUDDY_PERMISSION_DENIED is only an execution diagnostic. agentPresentation.ts:66-72 hides diagnostic in normal running state; neither current Activity nor history expresses permission denied. Violates user frozen requirement and design §18.1 item 4. Native tests execute/tests.rs:193,262 only asserted Provider phase, not denied summary.

Required minimal repair: closed safe permission-denied Activity semantic and necessary consumers; exact owner identity, bounded publication, no raw data/terminal/release authority preserved. No generic telemetry private identity, migration or capability. Add Product/MCP/history semantic assertions and preserve ordinary processing.

## Complete coverage

All 11 code/fixture paths from freeze-round0.json, untracked permission.rs, all five task contracts and frozen context/logs. Adjacent SDK definitions/Responder, Runtime shutdown/drop, Prompt terminal staging, Recovery/finalization, generic telemetry/projector, Activity derivation, Product/MCP DTO and actual frontend consumer inspected. No other confirmed issue. Typed options/id, identity, physical flush/read window/deadlines, cleanup, independent cancel, real terminal mapping, Job/Claim evidence and side-effect preservation verified by source plus recorded tests. Generic telemetry/projector restored baseline confirmed.

Verification checked: CodeBuddy143, telemetry11, Store54, Product observe21, same_runtime2, Usage57 plus other specified regressions PASS. fmt/check PASS; clippy only allowed unchanged usage_tests.rs987 baseline. Linux UNAVAILABLE, no WSL/real probe. Old zero-body run not accepted. Task must remain in_progress until repair, verification, new freeze and independent rereview.
