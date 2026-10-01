# Superseded Independent FULL_SCOPE Review

> **SUPERSEDED_BY_HOST_GATE_FINDING**：本 review 未识别 harness initialize params 与 CB5-004 Host PASS 不等价，不能作为 attempt-3 repair 的最终批准。其历史内容保留；新冻结目标必须另行独立只读 FULL_SCOPE review。

Mode: `CHILD_AGENT` (independent, read-only)  
Depth: Tier 3 protocol/evidence  
Coverage: `COMPLETE — 49/49`  
Gate: `PASSED`

## Frozen target

- Branch: `feat/codebuddy`
- HEAD/baseline: `b83418a3d12681d1cb372eebd9f58c53e08cc98a`
- Target files: 49
- Tree SHA256: `5254fd61e009679ccfceb822dca1849a851f48a429c765dc5d0a8940b52461e6`
- Production and authority docs diff: zero
- Findings: P0=0, P1=0, P2=0, P3=0

## Coverage

- Planning/decision/reviews: 15/15
- Harness: 4/4
- Root evidence: 11/11
- Attempt-2 shared evidence: 7/7
- Four crash evidence partitions: 12/12
- 25 JSON and 11 JSONL files parsed; 58 JSONL rows; six real wire files each contained the strict four-frame initialize/session-new sequence.

## Critical boundary result

- Usage scope/reset remain `unknown`; per-field terminal/late classification uses only that field's exact-bound samples.
- Missing usage is not zero or final unsupported.
- Only `session/load` is callable; no `session/resume` fallback exists.
- Exact crash result requires original RPC id plus typed `result.stopReason`.
- R2/PID absence never proves R1 Job termination or Claim release.
- Watchdog cleanup evidence does not claim Windows Job authority.
- Permission handling is typed `reject_once` and fail-closed; no bypass flags.
- DCR invents no private fields or migration.

## Remaining risks

- Real continuation, usage and crash recovery remain unobserved because both Provider attempts stopped at `session/new` HTTP 500. Their conservative `INCONCLUSIVE` / `NOT_OBSERVED` classifications are correct and are not review failures.
- The repaired watchdog path was statically reviewed but not triggered end-to-end.
- The independent reviewer did not rerun the filesystem-mutating unit suite; frozen verification records 11/11, while the reviewer independently ran syntax checks and a non-mutating Usage fixture.
