# CB5-002 Managed Pipe → Official ACP SDK Initialize Probe

## Goal

用真实 CodeBuddy CN 进程验证 SerenaDesktop-owned stdin/stdout pipes 能否交给官方 Rust `agent-client-protocol` SDK 完成 ACP initialize，并冻结 `protocolVersion` / capability 证据。产品版本与 binary hash 只做诊断，不参与兼容 admission。

## Compatibility rule

- 唯一“协议是否可连接”门禁：ACP `initialize` 成功并且 negotiated `protocolVersion` 与 SerenaDesktop 支持的 ACP protocolVersion 一致。
- CodeBuddy ProductVersion/FileVersion/commit/SHA256 不参与 allow/deny。
- capability/method 不参与“能否建立协议连接”的判定。
- Execute / Continue / Cancel / Recover / Activity / Usage 等能力分别由 initialize capability 与后续真实 method probe 决定；缺失只令对应 capability=false。
- initialize malformed / EOF / timeout / protocolVersion mismatch 才属于本卡的 protocol incompatibility/failure。

## Real binary

复用 CB5-001 已 Host-verified canonical entry：
- EXE: `%LOCALAPPDATA%\Programs\CodeBuddy CN\CodeBuddy CN.exe`
- CLI JS: `%LOCALAPPDATA%\Programs\CodeBuddy CN\resources\app\out\cli.js`
- env: `ELECTRON_RUN_AS_NODE=1`, remove `VSCODE_DEV`
- discovery command: `buddycn`（不是 `codebuddy`）
- product/hash metadata仅作为 evidence identity。

## Process / SDK ownership

- Probe harness 自己创建 CodeBuddy process 并持有 stdin/stdout/stderr/Child handle。
- SDK 不允许调用 spawn / Command 来成为 Process Authority。
- 将已经获得的 external stdin/stdout streams 适配给官方 SDK。
- 本卡只证明 external-managed pipe integration；Windows Job-at-creation 仍由 Phase 6 production runtime 实现，本卡不得为了 SDK 放弃该架构。

## Exact launch contract to discover

- 不假设普通 `--help` 会列出 ACP flag；Host preflight 没有看到 `--acp`/`--permission-mode`。
- Probe 必须找到真实可用 ACP invocation；候选包括真实 EXE + CLI JS + hidden `--acp` / `--permission-mode auto`。
- 所有真实进程必须使用临时 cwd、no workspace write prompt、本卡只做到 initialize。
- 如真实安装根本不暴露 ACP entry，必须以 BLOCKED evidence 结束，不猜协议。

## SDK

- 首选官方 Rust crate `agent-client-protocol`。
- 使用 task-local / temp Rust probe crate，不修改产品 Cargo.toml/Cargo.lock。
- 记录 crate version、feature flags、source/API path。
- 若 SDK 不能接受 external streams，记录编译/API evidence，并冻结 minimal NDJSON fallback 所需 initialize framing；不得让 SDK spawn Agent。

## Tests / probes

1. initialize success against real CodeBuddy, capture exact request/response JSONL and protocolVersion.
2. external-managed stream proof: process handle belongs to harness, SDK only sees streams.
3. EOF before/while initialize: deterministic error, no hang.
4. invalid protocolVersion: use task-local fake ACP peer or controlled response; compatibility gate rejects mismatched protocol.
5. capability absence/matrix: does NOT fail protocol connection; only records false/unknown capability.
6. clean shutdown: close SDK/streams, terminate/wait child within timeout; no orphan process.

## Evidence

- `initialize.jsonl`: sanitized exact wire ordering/fields.
- `sdk-details.json`: crate version/API/external-I/O proof.
- `process-evidence.json`: argv/env/cwd/pid/exit/cleanup facts.
- `verification.md`: PASS/PARTIAL/BLOCKED, protocolVersion, capabilities, limitations.
- binary identity reference back to CB5-001; do not copy credentials/environment secrets.

## Acceptance

- Official SDK + externally managed streams + real CodeBuddy initialize PASS; OR, if SDK API cannot do it, evidence proves why and minimal NDJSON fallback method/framing is frozen.
- Actual negotiated protocolVersion is known.
- Product version/hash never appears in compatibility decision.
- Capability deficits do not turn into protocol incompatibility.
- Product source files unchanged.
- No session/new, prompt, workspace write, install/upgrade/login, or Phase 6 production code.