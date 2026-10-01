# Independent task-local review context

仅审查新增Host harness/runner/tests；绝不执行真实CLI。审查目标为harness readiness，不是三项真实合同PASS。Fresh session/new采用Host已证明PASS，旧nested失败仅历史。

冻结目标见review-target.json。代码包括新增binary/newtransport/integration/runner/tests与只读复用base.rs、Cargo.toml/lock身份；证据包括本proofscope、tests、verification和README。旧task root报告/hash/evidence未覆盖。Host两份主文档修改属于本轮baseline，不能撤销。

重点：external streams owned Child、exactinit actual wire证据、typed new/mode/cancel/permission、exact identities、无重放、永久allow不选、manifest/hash真实trigger、terminal/runtime分离、bounded cleanup、sentinel。README仅给runner正式Host入口；binary real path也固定output，identity create_new阻重复。输出只安全合同字段。

已处理review意见：safe_rows实际投影函数接线与通过真实safe_rows测试；Host只用runner，binary不能任意更换output绕过。无新增产品接口、依赖或Cargo修改。
