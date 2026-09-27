# CB2-004 Frontend Config Model Synchronization

Implement only CB2-004 under explicit user authorization, including task planning and execution. Preserve every baseline change. No CB2-005, UI, Rust edits, Remote MCP changes, commits, or cleanup.

## Acceptance
- ManagerConfig.agentProviders matches current Rust camelCase wire.
- Open provider map and string/null route values retain unknown valid IDs; five fixed role keys.
- Initial placeholder and all frontend config fixtures synchronized; Rust owns migration defaults.
- Ordinary full/partial saves preserve Provider policy; dedicated CB2-003 IPC remains mutation authority.
- Focused controller/config round-trip, partial-save, defaults/fixture tests and npm build pass.
- Report commands, counts, scope diff and independent review without claiming Host Gate.

## Sources
- docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md SHA256 7e74f8ac766daf1c0c0e5c3cdaa68fee1db666ff8085084b18c5a24ea91e02a1 (verified)
- docs/technical-design-multi-agent-provider-codebuddy-v0.1.md SHA256 f9f416b22a74b86f842349824009ef4a32da9e21054fb023387d0cde5d77a32b (verified)
