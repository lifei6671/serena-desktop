# CB5-003 Design

## Process and SDK reuse

Reuse the CB5-002 task-local pattern: harness owns the canonical CodeBuddy Code Node child and pipes; official Rust ACP SDK receives only external streams. No product dependency changes.

## Client capabilities

Keep ACP Client filesystem/terminal capabilities false. CodeBuddy uses its own local tools inside the temporary cwd. This keeps Serena from becoming an ACP filesystem or terminal proxy in Phase 5.

## Event capture

Tee raw NDJSON at the transport boundary. Sanitize auth/credential/private arbitrary metadata. Preserve protocol fields, ids, session ids, update types, terminal result/stop reason, tool names/categories where public and needed for contract evidence.

SDK typed DTO and raw wire must be stored separately when CodeBuddy extension fields are not modeled by SDK.

## Prompt isolation

Every real scenario gets a fresh temporary directory outside the repo. Build a full recursive manifest before and after. Reject evidence if any path outside the scenario cwd is intentionally targeted by the prompt.

## conversationRequestId

Run two independent fresh sessions to avoid prompt-history confounding:
- Session A prompt without metadata.
- Session B prompt with generated conversationRequestId.

Adoption criteria are evidence-based: if plain prompt succeeds, the field is not required. It may still be recommended if it is carried/echoed or materially improves correlation. Record that distinction.

## Terminal semantics

Do not assume finish reason names. Capture the SDK/raw prompt response exactly. Later Provider mapping must use only the frozen fields.

## canExecute gate

`canExecuteEvidence = initialize PASS + session/new PASS + prompt PASS + exact session identity + terminal convergence + workspace isolation`.

Activity richness is not itself an execute admission gate, but raw ordering must be known before product activity projection.