# CB5-003 Fresh Session / Prompt / Activity Contract

## Goal

在临时 Workspace 中用真实 CodeBuddy Code ACP server 冻结 Fresh Execution 所需的 session/new、cwd、sessionId、session/prompt、session/update 和 prompt terminal response。证明 canExecute 的真实 wire，不修改任何用户项目，不涉及 Continue。

## Preconditions

- CB5-002 Host Gate PASS。
- CodeBuddy Code CLI 当前可用：`codebuddy --version = 2.158.0`（仅诊断）。
- ACP initialize negotiated `protocolVersion = 1`；这是协议 compatibility authority。
- 官方 Rust `agent-client-protocol = 2.2.0` 可消费 harness-owned external stdin/stdout。

## Scope

- task-local/temp Rust probe harness。
- 每个真实场景创建独立临时 Workspace。
- 允许 CodeBuddy 在临时 Workspace 内读/写，仅限测试约定文件。
- 禁止访问或修改 Serena Desktop repo 作为 Prompt Workspace。
- 禁止 Continue/session recovery、Cancel/permission matrix（CB5-004）、生产 Provider、Runtime、MCP、Git commit。

## Fresh session contract to prove

1. initialize(protocolVersion=1)
2. session/new with exact temporary cwd
3. capture exact returned sessionId and any session metadata
4. session/prompt with simple text content
5. capture ordered session/update notifications
6. capture prompt terminal response and exact stop reason/result fields
7. close streams / terminate / wait child, bounded cleanup

## Test scenarios

### A. Read-only prompt
- Temp workspace contains `input.txt` with deterministic content.
- Prompt asks CodeBuddy to read `input.txt` and return a deterministic marker without modifying files.
- Before/after full temp file manifest + SHA256 must match.
- Capture sessionId, prompt request identity, updates, terminal response.

### B. Isolated write prompt
- New independent temp workspace contains only seed files.
- Launch may use `--permission-mode auto` because writes are strictly isolated to temp scope.
- Prompt asks CodeBuddy to create exactly `output.txt` with deterministic content and then finish.
- Verify only expected file changes occurred; hash/content must match stated result.
- Delete whole temp workspace after evidence capture.

### C. Terminal/error contract
- Use a safe deterministic prompt/fixture that produces a terminal ACP prompt response without user-project side effects.
- Record terminal stop reason type/value exactly.
- If the real provider exposes an error terminal path that can be triggered safely without destructive actions, freeze it; otherwise mark error terminal NOT_PROVEN rather than inventing one.

### D. Activity ordering
- Record all `session/update` notifications in wire order with sequence numbers.
- Identify tool/activity/status update shapes actually emitted.
- Do not map them to Serena product activity yet; this card only freezes raw ACP contract.

## conversationRequestId decision

- Do not assume `codebuddy.ai/conversationRequestId` is required.
- Probe at least one prompt without it.
- Probe one independent prompt with a Serena-generated UUIDv7 (exactly 32 lowercase hexadecimal characters, no hyphens) in `_meta.codebuddy.ai/conversationRequestId` using the official SDK/raw extension mechanism supported by the protocol model.
- Determine: required / optional / ignored / echoed / useful for correlation.
- If adopted later, it must be Serena-generated and provider-private; this card does not modify product state.

## Identity requirements

- Exact sessionId must be captured from `session/new` response.
- Exact cwd sent and any cwd/session evidence returned must be recorded.
- Prompt RPC id and optional conversationRequestId must be separately identifiable.
- No identity may be synthesized from chat text or CodeBuddy UI labels.

## Evidence

- `fresh-session.jsonl`: sanitized ordered wire for initialize/session/new/prompt/update/terminal.
- `read-only-result.json`: cwd, sessionId, before/after manifest, terminal fields.
- `isolated-write-result.json`: cwd, sessionId, exact file delta, hash/content proof.
- `activity-order.json`: ordered update type/shape summary.
- `conversation-request-id.json`: A/B result and decision.
- `process-evidence.json`: executable/argv/cwd/PID/cleanup.
- `verification.md`: PASS/PARTIAL/BLOCKED and canExecute decision.

## Acceptance

- Real CodeBuddy `session/new` succeeds with temp cwd.
- Exact sessionId is available.
- Real `session/prompt` succeeds and reaches terminal response.
- Read-only prompt leaves workspace unchanged.
- Isolated write prompt changes only expected temp file.
- At least one real `session/update` stream/order is captured, or explicit evidence shows no updates for the chosen prompt.
- conversationRequestId requirement is decided from first-hand evidence.
- Process cleanup bounded; no product source changes.
- Only after these facts are proven may later production work consider `canExecute=true`.