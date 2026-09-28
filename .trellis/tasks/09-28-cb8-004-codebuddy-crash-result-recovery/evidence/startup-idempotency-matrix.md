# Startup and restart idempotency

| 场景 | 第一次 startup | 第二次 startup | durable 断言 |
|---|---|---|---|
| A/B/C/D 已安全收敛 | finalize/release 一次 | 无 reconcile item | execution revision、Provider wire、runtime attempt count 均不变 |
| R1 evidence 缺失 | `Unknown` + Claim retained，无 R2 | 相同 evidence 下仍 `Unknown` | 零 R2/wire/new attempt，execution revision不变，Claim retained |
| R2 evidence 首次失败 | `Unknown` + Claim retained，private 保持 `Inspecting` | recover 原 R2 后 `Interrupted/Unknown` | 无新 R2、无新 wire、attempt count 不变 |
| staged exact terminal | 恢复 original terminal/result/completeness | 无 reconcile item | 不创建 R2，不被 replay 降级 |
| Provider disabled/unavailable | registered provider 仍执行历史 reconcile | 相同 durable authority | 不依赖 execute admission health；无 LaunchSpec 时安全降级为 unknown result |
| orphan R2 | private `Inspecting` 优先恢复 exact R2；private provenance损坏时由下一 Host 的既有 provider orphan scan 捕获 | 第二次同 owner只剩 original execution `Unknown` | R2 approved、无新 wire/attempt；不绕过或释放 original Claim，不改绑 R1 |

生产入口保持 `ProviderRegistry -> registered CodeBuddy -> startup_reconcile -> startup_with_launch`。LaunchSpec 只决定是否能做新的只读 R2 inspection，不参与 R1 ownership 或 Claim authority。
