# Phase 3 Closeout Baseline

记录时间：2026-09-22（Asia/Shanghai）

## Authority SHA256

```text
9456f57d28ece6f133e2a1104b18a187eb7c149cd44d5681058d7bdce0386316  .trellis/tasks/09-21-macos-p3-cli-process-tree/prd.md
daf0a27c2f750ca4e79695638e37219428999e553a37a1f4da31a289bbb2ba90  .trellis/tasks/09-21-macos-platform-porting/prd.md
3635ce242b716b4a17eb978aef2b38cb8bf2fa972a47000bdba71a8ec3e08002  docs/codex-agent-runtime.md
209c2d7161f4fab23c49122efaa470f0423e842afd8879a8ba102842878590d5  docs/macos-porting-checklist.md
```

四项均与 Host 提供值一致。

## Git 与已有工作区

```text
branch: dev/macos
HEAD: 3a76f444cc23abbecbf4eb283e06b9d9dfa8c303
upstream: origin/dev/macos
ahead: 9
```

进入任务前已存在以下未提交状态，必须保留：Phase 3/5/6 与平台父任务 PRD、`docs/codex-agent-runtime.md`、`docs/macos-porting-checklist.md` 的修改，以及 `09-22-chatgpt-oauth-discovery-compat` 未跟踪任务目录。本任务新增父 Phase 3 child link 与 `09-22-macos-p3-closeout`。

## 主机与工具链

```text
host arch: arm64
macOS: 26.5.2 (25F84)
rustc: 1.96.0 aarch64-apple-darwin
cargo: 1.96.0
node: v22.22.0
npm: 10.9.4
Xcode CLI: /Library/Developer/CommandLineTools
```

## Codex 当前证据

```text
path: /Users/lifeilin/.local/bin/codex
version: codex-cli 0.155.1
format: Mach-O 64-bit executable arm64
binary SHA-256: 8eaf1ad12fe6bf89b1710330f58900014322c7c5af677e43be116d8ac5fc0a9e
```

该 version/hash 仅为 identity/diagnostic evidence。准入由当前共享 schema 必要子集验证器决定。

## 初始限制

- 当前没有已构建 `target/release/bundle/.../*.app`，需要从当前代码生成本地 Gate artifact；这不是 Phase 5 分发/DMG 工作。
- Windows 实机 Job Object/Host Crash Gate 在本机不可执行。
- Finder/登录项及所有真人观察 Gate 初始均未通过。
