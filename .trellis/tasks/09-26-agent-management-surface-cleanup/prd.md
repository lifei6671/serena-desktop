# Agent Management Surface Cleanup

## Goal

按用户当前视觉验收反馈，把 Agent 管理页收窄为 Provider / Role 管理页：移除主内容区底部的“新任务”Composer 和“最近任务”列表；保留 Agent 接入、角色分工、当前工作区信息，以及左侧工作区任务导航/任务详情能力。

## User-visible scope

- 保留页面标题“Agent 管理”。
- 保留“Agent 接入”Provider Cards、pending Claim warning、Provider enable/disable。
- 保留“角色分工”Role Routing Editor。
- 保留“当前工作区”信息条和“管理工作区”入口。
- 移除主内容区：
  - “新任务”标题、textarea、开始新任务按钮和 composer footer。
  - “最近任务”标题、筛选器、刷新按钮、任务卡列表、加载更多/空态等主内容历史区。
- 左侧工作区树中的任务导航保持不变；用户仍可从左侧打开已有任务详情。
- 任务详情页、Cancel / ResumePending / Manual Resolve、删除/隐藏任务能力保持不变。

## Architecture

- 只调整前端页面呈现和必要测试。
- 不删除 Agent request/domain/API 能力；只是 Agent 管理主页面不再提供 fresh Start Composer 或主区 history list。
- 不修改 Rust backend、MCP schema/hash、Provider routing/runtime/policy。
- 不影响下一步 CB5-001 CodeBuddy Probe。

## Acceptance Criteria

- [ ] Agent 管理主页面 DOM 不再存在 #agent-prompt、开始新任务、最近任务主内容区。
- [ ] Agent 接入、角色分工、当前工作区仍存在且布局连续。
- [ ] 左侧 ProjectTaskNavigation 仍加载并展示工作区任务，可点击进入详情。
- [ ] 从左侧进入详情后详情页正常，返回 Agent 管理后仍无 Composer/最近任务主区。
- [ ] Cancel/Resume/Manual Resolve 详情操作回归通过。
- [ ] Provider pending Claim 的“查看任务”仍可直接进入详情，不依赖主区任务列表。
- [ ] Provider pending Claim 的“取消任务”仍可调用既有 cancel。
- [ ] 不影响 Provider Catalog polling、enable/disable、Role Routing。
- [ ] frontend focused/full tests、build、lint 通过。
- [ ] Rust/Remote 文件 0 变化，git diff --check 通过。
- [ ] 不提交 Git。