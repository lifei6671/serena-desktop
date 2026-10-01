# CB2-004 implementation verification

Environment: Windows / PowerShell. All commands ran in `E:\wx_lifeilin\github.com\lifei6671\serena-desktop`.

## Delivery scope

- `src/types.ts`: required `ManagerConfig.agentProviders`, open `Record<string, AgentProviderPolicy>`, five required `string | null` role routes.
- `src/api.ts`: document full-config pass-through and Rust/Local IPC policy mutation authority; no new mutation wrapper.
- `src/app/useAppController.ts`: initial placeholder matches Rust defaults, hydration replaces it without overlay; refresh/action/autostart snapshots synchronize policy while preserving other draft fields. Partial-save types exclude policy, outgoing payload retains state policy, responses supply authoritative draft policy.
- `src/App.test.mjs`, `src/AgentPanel.test.mjs`: synchronize all 12 existing ManagerConfig fixture literals with shared provider settings.
- `src/configFixtures.mjs`: independent fixture factory, matching Rust defaults.
- `src/configModel.test.mjs`: eight behavioral tests using actual hook and API with JSDOM/IPC mocks.
- Task artifacts: this report and `npm-test.log`. Parent owns baseline comparison, final hash freeze, and independent review records.

## Commands and results

| Command | Result | Evidence |
| --- | --- | --- |
| `node --test src/configModel.test.mjs` (initial run) | FAIL, exit 1 | 7 passed / 1 failed. API test expected undefined args for get_app_state; Tauri actually passes `{}`. Corrected only the test expectation. |
| `node --test src/configModel.test.mjs` (after correction) | PASS, exit 0 | 8 passed / 0 failed / 0 skipped; 861 ms. |
| `npm test` | PASS, exit 0 | 141 passed / 0 failed / 0 skipped; 24.67 s; complete output in npm-test.log. |
| `npm run build` | PASS, exit 0 | `tsc && vite build`; 2244 modules transformed, Vite completed in 9.84 s. Nonblocking plugin timing notice only. |
| `npx eslint src/types.ts src/api.ts src/app/useAppController.ts` | PASS, exit 0 | No diagnostics. Existing ESLint config targets TypeScript/TSX. |
| `git diff --check -- src/types.ts src/api.ts src/app/useAppController.ts src/App.test.mjs src/AgentPanel.test.mjs` | PASS, exit 0 | No whitespace errors; Git CRLF conversion notices only. |
| Rust / Clippy / Linux / native desktop validation | NOT_RUN | Frontend-only task. Existing `usage_tests.rs` await_holding_lock blocker remains outside scope. |

Actual full-suite shell capture command:

```powershell
npm test *> .trellis/tasks/09-26-cb2-004-frontend-config/npm-test.log
$result = $LASTEXITCODE
Get-Content .trellis/tasks/09-26-cb2-004-frontend-config/npm-test.log -Tail 22
exit $result
```

## Key assertions

1. Initial policy and shared fixture match Rust defaults; hydration preserves an unknown provider, unknown route, null routes, and absence of CodeBuddy without overlaying defaults.
2. Real `api.getState` / `api.saveConfig` traverse mocked JSON IPC unchanged, using camelCase policy wire fields.
3. Polling adopts empty provider maps/null routes without losing unsaved port changes.
4. Both saveFields and saveToggle send state policy, not a stale draft, and adopt the newer policy returned after a simulated concurrent backend policy change.
5. Both partial-save paths retain unrelated edits made before and while saving.
6. Both failure paths refresh authoritative policy, retain ordinary draft fields, and do not apply the failed toggle.
7. General action and autostart snapshots propagate policy into the draft.

## Limits and review handoff

No UI, Rust, Remote MCP, CB2-005, dependency, or commit changes. No production implementation changes after green verification. Frontend tests mock backend authority; current Rust `replace_config` was inspected read-only and is responsible for rejecting policy mutation through ordinary config saves. This report is implementation evidence, not Host Gate approval. Independent review and final baseline/hash verification remain with the parent session.
