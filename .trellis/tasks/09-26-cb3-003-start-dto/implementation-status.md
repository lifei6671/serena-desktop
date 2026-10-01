# CB3-003 implementation status

Scope: DTO/parser/schema and directly related contract tests only. Existing dirty work retained from task baseline.

Changed: src-tauri/src/mcp/orchestration/dto.rs; src-tauri/src/mcp/provider_query_tests.rs.

- Start pair is parsed with existing AgentTaskRole and ProviderId Deserialize into Explicit; absent pair into LegacyGeneral.
- Half pair and invalid routing fields map to INVALID_PARAMS. Legacy context errors and workspace pre-validation remain unchanged.
- Non-start actions retain strict unknown-field rejection. Parser takes only JSON; no Product/Provider/Store/Runtime instance or lookup.
- JSON schema transform adds Start-only pair constraint, existing role wire enum, and ProviderId whitespace/control restrictions. No routing resolution.
- Full serde/parse/registry/Ajv matrix includes roles, unregistered IDs, Unicode, types/null, half pairs, private/unknown fields, non-start routing and legacy context.

Commands run from repository root (native Windows):

| Command | State |
|---|---|
| cargo test --manifest-path src-tauri/Cargo.toml start_compatibility_tests -- --nocapture | FAIL compilation, 0 tests run; router E0027 missing routing; initial Schema API error corrected after this run |
| rustfmt --edition 2024 src-tauri/src/mcp/orchestration/dto.rs src-tauri/src/mcp/provider_query_tests.rs | PASS |
| cargo fmt --manifest-path src-tauri/Cargo.toml -- --check | PASS, exit 0 |
| git diff --check | PASS, exit 0 |

Pending: authorized mechanical router destructuring, compile/test matrix, descriptor hash freeze, registry/provider contract tests, cargo check, clippy; independent Host Gate. No implementation completion claim.
