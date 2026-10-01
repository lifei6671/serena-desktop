# CB8-001 CodeBuddy Cancel Runtime Path

Authority: user frozen requirements; implementation-task-breakdown CB8-001; technical-design §22; CB5-004 Host cancel PASS; CB7-005 atomic release.

Persist intent first. Local cancellation only without Runtime binding/attempt/terminal. Provider cancel validates provider identity and writes Store independently of CLI health/enabled. Unique original Prompt owner polls durable exact identity at 100ms Skip and sends official typed session/cancel once after physical prompt flush. Single-slot permit allows only exact sessionId notification; ack only after full pipe write and flush.

Only original exact Prompt response supplies terminal; cancelled maps Cancelled, natural response keeps real outcome. Send failure or bounded absolute terminal timeout -> InterruptTimeout -> original Job shutdown -> evidence -> Interrupted/Unknown. No terminal fabrication or Claim release from cancel. Preserve private identities, terminal and existing atomic release. Pre-MarkSent cancellation fails closed.

Required: generic Store/Codex, transport permit/physical flush, native Windows cancel matrix (pre-runtime, prepared, post-flush, write, natural terminal, end_turn, no terminal, pipe fail, duplicate), unavailable/disabled control, safety/recovery; full CodeBuddy (parallel plus serial if needed), TaskManager/catalog/MCP, telemetry/same-runtime/usage, fmt/check/clippy baseline, freeze and independent read-only FULL_SCOPE review. canCancel false until Gates pass. No commit/push, registry, schema, retry, Continue/Usage/permission or real probes.
