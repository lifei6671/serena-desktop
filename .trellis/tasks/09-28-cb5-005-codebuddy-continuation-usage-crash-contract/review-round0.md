# Independent FULL_SCOPE Review — Round 0

Mode: `CHILD_AGENT` (independent, read-only)  
Target: 46/46 files, tree SHA256 `9b665130787c0db4b5833b9e70bf1609b1e473f09efafd1ba4febe2f7f6abda6`  
Gate: `BLOCKED`  
Findings: P0=0, P1=0, P2=3, P3=1.

## Findings

1. P2 — Usage exact Prompt/provider identity 只检查任意 identity 存在，未与 P1/P2/P3 已知 identity 做相等绑定；scope/reset 固定 unknown。
2. P2 — crash exact-result 在已过滤为 `session/update` 的集合中查 prompt response，分支不可达。
3. P2 — pipe write callback 和整次真实 Probe 缺 bounded watchdog；attempt 2 实际需要人工 interrupt。
4. P3 — `design.md` 把零事件直接写成 unsupported，与未到达 prompt 时应 INCONCLUSIVE 的最终规则矛盾。

## Repair

- Usage analyzer 现在要求 exact session + provider/RPC identity 与唯一 prompt window 相等；从非-usage prompt frames/terminal 收集 provider identity，并据 P1/P2/P3 数值序列给出 cumulative/reset observation，否则 unknown。
- crash exact-result 从 R2 全消息检查 original prompt RPC response；新增正反 fixture test。
- pipe write 增加 10 秒 timeout；真实 probe 增加 20 分钟 hard watchdog、活跃 Runtime best-effort kill 与 local stdio/readline handle cleanup。
- 修正文档：只有有效 prompt + bounded grace 后无事件或明确拒绝才可评估 unsupported；未到达 prompt 仍为 INCONCLUSIVE。

修复只改变 task-local harness/docs/tests，没有重跑真实 Provider，没有改写 raw evidence 或最终 conservative decision。
