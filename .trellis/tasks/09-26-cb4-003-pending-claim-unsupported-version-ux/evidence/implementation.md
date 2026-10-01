# CB4-003 frontend implementation evidence

Status: Implementation complete; frozen for coordinator/Host independent review. No commit.

## Scope and provenance

- Baseline: `evidence/baseline.json`, original files in its external `snapshot` directory. Pre-existing dirty work preserved.
- Delivery-owned implementation: `src/AgentPanel.tsx`, `src/AgentPanel.test.mjs`, `src/agentPresentation.ts`, `src/api.ts`, `src/types.ts`.
- `frontend-delivery.diff` compares these files against the Host baseline, excluding prior work. `frontend-final-hashes.json` identifies the review target.
- No Rust, Remote schema, registry, runtime, dependency, or shared style edits. No Phase 5 implementation.
- Read task PRD/design/implement, UI DESIGN, frontend specs, frozen source §10.3/§14.2.1/§25.3/CB-004. CodeGraph structural discovery succeeded for AgentPanel/api/presentation; unreturned source ranges read directly.

## Pending action DOM/state matrix

| Case | Verified behavior |
| --- | --- |
| Disabled + matching attention=pending_explicit_resume, canResumePending=false | Claim warning and actions rendered |
| Disabled + attention=none, canResumePending=true | Same warning/actions; no independent Claim inference |
| Unrelated provider pending row | Excluded from card blocker list |
| View blocker | Existing openDetails -> observe exact executionId, details DOM |
| Cancel blocker | Existing operate(cancel), exact executionId; no resume |
| Re-enable | Local set_enabled(providerId,true), warning disappears, binding preserved; no execution operation |
| Running disable | Execution unchanged, active count 1, Runtime running, draining label; no cancel/kill |
| Disabled role binding | Remains same provider and displays 已停用 |
| Regression | Existing cards, Role editor, Composer/list/detail, Cancel/Resume, Manual Resolve tests green |

Cancel respects existing canCancel and operation/list-error guards; this UI does not authorize release. Warning operates on already loaded ExecutionView rows, as scoped by task.

## Provider mutation matrix

| Case | Verified behavior |
| --- | --- |
| Enable success | Commits returned target provider setting only |
| Enable failure | Optimistic checkbox rolls back; error toast; role unchanged |
| Disable failure | Optimistic checkbox rolls back; error toast; role unchanged |
| Two providers + Role concurrent, out-of-order responses | Each response changes only its own policy key |
| Poll started before enabled mutation, returns in-flight | Pending enabled state retained |
| Poll started during mutation, returns after commit | Newly committed state retained |
| Poll started after completion | New authoritative catalog may replace override |
| Role mutation during enabled mutation + stale polling | Independent role generation protects role commit |

Provider start/end generations mirror CB4-002 pattern independently of Role generations. Neither mutation triggers resume, cancel, fallback or rebind.

## Unsupported diagnostic fixtures

- Exact `CODEBUDDY_VERSION_UNSUPPORTED` + arbitrary `future` Provider ID displays the mandated full text, including version 9.8.7.
- Exact code with null/blank version renders `版本 （未提供）`; no fabricated version.
- Missing/null code, `AGENT_PROVIDER_UNAVAILABLE`, suffix and lowercase near-matches do not display version warning, even with CodeBuddy ID, unavailable health, unknown-dev version and misleading errorMessage.
- Unsupported card has only normal local-policy Switch; no version override, ignore check, run anyway, Force Unlock or supported-table controls.
- Provider-neutral source assertion permits only stable diagnostic literal mapping and forbids providerId-specific layout/state branches.

LIMITATION: Real CodeBuddy unsupported diagnostic production source is not implemented at this phase. Optional frontend `diagnosticCode` is a consumer contract + fixture only. Current backend omission remains undefined. Discovery/admission and release-owned supported-version table remain later phase work.

## Commands and results

cwd for all commands: `E:\wx_lifeilin\github.com\lifei6671\serena-desktop` (Windows host; no Linux/runtime/browser screenshot evidence claimed).

| Exact command | Result | Count / exit | Log |
| --- | --- | --- | --- |
| `node --test src/AgentPanel.test.mjs src/App.test.mjs` | PASS | 117/117; fail/skipped 0 | frontend-focused.log |
| `npm test` | PASS | 172/172; fail/skipped 0; exit 0 | frontend-test.log |
| `npm run build` | PASS | tsc + Vite; exit 0 | frontend-build.log |
| `npm run lint` | PASS | exit 0; no warnings/errors | frontend-lint.log |
| `git diff --check` | PASS | exit 0; existing LF/CRLF notices only | tool output |

Rust gates delegated to main coordinator; no Rust gate result is inferred from frontend tests. Final independent review remains coordinator-owned. Stop here pending Host review.
