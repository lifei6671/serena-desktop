# Design

权威来源：implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md CB2-003 与 technical-design-multi-agent-provider-codebuddy-v0.1.md §26。SHA256 已与用户给定版本核验一致。

Local Tauri commands 是薄入口，注册在现有 invoke handler。Supervisor 的 operation mutex 保护最新 config 的读改写；复用 config::save 原子替换，成功后才发布内存配置及共享 admission 策略。复用必要的 Broker management lock，锁顺序沿用既有路径。避免 stale whole-config 覆盖策略。

Provider settings 使用 CB2-001 类型和验证。CB2-002 admission 必须观察同一成功提交后的策略。Registry health 的刷新只探测 admission 可用性，禁止 execute/session/ACP。具体探测复用已有 Codex discovery；未注册 Provider 返回现有错误。Health 与 enabled 独立。

保持 Execution 冻结字段不变，不改变 routing。新增测试覆盖持久化失败、串行管理、未知合法 id、null、health 副作用与 Remote 隔离。设计冲突才报告 DESIGN_BLOCKER。
