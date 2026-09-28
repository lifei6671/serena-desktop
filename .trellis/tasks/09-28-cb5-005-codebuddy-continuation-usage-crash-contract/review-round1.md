# Independent FULL_SCOPE Review — Round 1

Mode: `CHILD_AGENT` (independent, read-only)  
Target: 47/47 files, tree SHA256 `012dafec2e8b7bb10f281e4340dc553fc1b9816bcccb573c5fd2ba50d73ff31b`  
Gate: `BLOCKED`  
Findings: P0=0, P1=0, P2=3, P3=0.

## Coverage

- 全部 47 个目标文件从头读取；authority hashes、branch/HEAD、production zero diff 与冻结身份均匹配。
- 25 个 JSON、11 个 JSONL 可解析；6 份非空 raw wire 均为 4 rows 且 sequence 严格递增。
- 真实 Contract observation 仍不完整：没有 S1、prompt、R2、`session/load`、usage 或 crash target window；如实分类本身不计 finding。

## Findings

1. P2 — Usage analyzer 仅凭跨 prompt 数值增减推断 `cumulative`/reset，不能排除 per-turn、delta 或 gauge。
2. P2 — crash exact-result 接受同 RPC id 的 JSON-RPC error，未要求 typed `PromptResponse.stopReason`。
3. P2 — hard watchdog 直接退出前没有保存 final Workspace manifest、process reap/cleanup evidence，也没有执行 owned Workspace cleanup。

## Repair

- Usage 保留 exact Prompt/session identity binding，但未有 typed 语义时逐字段 `scope`/`resetBehavior` 始终为 `unknown`；数值走势不授权语义。
- exact recovered prompt result 只接受 original prompt RPC id 且 `result.stopReason` 为字符串的 typed terminal；error 和空 result 均拒绝，并增加 fixture assertions。
- watchdog 改为 timeout race：冻结 abort 状态，bounded 终止并记录活跃 Runtime，允许原路径短暂 unwind，再为全部跟踪 Workspace 落 final manifest、执行归属约束 cleanup、写 `watchdog-cleanup.json` 后 exit 124；仍明确不构成 Windows Job/生产 termination proof。

修复只改变 task-local harness/docs/tests，没有重跑 Provider，也没有改写 raw evidence 或最终 conservative decision。
