# CB3-004 — DESIGN_BLOCKER

Status: DESIGN_BLOCKER. No implementation or test files changed. CB3-005 not entered.

## Authority evidence

- `src-tauri/src/serena.rs:173-175`: Supervisor `runtime`, `operation`, and `provider_policy` are private. No production API exposes an operation guard or a generic callback inside that lock.
- `serena.rs:362-398` (`snapshot`) and `481-487` (`workspace_registry_config`) return cloned configuration; neither retains a configuration guard across creation.
- `serena.rs:320-347`: WorkspaceWriteGuard registers a workspace refcount under operation, then releases operation. It prevents workspace removal, not provider-policy mutation.
- `serena.rs:765-778`: `mutate_provider_settings` takes operation, then runtime, then commits policy while holding its write lock. `replace_config:782-804` also takes operation and can change `agentEnabled`.
- `src-tauri/src/agent/provider/control.rs:20-21,80-99`: policy settings are private; `registered_enabled` releases its settings read guard before returning. The policy owns AgentProviderSettings, not ManagerConfig.agentEnabled.
- `src-tauri/src/agent/task_manager.rs:638-670`: `ensure_product_start_enabled` runs at line 657 before entering `SupervisorState::create_workspace_start`.
- `serena.rs:162-170,704-735`: WorkspaceStartCreation has no routing identity or admission callback. The existing creation method takes operation, resolves the WorkspaceLease, checks remove admission, then calls the blocking Store transaction. It does not re-read agentEnabled or role/provider authority.
- `src-tauri/src/agent/store/transactions/product.rs:21-31,107-122`: creation currently has no provider/role input and constructs Codex + General. Store has no Supervisor configuration authority.
- Existing `serena.rs:1439-1466` test documents that provider mutation waits for operation; this pass inspected it but did not execute it.

## Concrete race

1. T1 reads current config and validates role/provider and enabled state in Product/TaskManager.
2. That read returns a clone; its configuration/policy guards are released.
3. T2 obtains Supervisor operation and commits a different route, disables the provider, or changes agentEnabled.
4. T1 subsequently obtains operation through create_workspace_start and commits Execution + Claim using the earlier authority.

Reading again just before calling create_workspace_start still leaves steps 2–4 possible. Holding a WorkspaceWriteGuard does not block step 3. A new independent mutex would not serialize the existing mutations. Creating then compensating violates this task's explicit rejection-before-side-effects rule.

## Minimal required scope adjustment

Allow only the Start coordination portion of `src-tauri/src/serena.rs` (WorkspaceStartCreation / create_workspace_start and focused tests) in addition to the current allowed scope. Under the existing operation lock, read current configuration and invoke routing/admission before the blocking creation transaction; pass the final provider + taskRole to Store. Preserve the existing mutation implementation, lock order and WorkspaceLease checks. Provider facts must also remain current through this boundary using the existing TaskManager/Registry synchronization. This proposal is not implemented.

No policy mutation, new Authority, fallback, prompt classification or Continue/Resume change is needed or authorized by this report.

## Frozen targets, not implemented or tested here

| Start intent | Required order |
|---|---|
| Explicit | agentEnabled → registered (`AGENT_PROVIDER_NOT_FOUND`) → enabled (`AGENT_PROVIDER_DISABLED`) → role configured (`AGENT_ROLE_NOT_CONFIGURED`) → exact route match (`AGENT_ROLE_PROVIDER_MISMATCH`) → health (`AGENT_PROVIDER_UNAVAILABLE`) → canExecute (`AGENT_PROVIDER_CAPABILITY_UNSUPPORTED`) → create |
| LegacyGeneral | agentEnabled → resolve current general route (`AGENT_ROLE_NOT_CONFIGURED` if absent) → registered → enabled → health → canExecute → create with General |

New request identity must remain execution-request-v3 with both provider and taskRole. Same-key exact retries must be idempotent; a changed provider or role must conflict. Approved historical v2/v1 compatibility must not rewrite stored hashes. Static inspection found that current fresh retry construction starts from the persisted row (`product.rs:47-73`), and work_adapter can return a preflight retry before the Supervisor path (`work_adapter.rs:261-274`); implementation must carry the newly resolved identity and recheck authority for those retries too. These are static findings and required targets, not passing requestKey/race tests.

## Verification

CWD: `E:\wx_lifeilin\github.com\lifei6671\serena-desktop`.

| Command / check | Result |
|---|---|
| Targeted CodeGraph + source inspection of Authority/creation paths | PASS; establishes blocker, not implementation acceptance |
| `git diff --check` | PASS, exit 0; existing LF/CRLF notices only |
| Focused routing / creation / identity tests | NOT_RUN; no implementation candidate |
| Product / TaskManager / Store and orchestration/registry regressions | NOT_RUN |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib mcp:: -- --nocapture` | NOT_RUN; Host's earlier 295 passed / 4 ignored is not fresh evidence |
| `cargo check --manifest-path src-tauri/Cargo.toml --locked` | NOT_RUN |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | NOT_RUN |
| Clippy | NOT_RUN; no claim that the historical warning is the only current finding |

Tests executed by this implementer: 0. Parent reported no project Docker Desktop Linux runner; no WSL, improvised runner or native substitute was used.

Diff owned by this implementer: this evidence document only. Existing dirty implementation files remain untouched. Parent started the existing task in its current session and confirmed the session is active; its task.json startup metadata belongs to the same task coordination, not this implementer's code delta. Review not required for this prose-only result. CB3-004 remains incomplete pending the stated coordination scope and subsequent implementation/verification.
