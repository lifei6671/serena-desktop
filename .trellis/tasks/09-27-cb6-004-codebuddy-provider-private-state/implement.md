# 执行与验证

1. baseline feat/codebuddy b40d67b392669a087dcd4d6d4e59065c233f292b，git status clean。
2. 新建 v13/domain/SQL/tests；同步现有 migration regression 的 current-version 断言，保留 frozen 输入。
3. Native Windows cargo fmt/check，focused store/migration tests，clippy --lib --tests -D warnings。现有 usage_tests await_holding_lock 需实际区分；不修无关基线。
4. git diff --check/scope，schema_v12 hash 不变；冻结 executable target 与 review context。
5. 独立隔离只读 full review，修 P0/P1/P2 后重新验证、freeze/review。

Linux：无 project Docker runner，不做 Linux 验证，不使用 WSL。Windows 工具用现存显式路径；本机 Python312 与 cargo.exe 已确认可用。
CodeGraph connector discovery 被 approval policy=never 拒绝；改用本地源码读取，不初始化或绕过远程连接器。
