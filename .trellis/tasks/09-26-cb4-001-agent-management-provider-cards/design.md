# CB4-001 Design

## Source of truth

Provider cards consume existing ProviderCatalogSnapshot：descriptor/version + enabled + health + capabilities + roleRouting。
Local UI 通过最小只读 Tauri bridge 获取；TypeScript 只做镜像类型，不创建第二 Authority。
activeExecutions 从已加载 ExecutionView 按 frozen provider.id 统计。

## Presentation

在 agentPresentation.ts 提供 provider-neutral helpers：display fallback、enabled/health labels、active count、runtime presentation、draining。
不使用 codex/codebuddy 条件分支决定卡片结构或状态。
当前 Product Catalog 没有 protocol metadata，因此不得从 providerId 猜协议。缺失时显示“协议：—”等中性 fallback；未来 descriptor 增强后直接消费数据。

## Read-only bridge

commands.rs 增加 local-only provider catalog getter；直接调用 product.provider_catalog(supervisor)。
lib.rs 注册；api.ts 加 typed call；types.ts 镜像 DTO。
不得刷新 health、启动 Runtime、创建 Execution/Claim 或修改配置。

## Layout

Agent 管理 -> Agent 接入 Provider cards -> 现有新任务 Composer -> 最近任务/详情。
Role editor 排除在本卡外。

## Tests

- read-only bridge no side effects / unknown provider retained
- Codex only / two-provider fixture
- idle / disabled / unavailable / draining
- unknown metadata fallback
- existing AgentPanel regression