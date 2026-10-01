# Design

保持 `reconcile_execution` 的 R1 ownership / Job proof authority 为外层安全门。读取 original execution 与 private state，计算 staged exact terminal，但在任何 R2 side effect 前先执行/重读 R1 durable termination evidence。staged exact terminal 在 R1 proof 后直接按 original terminal/result 收敛，不创建 R2，避免 replay 降级。

非 staged 路径增加窄 Result-Recovery inspection orchestration：从 exact private snapshot取得 S1、local conversation identity、optional provider request identity与 canonical workspace；创建新的 recovery runtime attempt R2，但不写 generic `execution.runtime_instance_id`；以既有 `Runtime::start_persisted` 和 typed ACP client完成 initialize 与唯一 `session/load`。inspection route 只收 exact S1 history，按 exact conversation过滤 assistant text并限制 bytes；missing/foreign/ambiguous identity归为 unknown/material difference，不公开任意历史正文。

R2 dispatch 前通过现有 `BeginInspection` 原子绑定 private `recovery_runtime_instance_id`，并在 load 完成后使用 `FinishInspection` 记录 `Partial/Unknown/MaterialDifference`。这些状态只描述 inspection。无论 load 成败，都关闭 R2并重新读取其 durable Job evidence；只有 R1和任何已启动R2均为 complete approved evidence才进入 provider-neutral finalization。

R2 launch/cleanup 失败时保留 recovery runtime durable record，使 startup orphan recovery可继续收敛；Execution 保持 unknown + Claim。restart时若 inspection已结束且R2 evidence complete，可复用既有 inspection outcome/result envelope继续finalize，不创建新 R2；若 inspection尚未结束，则先恢复 orphan R2 evidence，再按持久化状态安全推进或保持 unknown。首版不新增字段或 migration，因此 partial text使用现有 generic final result staging/inspection可安全表达的最小结构；若实际字段无法持久化 exact-bound text并保证restart幂等，则该分支保持 unknown而不扩大 schema。

测试使用 Win32/native fake、真实 SQLite/Job fixture和 fake ACP peer。生产不调用真实 CodeBuddy CLI。测试显式记录 method序列、runtime identity、Job evidence、result completeness、Claim与restart revision。
