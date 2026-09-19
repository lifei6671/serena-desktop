# P0-006 CodeGraph CLI evidence

Date: 2026-09-19. Evidence was collected with `C:\\Users\\lifei\\AppData\\Local\\codegraph\\current\\bin\\codegraph.cmd`; all experiment paths below are replaced with `<TEMP>`.

## Read-only help and version

- `codegraph --version` -> `1.6.0`
- `install --help` documents `--target <...|none>` and `--yes` as non-interactive.
- `init --help` documents `--yes` as non-interactive for scripts/CI.
- `status --help` documents `--json`.

## Isolated bootstrap

For the experiment, HOME, USERPROFILE, APPDATA, LOCALAPPDATA, XDG_CONFIG_HOME, and XDG_DATA_HOME all pointed below `<TEMP>/isolated-user`; PATH was unchanged. `codegraph install --target=none --yes` exited 0 with `No agent targets selected - nothing to do.` It created only `<TEMP>/isolated-user/.codegraph/telemetry-queue.jsonl` plus empty configuration roots. No real Agent config or business workspace was touched.

## Machine-readable status

`status --json <TEMP>/workspace` before init (exit 0):

```json
{"initialized":false,"version":"1.6.0","projectPath":"<TEMP>\\workspace","indexPath":"<TEMP>\\workspace\\.codegraph","lastIndexed":null}
```

`init --yes <TEMP>/workspace` exited 0. The first ready status (exit 0) included:

```json
{"initialized":true,"projectPath":"<TEMP>\\workspace","pendingChanges":{"added":0,"modified":0,"removed":0},"index":{"reindexRecommended":false,"state":"complete","pendingRefs":0}}
```

After adding `fixture.rs`, status exited 0 and reported:

```json
{"initialized":true,"projectPath":"<TEMP>\\workspace","pendingChanges":{"added":1,"modified":0,"removed":0},"index":{"reindexRecommended":false,"state":"complete","pendingRefs":0}}
```

`sync <TEMP>/workspace` exited 0. Its post-status had zero pending changes but `index.reindexRecommended:true`, so it remains stale/degraded by the frozen readiness rule. `index <TEMP>/workspace` exited 0; its post-status reported zero pending changes, `index.state:"complete"`, and `index.reindexRecommended:false`.

## Contract decision

The tested schema provides all frozen fields. `pendingChanges` is an object, so non-zero means any of `added`, `modified`, or `removed` is non-zero. `reindexRecommended` is nested under `index`. This is a minimal compatibility clarification, not a material contract difference.
