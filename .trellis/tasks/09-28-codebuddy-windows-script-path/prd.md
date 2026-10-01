# CodeBuddy Windows Verbatim Script Path

## 问题

Windows discovery 对 npm wrapper 的 `node.exe` 与 CodeBuddy 主脚本执行 `fs::canonicalize`。Rust 在 Windows 返回 `\\?\C:\...` 形式；该脚本路径随后作为 argv 原样交给 Node。Host 已用 CodeBuddy 2.158.0 与 Node 24.19.0 证明：普通 `C:\...\codebuddy` 可完成 ACP initialize，而 verbatim 脚本路径稳定触发 `EISDIR: illegal operation on a directory, lstat 'C:'`，最终表现为 `CODEBUDDY_ACP_EOF`。

## 目标

- 保留 discovery 的 canonical file identity，不通过 shell/cmd，不回退到 PATH 猜测。
- 仅在 Windows 外部进程边界，把本地盘符 verbatim file path 安全投影为普通 Win32 path。
- 投影后重新 canonicalize，并确认仍指向同一文件 identity；缺失、目录、identity mismatch、非法 namespace 或 UNC 均 fail-closed。
- npm wrapper 的 `node.exe` 与主脚本保持绝对且可验证；当前不隐式增加 UNC file 支持。
- 增加确定性单元测试，并用本机 `node.exe`（若可用）和临时脚本证明真实 launcher 交给 Node 的主脚本 argv 不含 `\\?\`；测试不得依赖 CodeBuddy 安装。

## 验收标准

1. discovery 继续保存 canonical `node.exe` 和 canonical script identity。
2. `LaunchRequest` 对 npm `node.exe + script + --acp` 在创建进程前完成 script projection 与 identity revalidation。
3. ordinary local-drive script path 保持原值；verbatim local-drive script 投影为 ordinary path。
4. UNC、relative/非法 namespace、非文件以及 identity mismatch 被拒绝。
5. 相关 Rust tests、`cargo check --lib`、`cargo clippy --lib`、`git diff --check` 有明确结果。
6. 最终交付改动经过覆盖完整的只读审查，P0/P1 为零才可报告完成。

## 非目标

- 不修改 Provider health 或 diagnosticCode 的对外投影。
- 不修改角色路由、ACP 协议契约、Work/Agent 状态机。
- 不改变 Workspace canonical authority、Claim/Execution identity 或数据库结构。
- 不增加 shell/cmd、自动安装、PATH fallback、网络盘映射或 UNC 支持。
- 不提交、不推送。
