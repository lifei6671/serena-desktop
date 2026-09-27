# CB6-002 CodeBuddy Windows Job-at-Creation Launcher

## Goal

Provide CodeBuddy with Windows process ownership from the first runnable instant, and freeze the external-process path and provider-child environment primitives consumed later by CB6-003 and CB7-002.

## Requirements

- `CreateProcessW` success means the created process is already in the configured Job through `PROC_THREAD_ATTRIBUTE_JOB_LIST`; post-spawn `AssignProcessToJobObject` is forbidden.
- The Job uses `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, forbids both breakaway modes, has a non-inheritable Job handle, and passes only stdin/stdout/stderr through `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`.
- The final executable is absolute and directly managed. `.cmd`/`.bat` wrappers are rejected at the launcher boundary; a resolved `node.exe + absolute installed script + --acp` LaunchSpec manages `node.exe` itself from creation.
- Consume CB6-001 `ResolvedLaunchSpec` without starting ACP or changing its discovery precedence/PATH projection.
- Project local verbatim paths to ordinary drive paths and verbatim UNC to ordinary UNC. Apply an explicit UNC support policy and fail closed when unsupported; never map a network drive.
- Verify the projected path resolves to the same workspace identity as the frozen canonical root using the repository identity rule. Keep the frozen canonical root as the only StateStore/Claim/Execution authority.
- Return one verified projected path value intended for both process `current_dir` and later `session/new.cwd`; no display/projected value becomes persisted authority.
- Copy the SerenaDesktop Host process environment, override only PATH with the resolver projection, build a complete sorted double-NUL-terminated Unicode environment block, and launch with `CREATE_UNICODE_ENVIRONMENT`.
- Do not use CommandRun's minimal environment, mutate global environment, or expose full env/PATH/credentials through diagnostics or `Debug`.
- Any error after `CreateProcessW` succeeds must return the owned process, Job, and stdio handles to the caller. This task does not invent CodeBuddy persistence/recovery or convert drop/primary-PID kill into termination evidence.

## Acceptance Criteria

- [x] First-runnable Job membership and no escape window are demonstrated by a Windows fixture.
- [x] Real Node + script containment is tested when local Node is available; otherwise an equivalent controllable Windows child-tree fixture is executed and the evidence boundary is explicit.
- [x] Unsafe wrapper LaunchSpecs fail closed before process creation.
- [x] Stdio inheritance is a whitelist and the Job/extra inheritable handles are not visible to the child.
- [x] Job terminate, KILL_ON_JOB_CLOSE, and policy validation are covered.
- [x] Local verbatim/ordinary identity, UNC supported/unsupported, and identity mismatch behaviors are covered.
- [x] Host environment inheritance, refreshed PATH replacement, Unicode block encoding, and diagnostic redaction are covered.
- [x] CB6-001 `ResolvedLaunchSpec` wiring is covered without ACP initialization.
- [x] Focused Codex launcher/runtime regressions prove its Job ownership, error, and termination evidence semantics remain unchanged.
- [x] Required Rust tests, fmt, clippy, scope/freshness checks, frozen target, and independent read-only review complete without P0/P1/P2.

## Non-goals

- ACP initialize/transport/session/new/prompt or any real CodeBuddy/CB5 probe.
- Runtime StateStore migration, startup recovery, cancel/continue/usage, Claim release, UI, or remote protocol.
- Auto-install, `npx` download, shell-string Runtime launch, schema/migration changes, or macOS work.
- Changing Codex Job ownership, Runtime lifecycle, error semantics, termination evidence, or Claim semantics.
- Commit or push.

## Authority

- `docs/implementation-task-breakdown-multi-agent-provider-codebuddy-v0.1.md` CB6-002.
- `docs/technical-design-multi-agent-provider-codebuddy-v0.1.md` §13, §14.1-§14.1.2, §23.1, and safety invariants 7-9.
- Current CB6-001 code in `src-tauri/src/agent/codebuddy/` is the implementation baseline.
