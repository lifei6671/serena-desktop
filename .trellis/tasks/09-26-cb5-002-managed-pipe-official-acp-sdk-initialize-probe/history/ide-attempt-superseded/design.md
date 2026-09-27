# CB5-002 Design

## Two separate axes

`protocolCompatible = initializeSucceeded && negotiatedProtocolVersion == supportedProtocolVersion`

`ProviderCapabilities = evidence from initialize + later method probes`

Do not combine them. A protocol-compatible provider may have `canContinue=false` or `tokenUsage=false` and still be connected/healthy. `availableForNewExecution` later remains `health && canExecute`, so lack of execute capability is expressed without calling the protocol incompatible.

## Harness ownership

Task-local Rust harness owns `Child`. Spawn canonical CodeBuddy entry with piped stdio. Convert/tunnel child's stdout/stdin into official SDK transport accepted external reader/writer types. Record evidence that SDK receives streams and never owns Child.

Probe must enforce timeout and finally cleanup Child on every path.

## Real initialize

Use official SDK request model where possible; do not hand-invent field names if SDK is available. Persist sanitized raw JSONL by teeing bytes at the transport boundary.

Do not assume `protocolVersion` representation; record exact JSON type/value returned and map it to SDK type.

## Negative fixtures

- fake peer returns mismatched protocolVersion: SDK may deserialize successfully, but Serena compatibility predicate must reject.
- fake peer closes output: EOF result must be bounded and deterministic.
- fake peer omits optional capability: initialize remains protocol-compatible; capability projection false/unknown.

## SDK fallback decision

If official SDK cannot attach to external streams, distinguish:
1. crate/download/environment unavailable;
2. API truly lacks external transport attachment;
3. adapter implementation bug.

Only (2) justifies freezing task-local minimal NDJSON initialize client. Fallback scope is not production code yet.