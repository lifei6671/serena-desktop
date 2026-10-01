# 执行与验证

1. 核验 Host 三个 SHA256 与 feat/codebuddy dirty baseline。
2. 独立 policy、frozen authority 注入、tool snapshots、typed options 与 flush。
3. focused paths/commands/options/identity/flush 回归。
4. cargo test --manifest-path src-tauri/Cargo.toml codebuddy -- --test-threads=1
5. cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
6. cargo check --manifest-path src-tauri/Cargo.toml
7. 更新技术设计与 validation；不提交，不重启 Host，真实 E2E 留待 Host 新二进制重启后验证。
