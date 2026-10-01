# Implementation and verification

1. 调查 recovery、runtime/client、private store、runtime attempts、startup registry 与 generic finalization，建立 R1/R2 authority map。
2. 实现 post-R1-proof inspection helper及最窄 store transaction/query；保持 staged terminal fast path，不新增 schema/migration。
3. 增加 fake ACP/native Job/SQLite tests，覆盖 crash A-D、错误矩阵、side-effect manifest、dual evidence、identity isolation、disabled startup 与 restart twice。
4. 运行 focused recovery/store/provider/Product/TaskManager tests和 CodeBuddy/full relevant module tests；运行 `cargo fmt --check`、`cargo check`、`cargo clippy --lib`。Linux 仅允许项目 Docker Desktop runner；若不存在则 UNAVAILABLE，禁止 WSL。
5. 生成 recovery matrix、startup idempotency matrix、request-sequence、authority hash/schema-zero-diff和验证证据。
6. 冻结完整 delivery target（tracked/untracked + SHA256），交未参与实现的独立只读 `FULL_SCOPE` reviewer；修复全部 P0/P1/P2 后重验重审，最多三轮。
