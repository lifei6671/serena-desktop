# Agent Management Surface Cleanup — verification evidence

- CWD: `E:/wx_lifeilin/github.com/lifei6671/serena-desktop`
- Existing task activated with `python .trellis/scripts/task.py start .trellis/tasks/09-26-agent-management-surface-cleanup`; no new task, Git staging/commit, or CB5-001 work.
- Starting HEAD: `a3ed7962d0cfdcf213de7bcfd02cf48bdf195aef`.
- Existing dirty frontend/Rust work preserved. Baseline source copies, 981-file SHA-256 manifest, status and HEAD are in `.trellis/.runtime/surface-cleanup-evidence/`.
- Task activation metadata is separate from the implementation diff: status `planning` → `in_progress`, branch `null` → `feat/codebuddy`.

## Implementation and data paths

Only `src/AgentPanel.tsx`, `src/AgentPanel.test.mjs`, and `src/App.test.mjs` changed relative to source baseline. `implementation-delta.patch` contains those baseline-relative hunks; `source-hashes.json` freezes their SHA-256 hashes.

Main Composer, recent-task section, filters, refresh, task cards, pagination and empty state are removed. Provider cards, Role Routing, current workspace and workspace-management entry remain. Pending Claim view still calls `openDetails`; cancel still calls `operate(cancel)`.

AgentPanel retains history polling every 1500 ms for the current first-page snapshot, Provider active counts, pending Claim blockers and selected-detail synchronization. As before, the UI describes counts as applying to loaded tasks, not all executions. Main-only pagination/filter state is removed. Selected details outside the snapshot still use `observe`; final-result fetching remains. History failures remain visible in Provider management and retry automatically. ProjectTaskNavigation source is unchanged, including its independent 3000 ms polling and pagination. Domain/API/request capabilities and backend are unchanged.

## Verification

| Command | Status | Result |
|---|---|---|
| `node --test src/AgentPanel.test.mjs src/App.test.mjs` | PASS | 104 tests; 104 passed; 0 failed/skipped/cancelled; exit 0 |
| `npm test` | PASS | 159 tests; 159 passed; 0 failed/skipped/cancelled; exit 0 |
| `npm run build` | PASS | tsc and Vite build; exit 0 |
| `npm run lint` | PASS | ESLint; exit 0 |
| `git diff --check` | PASS | exit 0; existing LF/CRLF warnings only |
| Baseline SHA-256 preservation | PASS | Only the three scoped source files changed; Rust/Remote 0 changes |

Logs are `.trellis/.runtime/surface-cleanup-evidence/{focused,full-test,build,lint}.log`. These are Windows frontend checks, not Linux/backend or desktop visual validation.

The first migration test run had 101/102 passing: a Role layout assertion still referenced the removed Composer. It was corrected to the preserved workspace bar; two shared-polling/pending-lock tests were then added, producing the final 104/104 result. Retained coverage includes sidebar navigation/pagination/detail return, Cancel/Resume/Manual Resolve, continuation and result refresh, local hide/delete, Provider enable/disable, Role routing, pending Claim and unsupported-version copy. Removed tests cover the deliberately removed main Composer/history controls.

## Review and Host handoff

Independent final review is recorded in `review.md`. Host Gate remains pending and must independently review/rerun; local checks do not claim Host acceptance. Task remains active and unarchived.

Spec assessment: no new reusable architectural convention; the existing task PRD/design already records this surface boundary. Frontend spec templates were not expanded for this bounded removal.
