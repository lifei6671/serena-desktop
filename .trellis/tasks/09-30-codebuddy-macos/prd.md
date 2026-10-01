# CodeBuddy macOS 接入

用户已明确授权实现与本机验证，不提交 Git，不改用户业务改动。

## 验收
- macOS arm64 原生 Mach-O 无 shell discovery，覆盖 PATH、HOME/.local/bin、Homebrew 和 /usr/local/bin；Windows 不回归。
- 产品 catalog 自然展示、available admission，默认 disabled。
- configuration catalog、Fresh、Continue、Activity、Cancel 使用受管 ACP；共享原业务 pipeline。
- setsid、PID=PGID=SID、Darwin start token，只有 group-empty 才提供 termination evidence。
- Runtime provider=codebuddy，macos/macOS process-group 身份，不伪造 Windows 字段；旧 Runtime 独立证明；generic terminal/Claim authority 不变。
- focused tests 不依赖已安装 CLI；可显式运行本机 smoke。
- cargo fmt/check、CodeBuddy tests，并如实区分真实 E2E 与 fixture。
