# CB3-003 Start DTO Compatibility Parser
Fresh execution, task creation and implementation authorized by user. Preserve all pre-existing changes.
Only parser/schema/contracts; no CB3-004 routing, dispatch, policy, Provider mutation, runtime, UI or commits.
Start full pair uses AgentTaskRole and validated ProviderId and explicit typed intent. Absent pair means LegacyGeneral, without resolving policy. Half pairs return INVALID_PARAMS. Non-start actions reject any routing fields. Unknown fields and invalid values reject. Keep legacy context/workspace validation.
JSON schema must independently reject half pairs and agree with serde matrix. Lock new execute descriptor hash and unchanged CB3-002 query hash; object-key ordering remains stable.
Run focused Rust tests, registry/schema/hash tests, cargo check, fmt, git diff --check and clippy. Known usage_tests await_holding_lock must not be fixed. Windows validation only; Linux requires project Docker runner, never WSL. Host gate remains separate.
