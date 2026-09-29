# 持久化并展示 Execution 实际模型与推理强度

## Goal

在不改变现有 `executionProfile` 请求快照语义的前提下，为每个 Execution 持久化 Provider 已可靠确认的实际模型与推理强度，并让任务详情优先展示这份历史事实。

## Requirements

- 保留 `executionProfile`：它仍只表示创建 Execution 时冻结的请求配置，来自 `execution_profile_json`，不得被 Provider 默认或实际配置改写。
- 新增 provider-neutral `effectiveExecutionProfile`，持久化 Provider 对本次 Execution 已可靠确认的实际 `model` 与/或 `reasoning`；两个字段独立 nullable，但至少一个字段必须有可靠非空值。历史记录允许整体为 `NULL`，不得回填默认值。
- effective profile 首次写入后不可被不同值覆盖；相同值重试幂等。写入必须同时验证 exact Execution、原 Runtime 与 Provider 身份，且不改变 lifecycle、Claim、status、dispatch state 或 revision。
- Codex 必须基于当前 managed Client 的真实 `model/list` 解析显式/默认 model 与 reasoning；完整 profile 必须在 `thread/start` / `turn/start` 模型副作用前持久化，无法可靠解析时 fail closed。
- CodeBuddy 必须基于 exact session 的 `session/new` + config ACK 后 `SessionCatalog` 当前 model 与 raw `availableModels` 权威解析；`supportsReasoning=false` 时只记录实际 model 并忽略陈旧 `thought_level`，支持 reasoning 时才要求并记录 exact 当前值。fresh/continue 都必须在 `session/prompt` 前持久化，禁止启动独立 Runtime 猜测。
- Continue child 不继承 parent 的 effective profile；Recovery 不启动新 Runtime 补造缺失证据。
- `ExecutionView` 同时投影 required-nullable 的 `executionProfile` 与 nullable `effectiveExecutionProfile`，get/list/observe 保持一致。
- 任务详情显示逐字段优先级：`effectiveExecutionProfile.field ?? executionProfile.field ?? "Provider 默认"`；不得读取当前 `roleDefaults` 或 Provider catalog 反推历史默认。
- 新增 `schema_v14.sql` 并升级 StateStore 版本；v13 -> v14 只增加 nullable evidence 列，不改写旧 schema、不回填历史行。
- 保持 `docs/ui/DESIGN.md` 的紧凑信息密度，不增加卡片或重排页面主体。
- 增加 migration、StateStore CAS、Codex、CodeBuddy、Product projection/schema 与前端任务详情回归测试。
- 不改变 Runtime safety、Claim、Recovery、requestKey、`execution_profile_json` hash/identity、Provider terminal 或 usage contract。
- 不提交、不 push。

## Acceptance Criteria

- [ ] v13 -> v14 migration 保留 `effective_execution_profile_json = NULL`，future-version 与 migration coverage 同步。
- [ ] StateStore 专用写入口覆盖 first write、same retry、different value、provider/runtime mismatch、合法 partial profile 与非法空 profile。
- [ ] Codex explicit/default model+reasoning 解析正确；continuation 前置验证失败不落证据，成功路径在各自首个 model-bearing side effect 前完成 CAS。
- [ ] CodeBuddy fresh 与 continue 从 exact session ACK/current config 解析，并证明 persist 先于 prompt side effect；non-reasoning model 只记录 model 且不阻塞 prompt。
- [ ] Product detail/query/observe/list 同时一致投影 requested 与 nullable effective profile；历史 null 不受当前 catalog/default 改变。
- [ ] 前端三层优先级 effective > requested > Provider 默认有回归测试。
- [ ] 相关 Rust/前端测试、migration 测试、build/lint、cargo fmt/check 与 `git diff --check` 有如实记录。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
