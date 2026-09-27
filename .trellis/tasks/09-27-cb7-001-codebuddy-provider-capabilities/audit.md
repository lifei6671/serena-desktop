# CB7-001 production audit / review context

Baseline: feat/codebuddy @ ecd6c078451b8d1616723722b98cb4c2b7549bb2, clean. Delivery-owned scope is new task evidence and the two test files listed below. baseline.json freezes 714 tracked CB5/CB6 evidence and relevant production/usage files. No production edits are necessary.

- codebuddy/provider.rs: descriptor id codebuddy, display_name CodeBuddy; only metadata.product_version supplies version. capabilities: can_recover=cfg!(windows); remaining five fields false. ACP catalog is not read here.
- provider/registry.rs: get_registered ignores health, preserving historical recovery access. get checks health only; it is NOT execution admission. provider/control.rs ProviderAdmissionPolicy::admit enforces registered -> enabled -> health -> requested capability. Changing Registry::get into execute-only lookup would unnecessarily change its generic contract.
- task_manager.rs refresh_provider_health rebuilds CodeBuddy with self.store.clone() and self.owner.clone(); replaces only the registered adapter. Existing CB6-005 disabled_missing_cli_refresh_retains_recovery_authority tests real restart/recovery through this path. Rerun unchanged test, do not rewrite recovery.
- product.rs provider_catalog derives enabled independently and computes availableForNewExecution from agent_enabled && enabled && health available && can_execute. Hence CodeBuddy is never available for new execution at this gate.
- Existing provider_catalog_codex.json has two cfg(windows) consumers (Product and MCP catalog tests) and already says CodeBuddy canRecover=true; test adapts using cfg!(windows). New cross-platform CodeBuddy snapshot must use cfg!(windows) explicitly; no new public fields or platform hardcoding.

Evidence authority: technical-design §7 and §33 + current user matrix. CB5 Fresh Execute/Cancel contract PASS do not fulfill CB7 fresh product implementation or CB8 cancel implementation. Frozen CB5-004 verification.md top section is current contract PASS, later entries are superseded; CB5-003 older task status/verification header is historical and not current authority. No frozen evidence changes or probes. Windows CB6-005 recovery Gate is completed. Non-Windows canRecover stays false.

Implementation ownership: implement child owns src-tauri/src/agent/codebuddy/provider/tests.rs and src-tauri/src/agent/product/provider_catalog_tests.rs. Main owns planning/evidence and final verification. Final independent reviewer must not be implementer, must be read-only and cover all delivery artifacts plus frozen executable hashes.

Review: Strict independent CHILD_AGENT, FULL_SCOPE, Tier 1 test-only change; correctness, test regression value, platform semantics, contract fidelity and scope. code-delivery-review Rust profile applies. Repository has frontend spec only; backend UI rules do not apply. Spec update assessment: no new production pattern or architecture decision; existing contract only, no spec change required.

Validation plan (cwd src-tauri; Cargo C:\Users\lifei\.cargo\bin\cargo.exe): focused provider::tests, provider registry/control tests, product catalog/projection tests, existing disabled_missing_cli_refresh_retains_recovery_authority; cargo fmt --all -- --check; cargo check --lib --tests; cargo clippy --lib --tests -- -D warnings. Preserve original logs and exit codes. Only known usage_tests.rs:987 await_holding_lock may be recorded as baseline; no suppression.

Platform: native Windows x86_64-pc-windows-msvc Rust 1.98.0. Non-Windows cfg semantics/static tests review is distinct from execution evidence. No project-provided Docker runner exists in checkout; Linux validation ENVIRONMENT_UNAVAILABLE and stopped, macOS NOT_RUN. No WSL or host claims of Linux acceptance. CodeGraph discovery was blocked by tool approval policy never; local read-only source fallback used.

