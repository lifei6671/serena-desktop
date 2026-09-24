# 2026-09-23 本机自动化证据

## PASS

| Gate | 命令 | 结果 |
|---|---|---|
| 前端 lint | `npm run lint` | PASS |
| 前端 build | `npm run build` | PASS |
| 前端 test | `npm test` | PASS，128/128 |
| 发布契约 | `node --test scripts/apply-release-version.test.mjs scripts/check-version.test.mjs scripts/ci-workflow.test.mjs scripts/release-workflow.test.mjs scripts/macos-bundle-config.test.mjs scripts/verify-installer.test.mjs scripts/verify-macos-release.test.mjs scripts/verify-uninstall-policy.test.mjs` | PASS，29/29 |
| Rust fmt | `cargo fmt --all -- --check`，目录 `src-tauri` | PASS |
| Rust check | `cargo check --locked`，目录 `src-tauri` | PASS |
| Rust clippy | `cargo clippy --locked --all-targets -- -D warnings`，目录 `src-tauri` | PASS |
| Rust test | `cargo test --locked`，目录 `src-tauri` | PASS，1083 passed，24 ignored |
| 版本一致性 | `node scripts/check-version.mjs` | PASS，Tauri/Cargo/npm 均为 1.1.0 |
| Windows 卸载静态策略 | `node scripts/verify-uninstall-policy.mjs` | PASS；真实卸载保留 Gate 未运行 |
| YAML 语法 | `ruby -ryaml -e 'YAML.load_file(".github/workflows/ci.yml"); YAML.load_file(".github/workflows/release.yml")'` | PASS |
| Diff 空白 | `git diff --check` | PASS |
| `.app` 静态签名 | `codesign --verify --deep --strict 'src-tauri/target/release/bundle/macos/Serena Desktop.app'` | PASS，ad-hoc + runtime |

`src-tauri/target/release/bundle/macos/Serena Desktop.app` 的主程序为 `arm64`，bundle identifier 为 `io.github.lifei6671.serena-desktop`，版本为 `1.1.0`，`LSMinimumSystemVersion` 为 `12.0`；`THIRD_PARTY_NOTICES` 在 `Contents/Resources/_up_/THIRD_PARTY_NOTICES/`。主程序 SHA-256 为 `6cdbaa82838b3ddcefd48b00cb6223f851d8f8796a4f83f63730e961b285eecc`。

## 未完成的实际产物 Gate

- `./node_modules/.bin/tauri build --ci -- --locked`：**FAIL**。`.app` 已构建并签名，DMG 阶段 `bundle_dmg.sh` 内 `hdiutil create` 返回“设备未配置”。独立命令 `hdiutil create -size 8m -fs HFS+ -volname SerenaDMGProbe /private/tmp/serena-dmg-probe.dmg` 同样失败；未产生探针镜像或 DMG。
- `node scripts/verify-macos-release.mjs`：返回 `{"ok":false,"diagnostics":[{"code":"DMG_COUNT_INVALID"}]}`。由于没有真实 DMG，挂载、目录和签名验证为 **NOT_RUN/UNAVAILABLE**；DMG filename/size/SHA-256 也无证据。
- GitHub hosted Windows/macOS CI 与 Release、Windows NSIS 实际构建、正式 GitHub Release 下载、Gatekeeper 放行及 Phase 6 真机矩阵：**NOT_RUN**。

本次按用户要求不提交 commit；Phase 5 Task 保持进行中。

## 2026-09-23 Host Review 后的有界收紧

- CI 从 matrix 改为 `quality-windows`（`windows-2022`/`pwsh`）与 `quality-macos`（`macos-14`/`bash`）两个独立 job；contract test 逐 job 检查完整前端、workflow 和 Rust Gate。macOS 独有 `uname -m == arm64`，Windows 独有卸载静态策略。
- Release 顶层权限改为 `contents: read`；只有 `publish` job 声明 `contents: write`。构建 job 继续只执行质量、版本、构建、验证和 artifact 上传。
- `npm test`：PASS，128/128；`node --test scripts/ci-workflow.test.mjs scripts/release-workflow.test.mjs scripts/macos-bundle-config.test.mjs scripts/verify-macos-release.test.mjs`：PASS，8/8；指定 Ruby workflow YAML 解析：PASS；`git diff --check`：PASS。
- 本轮只修改 workflow、对应 contract tests 与 Trellis 记录；Rust Gate 未重跑，不能将上一轮结果称为本轮重跑。DMG 挂载 verifier 和 GitHub hosted Gate 仍为 `NOT_RUN/UNAVAILABLE` 或 `NOT_RUN`，未提升为 PASS。

## 2026-09-23 verifier 挂载清理复核

- 普通 `hdiutil detach` 失败时，verifier 追加一次 `detach -force -quiet`；普通卸载成功不执行 force。attach 失败后仍尝试相同清理路径。
- 普通与 force 卸载都失败时返回固定 `DMG_DETACH_FAILED`；临时挂载目录删除失败返回固定 `DMG_MOUNT_DIRECTORY_CLEANUP_FAILED`，并保留先发生的验证错误。
- `node --test scripts/verify-macos-release.test.mjs scripts/macos-bundle-config.test.mjs`：PASS，9/9；`npm test`：PASS，132/132；`git diff --check`：PASS。
- 本轮只修改 verifier、对应测试和本证据文件。真实 DMG 因本机 `hdiutil create` 不可用仍为 `NOT_RUN/UNAVAILABLE`；未重跑 Rust Gate，未提交 commit。
