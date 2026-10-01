# Host review narrow follow-up

## Scope

只补CB8-003 continued child same-runtime `Partial`在prompt期间crash后，CB8-004必须启动独立external R2的跨卡测试。production authority顺序、schema、public capability与CB8-003 semantics保持不变。

## Transaction predicate review

`complete_codebuddy_runtime` 是complete evidence的sealed writer：它先对原Runtime snapshot执行完整`valid_identity`，并在同一事务的条件更新中固定provider、owner、Job name/session、Job-at-creation policy、platform/containment/process identity以及空的其他containment字段。complete后现有typed runtime mutation不会改写这些identity/evidence字段。

`approved_runtime()`在R2启动前、R2 shutdown后以及最终Claim release前重读durable Runtime，并组合`complete()`与当前Windows Session下的完整`valid_identity()`。因此完整authority不来自Store内的简化SQL predicate。

`begin_codebuddy_result_inspection`与`finish_codebuddy_result_inspection`在`IMMEDIATE`事务内再检查ownership、Claim和已sealed complete evidence的稳定子集，防止在登记/完成inspection时接受缺失或降级的Runtime行。虽然这两个局部predicate没有逐字段重复`valid_identity()`，但它们夹在完整`approved_runtime()`检查之间，且sealed typed writer之后没有合法路径改变被省略字段；最终release还会再次执行完整R1/R2检查。

结论：当前前后完整`approved_runtime()` + sealed `complete_codebuddy_runtime` write +事务内稳定子集检查已足够安全。为避免重复predicate漂移和越过本次窄修复，不重构production Store代码。

