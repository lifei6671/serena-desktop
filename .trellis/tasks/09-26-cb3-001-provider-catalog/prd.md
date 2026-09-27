# CB3-001 Provider Catalog Product Snapshot
用户授权已批准设计实施及本任务记录；Phase 2 Host Gates 已通过，完成后仍须 Host review。
只读 providers[]: id/displayName/version?/enabled/health/availableForNewExecution/capabilities 六项；当前五固定 roleRouting。各事实分离；仅枚举 Registry，未知合法 Provider 配置不伪造注册条目，未知 route 与 null 原值保留。
availableForNewExecution = agentEnabled AND enabled AND health==available AND canExecute。capabilities 原样投影声明。不创建 Runtime/ACP/Session/Execution/Claim，不修改 policy/health，失败同样无副作用。
测试覆盖 Codex available/enabled/registered、disabled、unavailable、未注册 route、canExecute=false、agentEnabled=false、前后无副作用、read failure、稳定 camelCase JSON fixture。
允许 Product、Registry read methods、focused tests 与本任务 artifacts。禁止 MCP/schema/router、UI、Start routing、Provider mutation、CodeBuddy-specific logic、commit、cleanup、CB3-002。已知 usage_tests.rs Clippy blocker 不修。
权威文档任务卡 CB3-001 及 technical design §7/§7.1，哈希已与用户给定值核对一致。
