# Windows launcher contract research

## Existing proof boundary

- `codex/windows_launcher.rs` creates/configures a non-inheritable KILL_ON_CLOSE Job, supplies `PROC_THREAD_ATTRIBUTE_JOB_LIST` and an explicit stdio `HANDLE_LIST`, then calls `CreateProcessW` directly with an absolute executable.
- Its first post-success step adopts ownership and its post-create identity failure returns `LaunchError.created`; Codex `runtime.rs` turns that owner into a Runtime and performs Job termination plus durable evidence handling.
- Codex Runtime validates no inheritance, KILL_ON_CLOSE, and no breakaway before process creation; later failures retain Runtime ownership instead of reducing it to a string or killing only the PID.
- Codex uses `lpEnvironment=null`. Changing that code to accept the CodeBuddy refreshed PATH would touch a frozen semantic and error/evidence boundary, so CB6-002 uses a provider-specific thin implementation.

## Existing path authority

- `config::canonicalize_workspace_root` validates directory existence and returns the filesystem canonical path.
- `config::same_workspace_root_identity` is the repository authority for comparing already-canonicalized roots and uses Windows UTF-16 ordinal case-insensitive equality.
- `mcp::serena::display` demonstrates verbatim local/UNC projection but is a display helper; it must not become provider authority. CB6-002 implements the projection at the provider boundary and then validates it through the config identity helpers.

## CB6-001 input

- `ResolvedLaunchSpec` carries an absolute resolved executable, argv, and the exact resolver PATH projection.
- npm wrappers resolve to canonical `node.exe`, canonical installed package script, and `--acp`; unresolved `.cmd`/`.bat` candidates do not become successful discovery results.
- Launcher validation still rejects wrapper executables independently so future/manual construction cannot weaken the runtime boundary.

## Environment and test boundary

- A non-null Windows environment block requires `CREATE_UNICODE_ENVIRONMENT`, UTF-16 `key=value` entries, single-NUL separators and a double-NUL terminator.
- The block is owned by the request through `CreateProcessW`; no environment mutation is required.
- The current host has Rust under `C:\Users\lifei\.cargo\bin` but no Node found on PATH/common `C:\Program Files\nodejs`; the test must therefore support an equivalent compiled Windows child-tree fixture and report Node evidence separately.
- CodeGraph discovery was attempted but the connector requires approval while the host policy is `never`; local exact reads and `rg` were used instead.
