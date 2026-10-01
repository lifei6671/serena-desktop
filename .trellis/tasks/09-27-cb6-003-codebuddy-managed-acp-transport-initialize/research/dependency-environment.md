# SDK availability and evidence provenance

cwd: `E:\wx_lifeilin\github.com\lifei6671\serena-desktop`; native Windows PowerShell.

## Current attempts

1. `python .trellis/scripts/task.py create 'CB6-003 Managed ACP Transport Initialize' --slug cb6-003-codebuddy-managed-acp-transport-initialize`: UNAVAILABLE; python is not recognized. `py --version`: `No installed Python found!`. Maintain task artifacts manually, not a fabricated CLI success.
2. `C:\Users\lifei\.cargo\bin\cargo.exe info agent-client-protocol@2.2.0`: FAILED to load dependency; unable to download registry config.json, Schannel `AcquireCredentialsHandle failed: SEC_E_NO_CREDENTIALS (0x8009030e)`. Cargo's built-in retries failed. This ran in a command batch with subsequent reads; the batch exit 0 is NOT Cargo success.
3. `Invoke-WebRequest -Uri https://static.crates.io/crates/agent-client-protocol/agent-client-protocol-2.2.0.crate -OutFile <task>/research/sdk.crate -TimeoutSec 30`: FAILED, `Authentication failed, see inner exception.` No downloaded crate/source produced.
4. `C:\Program Files\Git\mingw64\bin\curl.exe --fail --max-time 30 --output <task>/research/sdk.crate https://static.crates.io/crates/agent-client-protocol/agent-client-protocol-2.2.0.crate`: FAILED, tool exit 1, curl error 35, same Schannel credential error. Git curl also uses Schannel, so it is not a working alternative TLS stack.
5. `C:\Users\lifei\.cargo\bin\cargo.exe info --offline agent-client-protocol@2.2.0`: FAILED, exit 1, could not find package in registry. Default Cargo source directory has zero matching ACP SDK directories and cache search returned no matching crate.

No certificate checks were disabled; no credentials were accessed. No proxy/bootstrap infrastructure was created. No real provider was spawned. Dependency retrieval failure is an environment blocker, not a protocol health event or Material Contract Difference.

CodeGraph workspace discovery was UNAVAILABLE: MCP tool requires approval but approval policy is never. Local source reads were used. `.agents/skills/trellis-brainstorm/SKILL.md` is absent; `.trellis/workflow.md` was read directly and user supplied the complete authorized requirements.

## Reusable repository evidence (read only)

- `.trellis/tasks/09-26-cb5-002-managed-pipe-official-acp-sdk-initialize-probe/harness/Cargo.toml`, `harness/src/main.rs`, `evidence/sdk-details.json` and `evidence/initialize.jsonl`: inspected in this turn. Initialize NDJSON contains sanitized real-cli protocol-v1 request/response and fake EOF cases. These remain previous evidence, not a new Host probe.
- `.trellis/tasks/09-26-cb5-004-cancel-permission-contract/evidence/host-exact-launcher-proof/wire.jsonl`: located by literal config_option_update search; fixture extraction/order audit is still pending. Do not claim a regression fixture or test exists yet.
- `.trellis/tasks/09-27-cb6-002-codebuddy-windows-job-at-creation/verification.md`: historical launcher verification read for command conventions only; no current rerun claimed.

## Resume requirement

A functioning official Cargo registry/TLS path or locally available authenticated official SDK dependency/source is needed to inspect and compile the integration. Resume the same delivery baseline and task, not a new task or a custom client. Material Contract Difference remains NOT_ASSESSED for the dispatcher APIs; external managed ByteStreams support remains the user-frozen fact.

## Resume: RESOLVED_BY_HOST

Host reports CommandRun command-26872-1790491393629634-117 exit 0 for the existing CB5-002 cargo_bootstrap.py fetch. Explicit Python: C:\Users\lifei\AppData\Local\Programs\Python\Python312\python.exe. Restored official SDK 2.2.0/derive 2.2.0/schema 1.9.1 under CB5-002 .cargo-cache/registry/src/127.0.0.1-4419b7f07c862e47. Current local read confirms the SDK source exists. The prior TLS/offline errors above are historical and retained; they do not establish a Material Contract Difference. Bootstrap uses localhost HTTP and validated upstream HTTPS; no credential access/certificate bypass. Resume the original baseline; do not edit/cache-track the SDK sources.
