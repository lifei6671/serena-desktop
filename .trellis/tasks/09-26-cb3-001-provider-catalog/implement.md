# Implementation
1. 读取必读材料/代码/规则，基线见 baseline-status/head/hashes 与 baseline 源文件副本。
2. Trellis implement 子 Agent 负责 Product service/DTO 与测试/fixtures；必要时 Registry read methods。不修改其他已有文件。
3. Windows 原生 focused Rust tests、cargo check --locked、cargo fmt --all -- --check、cargo clippy --locked --all-targets -- -D warnings；根目录 git diff --check。记录 cwd/command/exit/count/log。Linux NOT_RUN，禁止 WSL。Clippy 已知 blocker 只记录。
4. 冻结实现 hash 与 evidence，由独立 check 子 Agent 只读审查本卡增量。主会话核对基线保留，不将子 Agent review 视为 Host Gate。
5. 保留任务供 Host review，不 commit/归档/进入下一卡。无新通用规范时记录无需 spec update。
