# CB5-002 Implementation Plan

1. Capture src/src-tauri hashes and task baseline; verify CB5-001 binary evidence.
2. Discover actual CodeBuddy ACP launch safely in temp cwd with bounded timeout; inspect install files only if needed.
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