# Implementation and verification

1. 调查 generic continuation call path、CodeBuddy fresh prepare/client/private store/Product capability，形成 implementation matrix。
2. 实现 provider validation、typed `session/load` client boundary、continued prepare 和 execute branch；复用现有 post-prepare production lifecycle。
3. 增加 provider/client/store/execute/product 集成测试，覆盖 A-J 与负向 no-fallback/no-replay assertions。
4. 运行 scoped tests，再运行 CodeBuddy full relevant modules、`cargo fmt --check`、`cargo check`、`cargo clippy`；如 frontend catalog fixtures 变化则运行对应 tests。Linux 只允许项目 Docker Desktop runner，无 runner则 UNAVAILABLE，禁止 WSL。
5. 记录 authority hashes、request sequence、continuation matrix、验证结果；确认无 schema/migration 和 no CB8-004。
6. 冻结本卡全部 tracked/untracked 内容及 SHA256，由独立只读 `FULL_SCOPE` reviewer 检查完整目标；修复全部 P0/P1/P2 后重验重审。
