# 修复 macOS CLI 状态检测

## Goal

让状态页在 macOS 使用真实 Codex 与 CodeGraph CLI 检测逻辑，不再返回平台不支持或遗漏已安装命令。

## Requirements

- macOS 状态页的 Codex CLI 检测必须复用正式 Agent discovery/compatibility authority，不能再返回固定的“当前平台不支持”。
- 状态页必须显示实际兼容 Codex executable 的 `--version` 输出；失败时保留 discovery 的稳定诊断。
- CodeGraph CLI 检测必须适配 Finder 精简 PATH，并检查当前用户的 `~/.local/bin/codegraph`。
- Windows 现有 Codex 与 CodeGraph 检测行为保持不变。
- 不增加配置项、第三方依赖或额外后台任务。

## Acceptance Criteria

- [x] macOS Codex 状态探针通过正式 discovery 后返回真实版本，不再走非 Windows 固定拒绝分支。
- [x] Finder 风格 PATH 不含 `~/.local/bin` 时，CodeGraph 仍能发现其中的可执行文件并读取版本。
- [x] 缺失或执行失败仍投影为现有不可用状态，不伪造版本。
- [x] 新增测试完成 RED/GREEN，相关前端/Rust 回归与格式检查通过。
- [x] 实际构建的 macOS `.app` 状态页显示本机 Codex 与 CodeGraph 版本。

## Notes

- 已复现：Agent 页面为 `Local Runner Active`，但状态页 Codex 行显示固定平台不支持，根因位于 `commands.rs::get_codex_version` 的 `cfg(not(windows))` 分支。
- 已复现：本机 CodeGraph 位于 `~/.local/bin/codegraph`，现有检测只依赖 GUI 进程 PATH。
- TDD RED：新增测试首先因缺少 `codegraph_candidate` 与 `codex_version_with` 无法编译；实现后两项定向测试均通过。
- 回归验证：`npm run lint`、`npm run build`、`npm test`、`cargo fmt -- --check`、`git diff --check` 均通过；完整 Rust 测试为 `1073 passed; 0 failed; 21 ignored`。
- 产物验证：`npm run tauri build` 成功；重新启动构建出的 `.app` 后，状态页显示 Codex `codex-cli 0.155.1`、CodeGraph `1.6.0`。
