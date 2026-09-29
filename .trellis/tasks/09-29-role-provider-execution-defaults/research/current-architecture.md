# Current Architecture Findings

- Baseline is clean at `54ec425` on `feat/codebuddy`, ahead of origin by the two user-owned Host commits `ef84516` and `54ec425`; they are excluded from this delivery unit and must not be rewritten.
- `AgentProviderSettings` currently owns `providers` plus the exact five-key `roleRouting`. Start routing is resolved under the Broker management lock in `agent/task_manager.rs` and carried as `FrozenStartRouting` into the Store transaction.
- Fresh Product creation currently hard-codes `execution_profile: {}`. Retry already overwrites Provider/role before canonicalization. Continuation parses the stored JSON but `continuation_core_eligible` rejects every non-literal `{}` profile.
- Provider catalog projection is in `agent/product.rs`; local settings mutations are serialized by the Broker management lock in `commands.rs`; MCP providers projection and frontend fixtures assume `providers` plus `roleRouting`.
- CodeBuddy `fresh.rs` owns session catalog validation and `DesiredConfiguration`; its current shape supports one config option and therefore cannot sequence model plus reasoning.
- Codex `app_server.rs` owns thread/turn JSON-RPC params. The fixed compatibility contract is in `codex/compatibility.rs`, and Provider start/resume wiring is in `codex/provider.rs`.
- `AgentPanel.tsx` already protects role/provider setting writes with per-key generations and uses the project Select component. The role grid currently renders only role and Provider.
- CodeGraph exploration was attempted first as required by repository instructions but the request did not complete and was terminated after repeated bounded waits; subsequent discovery used targeted literal/symbol searches and focused file reads.
- `trellis-start` and `trellis-brainstorm` skill files are not installed in this environment. Equivalent authoritative steps were loaded from `.trellis/workflow.md` and `get_context.py`; Trellis scripts run through local `uv` with a workspace-local cache because WindowsApps `python.exe` is inaccessible.
