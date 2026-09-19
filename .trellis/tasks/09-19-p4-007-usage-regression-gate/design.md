# P4-007 设计：Usage Regression Gate

## 权威与范围

以 technical design §52、§53、§56.37～§56.42、§101 和 revision003 的 P4-007 为准。P4-001～P4-006 已由 Host Acceptance；本任务只联合验证其公开 Usage 合同，不新增功能。默认产物仅为 Trellis task/evidence；test-only fixture repair 仅在失败明确归类为 `FIXTURE_STALE / EXPECTATION_STALE` 后允许。

## Gate 结构

G01～G17 覆盖领域校验、v9 迁移、固定 Codex parser/identity、epoch 基线和 delta、终止 grace/freeze、Product Projection/N+1/private leak、MCP strict schema/hash 与 title cleanup 回归。先以模块过滤定位，再以 Product full 和 full lib 的失败集合确认跨模块兼容性；失败集合按完整 test name 和可观察 failure signature 比较，而非只比数目。

## 失败分类

- `HISTORICAL_BASELINE`：仅 Host 列出的三个失败，签名不变，记录但不修复。
- `FIXTURE_STALE / EXPECTATION_STALE`：P4 已授权公开 DTO/schema 变更导致的夹具过期；最小 test-only 修复、复跑相关 Gate、如实记录。
- `NEW_PRODUCTION_REGRESSION`：立即停止 production 修复，最终 Gate FAIL 并报告根因。
- `ENVIRONMENT/NOT_RUN`：保存命令和错误证据，绝不伪作 PASS。

## 验证边界

不用真实 `account/usage/read` endpoint 或已清理的 P0 原始 JSONL；使用 P4-003 已验收的 evidence-derived fixture。absence 类要求用最小 source/static assertion 证明：不以 account Usage 作为 checkpoint、无跨 runtime subtraction、Product 不求 breakdown total、不写 Codex complete、Product 不访问 private Usage 表。
