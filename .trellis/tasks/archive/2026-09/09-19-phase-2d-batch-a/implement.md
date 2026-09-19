# Phase 2D Batch A Implementation Plan

1. P2D-001：实现并注册 Source in-process adapter，切换本地 Source 路由；运行 Source 与 Remote-disabled 聚焦测试。
2. P2D-002：实现并注册 Git stateless adapter，切换 Git 路由；运行 Git 六工具、A/B、non-Git、cancel 聚焦测试。
3. P2D-003：证明 Git `path` 字段和 `WorkspacePathResolver` 边界不变；运行有效/无 path、absolute、UNC、`..`、junction/reparse 聚焦测试。
4. 最终执行相关 Registry/Manager 回归、`cargo check --locked`、owned Rust `rustfmt` 与 `git diff --check`，随后冻结并进行只读交付评审。
