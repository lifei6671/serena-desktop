# CodeBuddy 会话权限模式改造

用户于 2026-09-30 明确授权按技术设计 §18 实现及验证；不提交 Git、不覆盖本任务既有 macOS 适配、不改用户配置或 CLI 安装。

## 设计与范围

- Fresh new / Continue exact source load 返回后先完成 typed catalog validate/replay，再仅对 advertise auto 的本次 Session 默认填 DesiredConfiguration.mode。
- 共用 configure 的 typed SetSessionModeRequest；ACK → replay（含 typed current_mode_update）→ confirm → acceptance / prompt。
- 无 auto 保持原模式；error/timeout/malformed/配置撤销拒绝派发。Continue lineage/source identity 不变。
- permission fallback 保留 exact identity + RejectOnce；公共 ExecutionProfile、model/reasoning、role defaults 与 catalog 查询不变。
- 仅修改共用 fresh/continued、直接相关 native fixtures/tests 与技术/验证文档；Windows/macOS 不分叉。

## 实施与验证

- [x] 当前四个用户提供 SHA256 全部匹配；已记录并保留开始时 29 项未提交 macOS 文件。
- [x] fresh/continued 接入共享默认选择；强化 replay/confirm 的模式确认。
- [x] 原生 Provider 测试：advertised/absent/config-only、ACK gate、set_mode 故障、replay rollback、Continue、auto 后 RejectOnce、EffectiveExecutionProfile；4 matrix / 28 场景，另1 typed ACK 回归。
- [x] cargo fmt --check、cargo check、CodeBuddy focused tests 125 PASS / 0 FAIL / 2 ignored（NOT_RUN）；既有 macOS Runtime/Recovery 7 项全部 PASS。
- [x] SDK 宽松接受数组/null ACK 的真实回归已在 exact pending 输入 guard 修复；独立只读评审无问题。
- [x] 文档记录 PASS/FAIL/NOT_RUN 和真实 Host 验收边界，见 docs/codebuddy-permission-mode-validation.md。

本轮用户要求不提交；不执行 commit、archive 或可能 auto-commit 的 journal 脚本。
