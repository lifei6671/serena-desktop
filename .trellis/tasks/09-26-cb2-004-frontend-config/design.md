# Design

Use AgentProviderSettings with Record<string, AgentProviderPolicy> and fixed role string|null mapping. Current Rust serializes all five roles and rejects unknown role keys, while allowing unknown provider IDs. Initial frontend settings are a pre-hydration placeholder matching Rust defaults, never a migration overlay.

Preserve incoming snapshots intact. General saves retain policy from server state and synchronize policy from returned authoritative snapshots into drafts, while preserving unrelated unsaved fields. Avoid introducing a second policy mutation API; Rust save_config already replaces incoming agent_providers with stored policy under its lock. Inspect existing callers and fixtures before selecting minimal type restriction for general partial saves. No policy management UI or unused IPC wrappers.
