# CB4-001 DOM state matrix

Command (Windows PowerShell, repository root):

```text
node --test src/AgentPanel.test.mjs src/App.test.mjs
```

Final result: PASS, exit 0; 95 passed, 0 failed, 0 skipped. Log: `frontend-focused-final.log`.
Initial run: 92 passed, 3 failed (two stale page/composer copy assertions and a static assertion exposing the old Codex running description). Updated assertions to the authorized neutral copy and changed the description to Agent; retained `frontend-focused.log`.

These are rendered React DOM checks in JSDOM, not native Tauri/browser visual acceptance.

`provider-catalog-example.json` is an illustrative contract-shaped fixture, not a live installed-provider report. Protocol is absent from the actual Catalog contract; optional version is omitted for the unknown Provider. The backend existing Codex fixture is `src-tauri/src/agent/product/fixtures/provider_catalog_codex.json`.

| Test name | Catalog / rows | Observed result |
| --- | --- | --- |
| provider card: idle available + stopped | Codex only, enabled, available, no rows | 已启用 / 可用 / 已停止 / 0; no red status |
| provider card: disabled without active execution | Codex only, disabled, available, no rows | 已停用 / 可用 / 已停止 / 0 |
| provider card: unavailable independently of enabled | Codex only, enabled, unavailable | 已启用 / 不可用 / 已停止 / 0 |
| provider card: disabled and unavailable remain separate | disabled + unavailable | 已停用 / 不可用 remain separate fields |
| provider card: draining while execution remains active | disabled + running row | 正在停用 / 可用 / 运行中 / 1 |
| provider cards dynamically render two providers and aggregate frozen row provider IDs | Codex + CodeBuddy, shared row display names, current general route Codex | Counts 1 and 2 by frozen row.provider.id; completed row excluded; another Provider excluded; locally hidden active row included |
| unknown provider uses id and missing version/protocol fallbacks with the same card layout | future-agent with null metadata; new-provider with omitted metadata | ID names, version/protocol —; identical six-field layout |
| catalog failure leaves composer and existing history usable | rejected local catalog getter | Independent catalog alert; Composer submit remains enabled after typing; details remain accessible |
| provider presentation and card layout contain no provider ID special cases | source assertions + dynamic fixtures | No Codex/CodeBuddy strings in panel/presentation; catalog array map and typed local IPC present |

Each state-matrix test checks Agent 管理 title, Agent 接入 preceding Composer, and preserved Composer/history regions. Cards contain no role select. The shared two-provider test checks the visible loaded-row and non-OS-state explanation.

Existing App navigation tests now expect Agent 管理, including tray navigation, main navigation order, task detail navigation state, and workspace selection on task start. Existing AgentPanel tests for Cancel, Resume, Manual Resolve, Composer, list, details, pagination, and continuation also pass in the 95-test focused run.

The runtime display derives solely from active loaded rows: dispatch_pending, running, cancel_requested, cancelling, finalizing, reconciling. It is not backend Runtime evidence and does not count unloaded history as a global total. No state is written back.
