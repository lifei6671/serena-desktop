# CB3-003 Start DTO Compatibility Parser

## Goal

为 `agent_execute start` 增加 `taskRole + providerId` 的有界兼容解析，同时保持已发布旧 Start 请求可解析；本任务只冻结 DTO/parser/schema 契约，不执行 Role Routing 或 Provider dispatch。

## Requirements

- 显式新请求必须同时提供 `taskRole` 和 `providerId`。
- `taskRole` 使用既有 `AgentTaskRole` enum；`providerId` 使用既有 `ProviderId` validation。
- 两字段都缺失时解析为 legacy Start，并以 typed intent 表示 `LegacyGeneral`；不得在 parser 中读取当前 general routing。
- 只提供其中一个字段时稳定返回 `INVALID_PARAMS`，禁止猜测或 fallback。
- continue / cancel / resume_pending 不新增 routing 字段，携带 `taskRole` / `providerId` 必须被 strict schema/serde 拒绝。
- 原有 Start 的 workRunId / workspaceId / requestKey / prompt / context 兼容不变。
- JSON Schema 必须与 serde parser 得出相同兼容矩阵，不能把 routing 两字段建模成彼此独立 optional。
- parser 不访问 Product/Provider/Policy/Runtime/Store，不创建 Execution。
- `agent_query` descriptor/hash 保持 CB3-002 冻结值；`agent_execute` 因本卡 schema 变化显式更新 hash。
- 不进入 CB3-004 routing authority。

## Acceptance Criteria

- [ ] both absent -> PASS，typed LegacyGeneral。
- [ ] both valid -> PASS，typed Explicit(taskRole, providerId)。
- [ ] only taskRole -> INVALID_PARAMS。
- [ ] only providerId -> INVALID_PARAMS。
- [ ] invalid taskRole / providerId -> INVALID_PARAMS。
- [ ] unknown extra field -> INVALID_PARAMS。
- [ ] JSON Schema 与 parser 对上述矩阵完全一致。
- [ ] continue/cancel/resume_pending 携带 routing 字段全部拒绝。
- [ ] 旧 Start context/workspace validation 回归通过。
- [ ] agent_query hash 不变，agent_execute 新 hash 显式冻结。
- [ ] focused tests、registry/schema/hash tests、cargo check、fmt、git diff --check 通过；Clippy 若仅为已知 task 外 blocker 如实记录。
- [ ] 无 routing/dispatch/Product/TaskManager/Runtime/Provider mutation 增量。

## Notes

本任务风险来自公开 MCP 兼容性。模糊请求必须 fail closed，不猜测调用方意图。
