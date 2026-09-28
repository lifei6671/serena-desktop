# Independent FULL_SCOPE Review Context — Attempt 3 Repair

> **STALE — DO NOT REVIEW AS CURRENT:** this context belongs to the superseded attempt-3 freeze. Host attempt 4 has since completed; the task remains `in_progress` pending Host attempt-5 Continuation semantic lineage.

Review mode: strict independent `CHILD_AGENT`, read-only. Do not edit files, run mutating tests, rerun CodeBuddy, delete temp files, commit or push.

## Target

- Baseline/HEAD `b83418a3d12681d1cb372eebd9f58c53e08cc98a`, branch `feat/codebuddy`.
- Delivery-owned scope is this task directory only; production `src/`, `src-tauri/` and authority `docs/` have zero diff.
- Frozen target: 60 files, tree SHA256 `de482e9ec0c029c84c284f2ab568bad66fb0d1c57133c0c986be7b47e1e39c08`.
- Hash algorithm/exclusions are authoritative in `freeze.json`. `review.md` is the explicitly superseded pre-repair review; future `review-attempt3.md` is excluded review metadata.

## Authority and risk

Tier 3 protocol/evidence review. Governing sources: current user repair request; CB5-005 authority docs; CB5-004 Host evidence; root `AGENTS.md`; JavaScript review profile.

Additional frozen Host sources:

- `host-direct-codebuddy-proof/wire.jsonl` SHA256 `bf193e9d353357a1114293307107a4d5b6a1e671e6b2a6cb4c4fe45ae04b1b01`
- `host-exact-launcher-proof/wire.jsonl` SHA256 `5f2c4df063d214627f17e8a5bf77975d6b4e935d648a2127d0b2d16ec81b9e9f`
- Pre-repair `harness/probe.mjs` SHA256 `33f8d0a7b202362e154fb4a3614f490eedf04abdeea42e42ed4ccc9ace8e5c01`

## Host finding and repair

The previous review missed a Gate-level initialize-shape drift. Attempt1/2 used different `clientInfo` and omitted CB5-004 `clientCapabilities._meta`; both are now `NON_EQUIVALENT_INITIALIZE_PROBE` diagnostic history. Their raw files remain unchanged and cannot support Provider/backend attribution.

Attempt-3 repair review round 0 then found one P1 evidence-provenance inconsistency: `capability-schema-inspection.md` still cited attempt1/2 for method selection. That paragraph now explicitly excludes attempt1/2 and freezes `session/load` only from pinned typed schema plus exact attempt3 `loadSession=true`; see `review-attempt3-round0.md`.

The repaired generator exactly matches both frozen CB5-004 Host requests:

```json
{"protocolVersion":1,"clientCapabilities":{"elicitation":{"form":{}},"_meta":{"subagent-transcript":true,"parameterizedModelPicker":true}},"clientInfo":{"name":"serena-desktop-gold-band-repro","title":"SerenaDesktop Gold Band Repro","version":"0.1"}}
```

The unit fixture verifies both source hashes and deep structural equality; attempt-3 `initialize-baseline.json` persists the exact params and params SHA256 `921a7bc1ec9fd8049cd908221b742dafb2d4c154cd7be8be2dd13304828f38b4`.

## Attempt 3 actual result

- Direct installed CodeBuddy 2.158.0, absolute `node.exe`/script, standard environment, ordinary task-owned Win32 temp cwd.
- Wire has exactly four rows: initialize request/response, session/new request/error.
- Initialize succeeded with `protocolVersion=1`, `loadSession=true`.
- Fresh session/new returned `-32603`, sanitized detail `Request failed with status code 500`.
- Fresh prerequisite gate stopped the attempt: no prompt, usage, S1, R2, session/load, session/resume, crash window or fallback.
- Wire SHA256 `74a6439ff305ea06e127086feae518cb5af1cab8c685bd9acc28b42abe493470`.
- Provider cleanup evidence: direct child reaped after taskkill failure plus direct SIGKILL; final manifest `{}`, workspace deleted, PID absent, temp residue 0. This is diagnostic only, not Windows Job/Claim authority.
- Result/summary/process evidence persisted before the outer harness was interrupted. The interrupt was needed because the completed-operation watchdog timer remained referenced. The code now clears it; a non-Provider child-process test proves exit within two seconds. Real Provider was not rerun.
- Attempt-3 failure result did not persist the already-computed initial empty manifest. The repaired harness now writes before manifests on all continuation/crash result paths, but that post-run repair is not represented as attempt-3 raw evidence.

## Final classifications

- Continue `INCONCLUSIVE`; unique candidate remains `session/load`, but no S1 or load call exists.
- Usage `INCONCLUSIVE`; zero events are not zero/unsupported; `tokenUsage=false`.
- Crash four windows `NOT_OBSERVED / NOT_RUN_BY_FRESH_SESSION_GATE`; result completeness unknown.
- Private schema `EXISTING_FIELDS_SUFFICIENT` only for the current conservative state.
- CB8-003 and CB9 remain blocked; no follow-on implementation entered.

## Required checks

Inspect all 60 target files from scratch, not only repair hunks. Explicitly verify:

1. initialize generator, test fixture, baseline evidence and attempt-3 wire establish exact CB5-004 params equality;
2. attempt1/2 are consistently reclassified as non-authoritative diagnostic history and their raw evidence is not rewritten or used for root-cause claims;
3. attempt-3 wire/hash/result/process/cleanup support only the stated fresh-session failure and downstream gate stop;
4. no `session/resume`, alternative recovery method, replay, fallback or hidden extra real attempt exists;
5. Continue/Usage/Crash/DCR decisions do not exceed attempt-3 evidence;
6. R2/PID/new Runtime is never used as original Runtime Job termination or Claim release proof;
7. watchdog timer repair, before-manifest repair and their NOT_RERUN boundary are honest;
8. production/code/docs authority scope is zero diff, task remains `in_progress`, no commit/push or CB8-003 work occurred;
9. all JSON/JSONL parse, frozen target identity matches, and review coverage is complete.

Return reviewed identity, complete path/partition coverage, P0–P3 findings with `file:line` evidence (or zero), gate `PASSED|BLOCKED|UNAVAILABLE`, and remaining risks. A correctly bounded `INCONCLUSIVE` is not itself a review failure.
