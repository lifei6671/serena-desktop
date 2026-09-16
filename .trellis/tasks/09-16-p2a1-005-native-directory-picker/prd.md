# P2A1-005 Native Directory Picker

## Goal

Implement the P2A1-005 local native single-directory picker candidate flow without registry mutation.

## Requirements

- Provide the local-only Tauri IPC command `workspace_pick_directory()` using the already-installed `tauri-plugin-dialog` single-folder picker.
- Return one selected local directory path. Cancellation returns `None`/`null` successfully and is a no-op.
- Keep picker behavior separate from validation and registration: do not canonicalize, inspect, persist, mutate the Workspace Registry, synchronize, activate, or prepare providers.
- Register the command only in the local Tauri `generate_handler!`; do not expose it through Remote MCP.
- Add `api.workspacePickDirectory(): Promise<string | null>` invoking exactly `workspace_pick_directory`.
- Add a compact ProjectPanel action that stores a selected result as component-local `candidateRoot`, displays it as pending, preserves it on cancellation and picker failure, and reports picker errors through the existing toast mechanism.

## Acceptance Criteria

- [x] The Rust conversion seam returns the selected path, maps cancellation to `Ok(None)`, and returns conversion failures as errors.
- [x] The command has no Registry, Supervisor, Provider, Git, Serena, or CodeGraph dependency.
- [x] The frontend API invokes the frozen IPC name with `string | null` result typing.
- [x] A selected path appears as a local candidate; cancel and errors neither create nor clear it and never trigger inspect, register, sync, or activation.
- [x] Focused Rust and frontend tests, build/type check, locked library check, targeted rustfmt, and both diff checks are recorded before delivery.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
