# Phase 3 Closeout Design

## 边界

本任务是验证与证据收口任务，不重新设计 Runtime、Provider、StateStore、Claim 或进程所有权。验证顺序从低风险的只读/隔离 Gate 开始，再进入真实 `.app` 产品路径；只有证据证明现有实现缺陷时才修改代码。

明确排除 DMG、GitHub Actions、通知/LAN/UI 矩阵、签名发布与 Phase 4/5 实现。

## Authority 与兼容契约

当前 Codex 准入链固定为：

```text
macOS ARM64 host preflight
  -> candidate regular/execute/canonical/Mach-O/ARM64 preflight
  -> managed `codex --version`
  -> managed `codex app-server generate-json-schema --experimental --out <temp>`
  -> shared SerenaDesktop required-schema-subset validator
  -> formal Runtime initialize/Provider lifecycle only for product execution
```

version、binary SHA-256、完整 schema SHA-256 在每次 Gate 中记录，但不得参与精确 allowlist 判定。静态 schema Gate 不产生业务 RPC；真实 Agent 产品 Gate 仍需经过正式 Runtime initialize、JSONL、Provider、termination evidence 与 Claim 契约。

## 隔离与清理

- 普通路径使用任务专属临时目录；空格/中文路径使用同一隔离根下的测试 Workspace。
- 外置卷路径使用临时 sparse/disk image 创建 APFS volume，只将测试 Workspace、临时 runtime 或 `TMPDIR` 放入该卷；记录 device 与 mount point 后再执行测试。
- 无论 Gate 成败都先停止产品拥有的 Runtime，再卸载临时卷、删除镜像与任务临时目录。
- 残留检查使用 PID/PPID/PGID/SID、应用记录和监听端口；不以进程名粗暴 kill 用户进程。

## 产品 Gate

构建当前代码的 arm64 `.app`，通过 Finder/LaunchServices 打开。产品 UI 用于：

1. 验证 Codex discovery 命中路径与版本；
2. 在隔离 Workspace 发起最小 Agent start 并等待正常完成；
3. 仅在能构造明确、可停止的任务时执行 cancel；完成后用同一 lineage 验证 continue；
4. 执行 Serena install/detect/start/stop/restart，并验证 Workspace capability；
5. 通过标准退出路径触发统一 shutdown，再核验进程树和监听端口。

若 UI 不暴露足够可观察证据，允许以现有 ignored live tests 补充底层真实 evidence，但不能把它们冒充 Finder 产品 Gate。

## 状态判定

- `PASS`：实际执行并取得当前契约要求的完整证据。
- `FAIL`：实际执行且行为违反契约。
- `NOT_RUN`：本轮未执行，包括需要真人观察而自动化不可代替的项目。
- `UNAVAILABLE`：当前机器/平台客观无法执行，例如 Windows Job Object 实机 Gate。

父 Phase 3 仅在 PRD 所有 acceptance 与 checklist 退出条件均为 `PASS` 时归档。登录项/Finder真人观察等仍未完成时，父任务保持 `planning`。

## 回滚

代码修复若出现回归，只撤销本任务自身改动；不覆盖进入任务前已有的工作区变更。临时资源清理独立于代码回滚，任何失败路径都必须执行。
