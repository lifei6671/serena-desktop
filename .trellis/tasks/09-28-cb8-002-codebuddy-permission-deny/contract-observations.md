# Contract observations

Frozen acceptance.json and both design/task documents match user-provided SHA256 at task start. CB5-004 permission.jsonl sequences 90–96 establish: exact conversation tool_call first, request id 0 with same session/toolCall, selected advertised reject response, no cancel notification, independent exact prompt cancelled response. This evidence is read-only; no real probe rerun.

The archived harness sanitizer at harness/src/bin/host_cancel_permission.rs maps each option to only optionId/kind (lines 248–254). Missing display name in sanitized evidence is intentional redaction, not a production wire schema difference. Pinned dependency is agent-client-protocol =2.2.0, default-features=false. Implementer confirmed v1 RequestPermissionRequest/Response, PermissionOptionKind::RejectOnce and SelectedPermissionOutcome match the contract.

Available project spec layers: frontend only (get_context.py --mode packages). No backend layer or Docker runner discovered in repository paths/scripts/CI. Native Windows gates apply; Linux cannot be asserted. WSL was not invoked.

CodeGraph workspace catalog tool was blocked by approval policy=never, and no local codegraph command was found. Direct source reads are used as fallback. No CodeGraph initialization or index changes.
