# 侧栏品牌与标签版本

## Goal

将侧栏品牌改为 SerenaDesktop 并在 GitHub 标签构建中注入应用版本。

## Requirements

- 将侧栏 Logo 区现有的两行 “Serena / Desktop” 改为单行品牌名 `SerenaDesktop`，并在其后显示版本号。
- 本地开发和普通构建显示当前产品版本；GitHub 的标签发布构建显示触发构建的标签版本。
- GitHub 发布工作流以 `vX.Y.Z` 标签为唯一版本来源，在编译前同步 Tauri、Cargo、Cargo.lock 与 npm 的产品版本，使安装包和运行时版本与标签一致。
- 保持现有标签触发、质量检查、NSIS 打包与发布流程；不新增依赖，不提交 CI 构建时生成的版本改写。

## Acceptance Criteria

- [ ] 侧栏品牌显示 `SerenaDesktop vX.Y.Z`，不再显示拆分的 `Serena` / `Desktop`。
- [ ] 常规构建会把仓库产品版本编入前端；标签发布构建会把 `github.ref_name` 编入前端。
- [ ] 发布工作流在所有构建、检查与打包步骤前验证并将 `vX.Y.Z` 同步至 Tauri、Cargo、Cargo.lock 与 npm 产品版本，且 `cargo --locked` 可继续使用。
- [ ] 无效标签会失败，且版本同步脚本和发布工作流契约都有自动化测试。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
