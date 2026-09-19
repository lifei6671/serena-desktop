# Phase 2D Batch B implementation plan

1. Add a CodeGraph adapter beside the Source/Git adapters, with a small injectable command seam for deterministic fixtures.
2. Parse only the proven status JSON fields and project it to the generic capability health DTO.
3. Register the shell provider without registering a CodeGraph Remote tool or modifying the legacy runtime/query lifecycle.
4. Implement the three declared local prepare actions through the existing provider/Manager path, including post-action status.
5. Add deterministic command fixtures for status and action lifecycle, then run focused Rust tests, owned rustfmt, `cargo check --locked`, and diff checks.
