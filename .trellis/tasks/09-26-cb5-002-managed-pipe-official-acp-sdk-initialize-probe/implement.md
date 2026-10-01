# CB5-002 Implementation Plan

1. Capture src/src-tauri hashes and task baseline; verify CB5-001 binary evidence.
2. Preserve historical IDE failure evidence; verify installed standalone CLI shim/package identity and use canonical Node + bin/codebuddy --acp in temporary cwd.
3. Create task-local Rust probe crate using official `agent-client-protocol`; do not touch product Cargo files.
4. Implement harness-owned Child + piped stdio + external-stream SDK adapter; tee sanitized initialize wire.
5. Run real initialize and record protocolVersion/capabilities.
6. Implement fake-peer tests for EOF, protocol mismatch and missing optional capability.
7. Verify clean shutdown/process wait and no orphan.
8. If official SDK external-stream path fails, prove whether it is API limitation; only then add task-local NDJSON initialize fallback proof.
9. Produce initialize.jsonl, sdk-details.json, process-evidence.json, verification.md and hashes.
10. Recheck product files unchanged and perform independent review. Stop before CB5-003.

## Rollback

Any inability to prove real ACP initialize => CB5-002 BLOCKED/PARTIAL. Do not compensate by trusting product version, binary hash, help text, or guessed capabilities.
## Resume verification

Rerun the existing 11 Rust tests from the normal E: workspace, Python 2 cleanup tests, fmt, actual standalone CLI initialize and bounded child cleanup. Preserve raw capability extensions/auth method public fields. Rebuild current evidence/hashes; preserve earlier real-default/real-auto and historical reviews. Main session performs product baseline verification and independent delivery review before Host Gate.
