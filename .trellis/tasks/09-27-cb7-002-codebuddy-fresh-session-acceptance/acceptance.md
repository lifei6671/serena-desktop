# CB7-002 acceptance map

| Contract | Implementation / Evidence |
| --- | --- |
| SDK id/method authority; raw whitelist only models | protocol.rs incoming/take_session_new_extensions; client tests exact SDK id, unrelated/unknown/duplicate responses, duplicate snapshot, wrong/missing/empty identity, malformed models, frame bound, shutdown |
| Actual CB5-003 returned catalogs | cb7_002_sdk_v1_preserves_host_catalog_with_extensions replays existing sanitized success response; typed session/modes/config match exactly and snapshot models matches exactly; no real probe |
| Workspace authority, one projection, exact cwd | fresh::prepare reads Execution; LaunchRequest owns verified ExternalProcessPath, cloned value solely supplies NewSessionRequest; native peer writes in same cwd and wire cwd equals temp ordinary path; canonical DB root remains verbatim |
| Wrong/missing/UNC/identity fail before side effect | fresh::tests::boundary; CB6-002 existing external_path tests retained; no shell or argv configuration injection |
| Durable R1 before launch | reserve_runtime_attempt, prepare_codebuddy_runtime, narrow bind_codebuddy_prepared_runtime OCC, private BindRuntime; Runtime::start_persisted validates original row then owns launch and cleanup |
| Live policy/process then initialize | native gated test observes state starting + policy/process/private R1 at initialize request; next gate observes running + actual protocol=1 before session/new |
| Minimal new, one request | typed NewSessionRequest, default empty mcpServers, no meta; matrix exact request count=1 for new error/EOF/timeout |
| Exact session durable before route/replay/config | gated test observes private exact-session by set_mode request; early frames in original order, wrong session retained separately; concurrent generic revision failure aborts before identity write/config/acceptance |
| Default no configuration | DesiredConfiguration::default is none; matrix shows zero set requests; no mapping from generic Execution mode |
| Explicit mode/config only | catalog validates ids/current values; desired auto requires advertised current catalog; typed set_mode ACK; explicit config id/value requires advertised typed option and matching ACK current value; no guessed fallback; multitask=true rejected |
| Runtime cleanup | persisted Runtime explicit shutdown awaits recovery evidence; Drop first closes whole Job then schedules evidence; unaccepted Prepared drop and closed-before-accept tested; next startup classifies durable state and releases only through existing evidence authority |
| Acceptance boundary / no prompt | real Sink callback after durable session/config ACK and acceptance_ready; printed native ORDER trace ends accepted -> STOP; production preparation has no prompt method call |
| Capability/lifecycle unchanged | TaskManager focused test exercises admission and direct execute: unsupported, sink false, no runtime/attempt/private rows; CB7-001 catalog and provider matrix regressions included |
| No schema or public identity pollution | schema/Cargo unchanged; private session not thread/turn; catalogs only in Prepared memory |

Cancellation/crash classification: prelaunch reserved/preparing rows remain recovery-visible; unverified policy cannot mint complete evidence. While CreateProcess is pending, the private ownership task prevents an early destroyed observation; after caller cancellation it completes bounded initialize then drops/cleans the owned Job. From initialize onward, dropped prepare owns Runtime and schedules cleanup. New/config unknown outcome never replays side effects. No execution finalization is added to preparation.

CB7-003 integration note: current generic Dispatch transition assumes it establishes first Runtime binding. CB7-003 must consume the already prepared R1 and implement its full lifecycle; this card deliberately does not invoke Dispatch or execute. The internal Prepared object retains Runtime/session and exposes a consuming acceptance boundary only.

Review repair: explicit category=mode config ACK reconciles legacy modes even without a notification; mode_option_ack_reconciles_legacy_modes_without_notification proves acceptance and prompt count zero.
