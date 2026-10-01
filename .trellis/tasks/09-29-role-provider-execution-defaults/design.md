# Design

## Contracts

`AgentProviderSettings` gains `roleDefaults`, serialized as `role -> providerId -> {model, reasoning}`. The outer map is sparse but only accepts the five `AgentTaskRole` strings. Provider IDs use the existing `ProviderId` validator even when unregistered or disabled. `ExecutionProfile` is the canonical sparse `{model?, reasoning?}` snapshot; absent and null configuration values both resolve to absent profile fields, so the canonical all-default profile remains `{}`.

The Product provider snapshot adds `roleDefaults` additively. The existing provider catalog remains a cheap policy/health projection and never calls configuration discovery. A separate Provider port accepts a registry-resolved workspace authority and returns a provider-neutral `ExecutionConfigurationCatalog` containing model choices, model-specific reasoning choices/defaults, optional global reasoning fallback, and current/default selections.

## Data flow

```text
Config roleDefaults
  -> resolve_start_routing under Broker management lock
  -> FrozenStartRouting.execution_profile
  -> canonical request hash + executions.execution_profile_json
  -> Provider Start application

source execution_profile_json
  -> typed validation at Continue/recovery boundary
  -> child Execution exact inherited JSON/profile
  -> Provider resume/turn application
```

Catalog flow is separate:

```text
Tauri command(providerId, workspaceId)
  -> Broker management authority + Workspace Registry canonical root
  -> registered/enabled/available Provider port
  -> bounded temporary managed runtime
  -> initialize/catalog RPC(s)
  -> provider-neutral projection
  -> complete shutdown and convergence
```

Catalog failures return stable errors and never mutate `roleDefaults`.

## CodeBuddy

Reuse the managed launcher, ACP initialization guards, `session/new(cwd, mcpServers=[])`, raw models, and typed config options. Project model entries from the validated session catalog. Project reasoning from the live `thought_level` category/id option without hard-coded option values. Extend `DesiredConfiguration` to carry independent optional model and reasoning selections.

When starting, send the model option first. Wait for its acknowledgement and incorporate any updated config options before validating/sending reasoning. An unset value sends no `session/set_config_option`. Any configured value missing from the authoritative catalog fails closed before prompt dispatch. Temporary discovery ownership follows the existing Job/termination convergence model on success, error, or cancellation.

## Codex

Extend the fixed compatibility contract with `model/list` and the generated schema features for model/effort request properties. The managed app-server client initializes and paginates `model/list` without parsing CLI output. Catalog projection retains hidden metadata but the UI filters hidden entries by default. Reasoning options and defaults come from the selected model only.

For a fresh Execution, pass configured model to `thread/start` and pass configured model plus effort to `turn/start`. For Continue, resume the existing thread and pass the source profile to `turn/start`. Missing values remain omitted so Codex owns its defaults.

## Persistence and identity

No database migration is required because `execution_profile_json` already exists. A typed parser validates exactly the provider-neutral sparse schema and produces canonical JSON. `FrozenStartRouting` carries the resolved profile into `WorkspaceStartCreation`. Fresh retries overwrite historical identity inputs with the current resolved Provider, role, and profile before canonicalization, so a changed default yields `EXECUTION_REQUEST_KEY_CONFLICT`.

Continuation eligibility stops requiring literal `{}` and instead requires a valid profile. Child creation and recovery read and validate the source JSON, then inherit it exactly rather than consulting configuration.

## UI state

Each role row renders four logical columns: role label, Provider Select, model Select, and reasoning Select. The selected Provider indexes `roleDefaults[role][provider]`. Catalog state is keyed by `providerId + workspaceId`; loading/error state does not modify settings. A saved value not present in the current catalog is injected as a disabled/current-unavailable choice. Reasoning options are recomputed for the selected/saved model; an invalid saved reasoning remains visible until an explicit user mutation. Unsupported reasoning shows `不支持` and disables the control. No workspace disables catalog-dependent controls with a stable hint.

Mutations retain the current generation/pending pattern. Provider-route and role-default writes remain independent, and polling responses cannot overwrite a newer local mutation.

## Review and rollback boundaries

1. Configuration/profile/Product contracts and persistence identity.
2. CodeBuddy and Codex catalog/application lifecycle.
3. Local API, Product/MCP projection, AgentPanel, and fixtures.
4. Cross-layer integration, lifecycle cleanup, validation, and final independent review.

Rollback is deletion of the additive `roleDefaults`, catalog port/commands, profile parameters, and UI columns; no schema migration or committed state is introduced by this task.
