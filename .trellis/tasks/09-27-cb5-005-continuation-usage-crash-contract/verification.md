# Stage A verification

cwd=`E:\wx_lifeilin\github.com\lifei6671\serena-desktop`。Windows host fake evidence，非 Linux/Provider evidence。

命令完整参数见 README；Cargo 使用 task-local cache 和 `--offline --locked`。

| 检查 | 状态 | 证据 |
|---|---|---|
| cargo test --offline --locked --manifest-path TASK/harness/Cargo.toml | PASS exit 0 | 16 passed / 0 failed / 0 ignored；2.28s |
| cargo build --offline --locked --manifest-path TASK/harness/Cargo.toml | PASS exit 0 | cb5-005-continuation.exe（最终修复后重新构建） |
| cargo fmt --manifest-path TASK/harness/Cargo.toml -- --check | PASS exit 0 | 无差异 |
| git diff --check | PASS exit 0 | 无空白错误 |
| task.py current --source | PASS exit 0 | 本任务 in_progress/session pointer |
| resume / load 真实 CLI | NOT_RUN | 父 Host 专属；没有真实 sentinel/result |

过程中两个编译错误均已修复：SDK Error 不支持 From<io::Error>，改为静态 typed internal_error；静态 cleanup error 转换需显式 Vec<String>。最终同一来源构建与测试已通过。

测试涵盖官方两方法实际 wire、wrong session/cwd、no fallback/failed recovery no P2、early notification、phase 与当前 conversation 归因、token hash/match 脱敏、双 runtime 同 S1/cwd 和 PARTIAL、sentinel/no output override、完整隐藏 manifest/junction fail closed、timeout/frame bounds、cleanup kill-error。

测试 fixture 可保留公开常量 token 在源码中；随机 P1 memory token 从不持久化。测试的安全方法日志位于测试临时目录，已由测试回收。

独立review Round 1发现 P1：invalid new session ID 在拒绝发送prompt后仍进入state。已在进入state之前校验，新增 invalid_new_identity_never_persisted 回归；16/16通过，最终binary重新构建，fmt/diff check通过。等待相同独立Reviewer复审。
