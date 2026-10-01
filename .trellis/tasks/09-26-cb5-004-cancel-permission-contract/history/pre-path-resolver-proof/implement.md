# CB5-004 Implementation Plan

1. Capture HEAD/product baseline，复用 CB5-003 external-stream SDK 与 temp manifest 基础。
2. Inspect official SDK session/cancel 与 request_permission typed APIs。
3. 实现 cancel-before 与 cancel-after real probes。
4. 用 process-scoped temporary settings/default mode 触发真实 permission request，并执行 typed deny。
5. 捕获 permission deny 后 updates/terminal/manifest。
6. fake/unit tests覆盖 identity、malformed option、timeout、late update、cleanup failure。
7. 生成 cancellation/permission/process evidence 与 verification。
8. 独立 task-local review；验证 fmt、product 0变化、git diff --check。
9. Stop before CB5-005。

若真实 permission 或 cancel 场景无法稳定触发，明确 PARTIAL；不得用 fake peer替代真实 capability结论。