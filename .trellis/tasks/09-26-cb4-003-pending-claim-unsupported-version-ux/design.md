# CB4-003 Design

## Data model

### Pending blocker

Use existing ExecutionView only. A loaded row is a pending blocker when:
- row.provider.id matches card provider;
- row.attention === 'pending_explicit_resume' OR row.availableActions.canResumePending === true.

`attention=pending_explicit_resume` is already projected only from persisted dispatch_pending/not_dispatched + owned Claim + no Runtime attempt/worker/quarantine. UI does not re-derive Claim safety.

### Unsupported version

Frontend presentation accepts optional `diagnosticCode?: string | null` on provider management/catalog view data. Current backend may omit it.
Only exact code `CODEBUDDY_VERSION_UNSUPPORTED` selects unsupported-version copy. Health/version/provider ID alone never do.
This is a forward-compatible consumer contract for later CodeBuddy discovery; do not change Remote Product schema in this task solely to fabricate the signal.

## Actions

Provider enable/disable uses existing local Tauri command `agent_provider_set_enabled`.
Role routing continues through `agent_provider_set_role_route`.
Pending cancel uses existing `agent_operation(cancel)`.
View uses existing details flow.
Resume remains user-explicit from task details/list after provider is re-enabled.

## Mutation state

Use per-provider pending state for enabled toggle/re-enable. Optimistically reflect desired enabled state only while mutation is pending or wait for returned AgentProviderSettings; on success patch current catalog policy from returned settings, on failure restore prior state and toast.
Catalog poll crossing mutation boundaries must not overwrite newly committed enabled state with stale snapshot; reuse the generation/override pattern already used for Role Routing or factor a provider-policy equivalent.

## UI layout

- Card keeps six status fields.
- Add enable Switch/action in card footer.
- If disabled + blocker rows, render warning block below fields.
- Render each blocker with task summary + 查看任务 / 取消任务.
- Warning-level 重新启用 Provider calls same enable mutation.
- No Force Unlock.
- Unsupported-version notice is informational and has no override control.

## Tests

- pending claim fixture with view/cancel/reenable.
- disable running -> draining, no cancellation.
- re-enable does not auto-resume.
- provider enable save failure rollback and stale poll protection.
- disabled Role binding remains.
- unsupported diagnostic fixture exact copy/no override.
- unavailable without diagnostic does not show unsupported copy.
- no Force Unlock.
- Agent UI full regression.