# CB4-002 Frontend evidence

Scope: local Role Routing Editor only. Frontend-owned implementation files: `src/AgentPanel.tsx`, `src/api.ts`, `src/styles.css`, `src/AgentPanel.test.mjs`. Byte baselines are preserved in `baseline/src/`; `baseline/frontend-hashes.txt` records their original hashes. No backend, Runtime, Remote MCP, or Execution mutation was added.

## Role matrix

| Wire role | UI label | Initial persisted fixture | Mutation authority | Pending / rollback |
|---|---|---|---|---|
| development | 开发 | Catalog roleRouting.development | agent_provider_set_role_route | Per-role |
| testing | 测试 | Catalog roleRouting.testing | Same local IPC | Per-role |
| review | 评审 | Catalog roleRouting.review | Same local IPC | Per-role |
| analysis | 分析 | Catalog roleRouting.analysis | Same local IPC | Per-role |
| general | 通用 | Catalog roleRouting.general | Same local IPC | Per-role |

All five persisted bindings are asserted on mount. General clear is asserted again after unmount/remount. Registered options are dynamic descriptors, including disabled entries. Unknown current IDs remain visible with 未注册. Every Provider option uses a `provider:` prefix; the clear sentinel cannot collide with legal Provider IDs such as `none` or `__none__`.

## DOM examples and assertions

- Set: development selects Codex; mock receives `{ taskRole: 'development', providerId: 'codex' }`. Returned settings contain `returned-provider`, which becomes the visible committed value; the draft is not treated as authority. Existing Execution fixtures and action calls remain unchanged except reads.
- Clear / restart: general selects 未指定 Agent, sends null, commits null and displays null after a persisted fixture remount.
- Disabled / unknown: `none` is a registered disabled Provider with dynamic label Future Agent; its option remains selectable. Unknown `__none__` and `historical` IDs are shown without writes. The user can explicitly clear or replace them.
- Failure: development selects Other while testing independently selects Other. Development failure restores its previous Codex binding and shows a role-specific toast. Testing stays pending, then commits successfully; Composer remains usable.
- Concurrent responses: testing success arrives before development clear success. Each response updates only its own role, so a stale sibling field in the full settings response cannot overwrite the other role. A later failed development set restores the committed null rather than the old Catalog value.
- Stale polls: a poll starts before saving and returns during pending; another starts during saving and returns after success. Neither replaces the draft / committed value. A fresh poll started after completion updates current policy, proving that committed overrides are not permanent.

## Ordering design

Each role has a local generation counter. Mutation start and settlement both advance it. A poll captures generations at request start and can remove a role's committed override only when its generation still matches and that role is not pending. The Catalog IPC reads current policy; therefore a new read begun after mutation completion can resume authority. The frontend stores no second persisted settings source.

## Verification notes

Final normal run (after removing temporary logging and using real disposable timer handles): `node --test src/AgentPanel.test.mjs src/App.test.mjs` — PASS, exit 0, 101 tests / 101 passed / 0 failed, duration 24615.785ms. `git diff --check` — PASS, exit 0. Final hashes and machine-readable result are in `frontend-final.json`. Full npm/build/lint and Rust/registry gates are coordinated by the main session.

The six new role DOM tests passed in every focused run observed. Final full focused counts and exact gate outcomes are recorded separately by the main session. Early focused runs were interrupted after prolonged silence following the existing sidebar date/focus tests; those interrupted runs are not PASS evidence. Isolated `node --test --test-name-pattern='task focus shows' src/AgentPanel.test.mjs` passed 1/1; the isolated date/focus combination passed 2/2. Temporary stage logging is removed before final delivery. These are Windows Node/JSDOM checks, not browser screenshot or Linux runtime evidence.

The full diagnostic run eventually failed with Node heap exhaustion. Root cause: the new stale-poll test initially intercepted every global `setInterval`, including JSDOM's internal requestAnimationFrame interval (`node_modules/jsdom/lib/jsdom/browser/Window.js:616`). After restoring the mock, JSDOM retained the fake animation-frame timer handle, so later sidebar deletion focus callbacks did not run. The existing DOM-object equality assertion then attempted to format large React/DOM graphs on failure. The final test mock intercepts only the 1500ms application polls and delegates all other intervals to the original function. The heading-focus assertion still checks exact element identity, now expressed as a boolean comparison to provide bounded failure output. No production focus behavior or assertion requirement was changed.
