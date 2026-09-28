# Recovery matrix

测试入口：`agent::codebuddy::recovery::tests`，使用真实 SQLite、Windows Job fixture 与 native fake ACP peer，不调用真实 CodeBuddy Provider。

| 窗口/故障 | R1 evidence | R2 | R2 evidence | inspection strength | result completeness | final status | Claim |
|---|---|---|---|---|---|---|---|
| A1: Prompt/acceptance 前，private `Prepared` + generic `dispatch_pending/not_dispatched`，R1/session 已建立 | complete | 不创建 | N/A | `NotAttempted` | `Unknown` | `Interrupted` | release |
| A2: private `Sent` + generic `dispatch_pending/dispatching`，physical prompt flush 前 | complete | 创建，只读空历史 | complete | `Unknown` | `Unknown` | `Interrupted` | release |
| B: Prompt physical flush 后、terminal 前，空历史 | complete | 创建 | complete | `Unknown` | `Unknown` | `Interrupted` | release |
| B/C: exact conversation assistant text | complete | 创建 | complete | `Partial` | `Partial` | `Interrupted` | release |
| C: side effect 后、terminal 前 | complete | 创建 | complete | `Partial` 或 `Unknown` | `Partial` 或 `Unknown` | `Interrupted` | release；marker SHA256 不变 |
| D1: private exact terminal 已见、generic 未 staged | complete | 创建 | complete | `Partial` | `Partial` | `Interrupted` | release |
| D2: private + generic exact terminal/result 已 staged | complete | 不创建 | N/A | 保留原值 | `Complete` | original `Completed` | release |
| R1 policy/identity evidence 缺失 | 不完整 | 禁止创建 | N/A | 不推进 | `Unknown` | `Unknown` | retained |
| R2 empty/foreign/wrong session/load error/load mismatch/no capability | complete | 创建 | complete | `Unknown`/`MaterialDifference` | `Unknown` | `Interrupted` | release |
| R2 termination evidence 失败 | complete | 已创建 | 不完整 | `Inspecting` | `Unknown` | `Unknown` | retained |
| 上一行下一次 startup 取得 R2 approved evidence | complete | 复用原 R2 | complete | `Unknown` | `Unknown` | `Interrupted` | release |
| R2 orphan：private provenance 缺失、owner 已变化 | complete | 不创建新 R2 | orphan scan取得complete | 不推造 | `Unknown` | `Unknown` | retained；仅收敛 orphan R2 |
| CB8-003 continued child：same-runtime `Partial`（`recovery_runtime_instance_id == R1`）后 prompt crash，exact replay | complete | 创建exactly one external R2，`R2 != R1` | complete | external `Partial` 覆盖same-runtime provenance | `Partial` | `Interrupted` | release；second restart零wire/attempt/revision变化 |
| 同上，但external R2 termination evidence失败 | complete | 创建exactly one external R2 | incomplete | 保持external `Inspecting` | `Unknown` | `Unknown` | retained；same-runtime `Partial`不授权release |

所有启动 R2 的成功收敛分支均断言：`R2 != R1`，`execution.runtime_instance_id == R1`，`private.recovery_runtime_instance_id == R2`，且 Claim release 前重新读取 R1/R2 complete approved evidence。
