# CB6-002 Design

## Boundary and reuse decision

Implement a CodeBuddy-owned Windows launcher in `agent/codebuddy/windows_launcher.rs`. Do not extract the Codex launcher in this card: Codex intentionally inherits Host environment through `lpEnvironment=null` and is coupled to frozen `CODEX_*` errors, Job names, persisted policy verification, and Runtime termination evidence. CodeBuddy additionally requires an explicit refreshed PATH and complete Unicode environment block. A shared launcher refactor would therefore enlarge the Codex blast radius without being required by this card.

Reuse only stable repository authorities:

- CB6-001 `ResolvedLaunchSpec` and its exact PATH projection.
- `config::canonicalize_workspace_root` plus `config::same_workspace_root_identity` for workspace identity.
- The already proven Win32 invariant set and RAII ownership shape from Codex, with CodeBuddy-specific names/errors and tests.

## External process path

`ExternalProcessPath` is a private verified value containing only the projected ordinary Win32 path. Construction takes the frozen canonical root plus an explicit UNC capability policy:

1. project `\\?\C:\...` to `C:\...` and `\\?\UNC\server\share\...` to `\\server\share\...`;
2. reject UNC when the supplied launcher/provider policy does not support it, without drive mapping;
3. canonicalize the projected path using the existing workspace root validator;
4. compare that canonical result to the frozen canonical root with `same_workspace_root_identity`;
5. reject missing/non-directory/mismatch and return the single projected value on success.

The value is intentionally not serializable and exposes only the path needed by the external process boundary. CB7-002 must reuse this same value for `session/new.cwd`; this card does not add that protocol call.

## Provider child environment

`ProviderChildEnvironment` snapshots the Host process environment into owned OS strings, removes every case-insensitive PATH spelling, inserts exactly one `Path` value built from `ResolvedLaunchSpec.path_projection`, sorts entries case-insensitively as required by Windows environment blocks, rejects embedded NULs or invalid names, and encodes `key=value\0...\0` in UTF-16 with a final extra NUL.

Production construction reads the current Host environment once per launch request. Tests use injected baselines to prove inheritance, Unicode, duplicate PATH removal, refreshed PATH replacement, and redacted `Debug`. No complete environment or PATH is persisted or logged.

## Launch and ownership

`LaunchRequest::from_resolved` validates the resolved executable before any Job/process creation:

- executable must be absolute and use a directly creatable `.exe`/`.com` boundary;
- arguments are passed as an argv vector through Microsoft CRT-compatible UTF-16 quoting;
- npm resolution remains `node.exe`, absolute script argv, `--acp`; `.cmd`/`.bat` cannot cross this boundary;
- current directory must be an already verified `ExternalProcessPath`;
- environment must be the explicit provider projection.

The launcher creates and configures a unique named Job, makes its handle non-inheritable, creates non-inheritable pipes whose child ends alone are marked inheritable, adds exactly two startup attributes (`JOB_LIST`, `HANDLE_LIST`), verifies policy before process creation, and calls `CreateProcessW` with `EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT`.

After successful `CreateProcessW`, all fallible post-create validation occurs only after process/thread handles are adopted. `LaunchError.created` owns the process, Job, and parent stdio handles on such failures. The later Runtime can terminate/persist evidence; this card does not create an incomplete alternate Runtime owner.

## Tests

- Pure tests: argv quoting, wrapper rejection, resolved spec mapping, local/UNC projection, identity mismatch, environment projection/block/redaction.
- Isolated real Windows fixture: first operation observes Job membership; argv, stdio, handle whitelist, policy, KILL_ON_JOB_CLOSE, `TerminateJobObject`, and post-create fault ownership.
- Child tree: run Node + generated local script if a `node.exe` exists without download; otherwise compile/run a Rust child-tree fixture. Assert root and spawned descendant are both in a Job.
- Codex regression: run existing focused `agent::codex::windows_launcher` and relevant `agent::codex::runtime` tests without modifying their sources or hashes.

## Material contract difference check

No blocking difference is currently known. The existing CodeBuddy skeleton has no Runtime owner yet, but this card can safely express post-success failure ownership through a launcher error that contains `CreatedChild`, matching the proven Codex boundary. No caller publishes or drops that ownership in production during CB6-002 because the launcher is not connected to ACP/Runtime yet.
