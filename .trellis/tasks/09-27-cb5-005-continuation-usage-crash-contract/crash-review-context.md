# Stage C review context

Authority：本轮用户Stage C A–F要求、仓库中文注释规范、technical-design-multi-agent-provider-codebuddy-v0.1.md §23及Stage A/B冻结事实。Tier 3 protocol/persistence审查，仅task-local，DCR不是migration授权。

Target：crash-review-target.json，13文件完整scope；crash-baseline.json为747项起点，crash-scope.json记录旧文件及11份Stage A/B raw evidence hash。排除既有.gitignore/其他task、所有生产代码、主Cargo、DB migration。生成物仅task-local dev/test binary，真实mode未运行。

requirement → implementation → gate：

- 固定mode/sentinel/write-ahead → main、crash::reserve、crash_runtime → sentinel测试/fake在收到Prompt时检查identity文件。
- correlated before窗口/terminal优先 → crash::trigger与single-thread select → unit ordering、terminal-first、timeout fake。
- exact afterterminal/独立load → run_runtime/scenario → fake both windows、wrong terminal/load失败无fallback。
- history identity/分组去重/hash/privacy → crash::project → grouping/unit+fake wrong session/request/message；future terminal仅Material Difference。
- cleanup/manifest/Claim隔离 → 既有transport/evidence与grade → 新cleanup/manifest gate及旧34项边界回归。
- private schema → DCR → readonly store.rs/schema_v9/v12核验，review FK/ownership/OCC/retention/null历史/public Port边界。

验证结果见crash-verification.md；review必须只读，不能运行真实CLI或Host modes。预期输出matching aggregate、全scope coverage、P0/P1/P2 findings与PASSED/BLOCKED。review不替代Host real gate；Stage C仍NOT_RUN、任务仍in_progress。
