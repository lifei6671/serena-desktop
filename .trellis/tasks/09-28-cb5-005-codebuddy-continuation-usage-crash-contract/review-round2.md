# Independent FULL_SCOPE Review — Round 2

Mode: `CHILD_AGENT` (independent, read-only)  
Target: 48/48 files, tree SHA256 `5c93c65020cd685639c7225df658b9fdf715b581e99fcd16cd27c4b97adf41b8`  
Gate: `BLOCKED`  
Findings: P0=0, P1=0, P2=1, P3=0.

## Coverage

- 全部 48 个目标文件从头读取；authority hashes、branch/HEAD、production zero diff 与冻结身份均匹配。
- 25 个 JSON、11 个 JSONL（55 rows）可解析；6 份真实 raw wire 均为严格四帧 initialize/session-new request/response。
- Round 1 的三项修复均已闭合；watchdog 分支只做静态审查，没有真实端到端触发。

## Finding

- P2 — `lateBehavior` 使用 prompt window 的全局 late-event 状态，导致早期事件独有的可选字段也可能被其他字段的 late event 连带标记为 `observed_after_terminal`，违反逐字段证据语义。

## Repair

- 每个字段现在只基于自身 sample 的 exact prompt binding 计算 terminal coverage 与 late behavior；unbound 时明确为 `unknown_unbound`。
- 新增可选 `cost` 仅存在于 terminal 前、`used` 同时有 late sample 的 fixture，证明两字段不会互相污染。

修复只改变 task-local parser/test/docs，没有重跑 Provider，也没有改写 raw evidence 或最终 conservative decision。
