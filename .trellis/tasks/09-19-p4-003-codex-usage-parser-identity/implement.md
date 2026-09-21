# P4-003 Implementation Plan

1. Add fixed-schema Codex-private usage wire structs and strict integer deserialization in `agent/codex/protocol.rs`, then cover valid and invalid wire cases.
2. Extend the provider telemetry event with only safe cumulative fields and write its encapsulation tests.
3. Reuse Activity's binding inputs in a pure `usage_telemetry_event` mapper, integrate the Usage notification in the active client loop, and add focused adapter tests.
4. Run required focused Rust checks, formatter and diff check. Record command output/counts in `evidence.md`.
5. Run a full delivery-owned review before reporting; do not commit or push.
