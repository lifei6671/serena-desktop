# CB2-003 Local IPC for Provider Policy

用户已授权创建、规划与实现本任务。唯一范围是任务卡 CB2-003 与设计 §26。

实现 agent_provider_settings_get、agent_provider_set_enabled(providerId, enabled)、agent_provider_set_role_route(taskRole, providerId?)、agent_provider_refresh_health(providerId)。复用既有校验，保留合法但未注册 Provider 配置；null 清空角色路由。

验收：持久化策略可读取与重启恢复；并发修改由既有管理锁串行；持久化失败不改变 Supervisor 或 admission 内存 Authority；toggle/route 不创建 Runtime/Execution/Claim；health refresh 只进行 Admission probe，无 Execution/Session/ACP；已有 Execution 的冻结路由不变；Remote registry 无 mutation commands；CB2-002 回归通过。

保留全部起始 dirty 文件（baseline-files.json 与 baseline.patch 记录）。禁止 UI、CB2-004、CodeBuddy Runtime/ACP、routing contract 变化、任务外 warning 修复、git commit。
