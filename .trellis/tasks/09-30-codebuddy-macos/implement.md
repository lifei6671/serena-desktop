# 实施与验证

- [x] 原生 discovery 与 focused fixtures；catalog 平台注册。
- [x] macOS launcher/runtime、durable store 与 recovery 证据；失败 ownership 保留。
- [x] 平台共用 ACP lifecycle 和 Provider 能力；Windows launcher 不改。
- [x] 修改文件 rustfmt check、cargo check、CodeBuddy 120 项、Codex macOS 45 项、npm 176 项和 lint。
- [x] 独立只读评审及发现问题的修复；无 Git 提交。
- [x] 真实 CodeBuddy 受管 initialize / group cleanup smoke。
- [ ] 真实 CodeBuddy catalog / 模型 E2E：session/new 返回 -32603 Internal error，尚未通过。不把 native fixture 当作真实模型验收。

详见 `docs/codebuddy-macos-validation.md`。任务保留 in_progress，真实 catalog 阻塞尚未解除；不执行 archive/auto-commit。
