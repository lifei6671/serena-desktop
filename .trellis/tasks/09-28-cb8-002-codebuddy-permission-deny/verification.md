# CB8-002 final verification

Baseline: feat/codebuddy 6749972982388cbd47633fe170e0d3797bfc05f3, clean. All validation cwd E:/wx_lifeilin/github.com/lifei6671/serena-desktop; native Windows only. Exact commands/exits/counts and historical failures: validation-results.jsonl. No commit/push, real CodeBuddy probe or CB8-003 work.

| Final / applicable check | Result |
|---|---|
| CodeBuddy full serial | PASS 144/144, repair1-codebuddy.log, exit 0 |
| Activity full filter | PASS 59, 1 ignored existing real Codex smoke, repair1-activity-node.log, exit 0 |
| Product full module | PASS 129, 5 ignored existing real smokes, repair1-product.log, exit 0 |
| Frontend AgentPanel | PASS 85/85, repair1-agent-panel.log, exit 0 |
| TypeScript / scoped ESLint | PASS, round1-tsc.log / round1-eslint.log, exit 0 |
| fmt / check | PASS, round1-fmt.log / round1-check.log, exit 0 |
| clippy --lib --tests -- -D warnings | FAIL exit 101, only unchanged usage_tests.rs:987 await_holding_lock, round1-clippy.log; user-authorized frozen baseline |
| Store transactions | PASS 54, verified-agent-store-transactions-tests.log; final Activity59 additionally covers changed Activity transaction/lifecycle helper |
| Codex cancellation | PASS 15, 1 ignored existing real smoke |
| TaskManager / Provider control | PASS 31 / 1 |
| Product provider catalog / MCP provider query catalog | PASS 5 / 4 |
| telemetry / same_runtime / usage | PASS 11 / 2 / 57; unchanged generic telemetry restored |
| git diff --check / frozen references | PASS; all three user-specified SHA256 unchanged |
| Linux | UNAVAILABLE: no project Docker runner; Linux validation stopped, no WSL |

Ignored smokes are not real runtime proof. Native permission tests use repository fake ACP child with real Windows Job/pipe/SQLite. Existing generic regressions remain applicable: no changes to cancel control, catalog/capabilities, Usage, same-runtime or generic telemetry/projector after their passing checks. Final Activity/CodeBuddy/Product suites cover the modified Store semantic helper, Product boundary and native lifecycle.

## Independent review repair round 1

Round0 FULL_SCOPE found P1: ordinary provider.processing Activity did not express deny (review-round0.md, freeze-round0.json). Repair adds closed provider.permission_denied summary to existing Activity current/history/revision in exact owner Store transaction, coherent Product validation and fixed Chinese label. No schema, phase, capability, generic telemetry identity or permission UI. Shared monotonic notification sequence prevents queued pre-deny Activity overwriting the hint; subsequent real Activity resets normally, body collector remains complete, finalizing/reconciling priority remains. Tests assert live denied Product/shared MCP JSON DTO, separate Store history/Claim facts, revision/order, ordinary reset and frontend Running label. This is not a claim of a live MCP Host/UI session.

All historical failures retained: initial Debug snapshot regression; generic telemetry private-identity regression (fixed by restoring baseline); initial zero-body Product filter (not accepted, corrected); round1 missing tests import; incorrect assertion expecting private ownsClaim in public DTO (changed to verify absence, Claim checked through Store); Activity command missing Node PATH (47 pass/12 environment failures; corrected PATH final59 PASS). Tests were not weakened to hide valid behavior failures.

Scope cleanup: telemetry.rs/projector.rs restored to original normalized HEAD content and Windows checkout CRLF, removing their status-only changes; no index writes. Temporary helper scripts were not added. Source scope includes only permission implementation, necessary Activity consumers and tests. Existing design §18 remains frozen; no unrelated frontend spec scaffolding changed. Task design and implementation-verification.md provide exact requirements-to-tests mapping.

Final independent FULL_SCOPE rereview must validate the new freeze target and all delivery paths before task completed. Reports/freeze/task lifecycle fields are administrative evidence, separate from executable target.
