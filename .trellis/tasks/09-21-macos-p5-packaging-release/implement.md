# Phase 5 CI 与 Release 执行

1. 更新 macOS overlay 与最小 Info.plist，保留 Windows NSIS authority 和第三方声明资源。
2. 将 CI 扩展到 Windows x64 与 macOS arm64 完整质量 Gate。
3. 将 Release 拆为两平台构建及统一 publish；每个平台仅上传 verifier 通过的产物。
4. 实现 macOS verifier 与可跨平台运行的纯逻辑测试，更新 workflow/bundle contract tests 和安装文档。
5. 运行用户列出的本机 Gate；构建真实 app+DMG 并运行 verifier。记录所有未在本机执行的远端或人工 Gate。
6. 检查 diff、更新清单中已经验证的自动化项；本次不提交 commit，也不将 Phase 5/6 标成全面完成。
