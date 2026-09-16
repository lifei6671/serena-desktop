# Implementation Plan — P2A2-005 Local Tauri IPC Workspace Authority Foundation / Audit

1. Record the Local Tauri IPC surface inventory and atomic cutover owner map.
2. Verify the existing Agent Start TypeScript DTO serializes `workspaceId` and
   that selection only supplies the UI's default value.
3. Add only contract/audit tests that preserve current public behavior.
4. Verify no Execution snapshot, Claim, dispatch, or backend authority route
   changes occurred; record focused test evidence and review the final scope.
