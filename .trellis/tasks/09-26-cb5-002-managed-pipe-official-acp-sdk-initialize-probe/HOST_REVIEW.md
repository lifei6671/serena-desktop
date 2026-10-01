# CB5-002 Host Review

Status: ACCEPTED.

## Final host evidence

- Real CodeBuddy Code CLI installed:
  - `codebuddy --version` => `2.158.0`
  - package `@tencent-ai/codebuddy-code@2.158.0`
  - `codebuddy --help` advertises `--acp`, stdio NDJSON, `--acp-transport`, and `--permission-mode`.
- Provider target boundary is now explicit:
  - `codebuddy` / CodeBuddy Code CLI is the public ACP server.
  - `buddycn` / CodeBuddy CN IDE is diagnostic-only and MUST NOT be registered as the ACP Provider executable.
- Canonical probe process:
  - executable: `C:\nvm4w\nodejs\node.exe`
  - CLI entry: `C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy`
  - args: `--acp`
- Official Rust SDK:
  - `agent-client-protocol = 2.2.0`
  - external `ByteStreams` attached to harness-owned child stdin/stdout.
  - SDK does not own/spawn the CodeBuddy child.
- Host reran task-local Rust tests: 11/11 PASS.
- Host reran Python cleanup failure tests: 2/2 PASS.
- Host reran task-local rustfmt: PASS.
- Host reran real CLI probe:
  - initialize succeeded.
  - negotiated `protocolVersion` is JSON number `1`.
  - `protocolCompatible=true`.
  - raw capabilities include image, embeddedContext, HTTP/SSE MCP, loadSession, delegateToolsSupport, multitaskSupport; mainAgentSupport=false.
  - authMethods public id/name recorded only; no authentication call/token capture.
- Protocol gate:
  `initializeSucceeded && negotiatedProtocolVersion == 1`.
  Product version, hashes, capabilities, and child exit code do not participate.
- Missing optional capability fixture remains protocol-compatible.
- Cleanup:
  - direct child reaped and stderr joined.
  - current real probe cleanup succeeded with no recorded errors.
  - Windows Job-at-creation is NOT proven in this card and remains Phase 6 work.
- Product source baseline: 315 files, 0 changes during CB5-002.
- Whole-tree `git diff --check`: PASS after Host removed one trailing-whitespace defect in the updated task-breakdown document.

## Current design hashes

- Technical design: `200d348ee639b851db708a4392fe19779a4f0d749ca217f9d4917662aab8abad`
- Task breakdown: `293b58c791f0b1048e1f8134e1c4da8e95bd1bbbb6c68dd0cb853f328e8fcd4b`

CB5-002 passes. CB5-003 may proceed.
