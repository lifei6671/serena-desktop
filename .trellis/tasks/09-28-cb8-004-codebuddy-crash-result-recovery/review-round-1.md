# Independent FULL_SCOPE review — round 1

冻结目标：23/23 SHA256 verified，baseline `a42176717c33ea08c7f5be3f6fc96107f85fe578`。

Gate：`CHANGES_REQUIRED`。P0=0，P1=0，P2=3，P3=0。

1. P2：A window fixture统一使用 `running/dispatched`，没有覆盖真实 `dispatch_pending/not_dispatched` 与 `dispatching -> uncertain` pre-flush startup状态。
2. P2：invalid R1 evidence只执行一次 startup，没有证明 evidence不变时第二次仍 `Unknown + Claim retained` 且零 R2/wire/attempt/revision变化。
3. P2：缺少独立 orphan Result-Recovery R2 被 provider orphan runtime scan捕获且不绕过 Claim 的验收。

修复：新增 A1/A2真实 generic 状态；在 invalid R1 fixture原地二次 startup断言；新增 private provenance缺失 + next Host orphan scan测试。生产实现未因这三项改变。

