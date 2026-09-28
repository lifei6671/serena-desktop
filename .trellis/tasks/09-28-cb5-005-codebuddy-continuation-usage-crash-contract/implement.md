# 执行计划

1. 静态检查 pinned ACP schema 与 CodeBuddy 2.158.0 bundle，冻结 `session/load` 和 provenance。
2. 编写最小 task-local JSON-RPC harness 与 normalization tests。
3. 先运行 `node --test` 和 inspect；PASS 后再运行一次真实 continuation/usage Probe。
4. 串行运行四个独立 crash 窗口，保存所有失败与 cleanup evidence。
5. 生成逐字段 Usage 表、crash matrix、DCR、verification 与 validation-results。
6. 冻结 task-local target，执行生产源码零 diff、三份 authority hash、`git diff --check`，委派独立只读 `FULL_SCOPE` review。
7. Host repair：以两份 CB5-004 Host PASS wire 为 fixture 修复 initialize exact equality，将 attempt1/2 降级为非等价诊断证据。
8. 静态门通过后只执行一次独立 attempt-3；fresh prerequisite 失败即停止，否则继续完整 continuation/usage/crash。
9. 依据 attempt-3 更新 decision/DCR/verification，重新 freeze，并执行新的独立只读 FULL_SCOPE review。
10. Host environment repair：增加 gated `probe-host-attempt-4`、独立 evidence root 与安全环境 provenance；仅运行静态 tests。
11. 停止于 Host handoff；由 SerenaDesktop command_execute 执行 attempt-4 后再更新最终 decision、freeze 与 review。
