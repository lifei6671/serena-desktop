# Phase 2D Batch B design

## Frozen boundary

`CodeGraphCapabilityProvider` is a `StatelessCommand` capability shell. It owns no root, process, runtime slot, cache, or Remote tool. Every operation receives the server-resolved `WorkspaceLease` and uses only `lease.canonical_root`.

## P0-006 compatibility result

The real 1.6.0 CLI returns an object for `pendingChanges` (`added`, `modified`, `removed`), and places `reindexRecommended` inside `index`. The implementation treats any non-zero count or `index.reindexRecommended == true` as stale. `index.state == "complete"` with all counts zero and no recommendation is ready. `initialized == false` is absent/not-prepared before fields unavailable in that state are inspected.

## Readiness projection

The command runner must be bounded and cancellation-aware. It returns only typed status; raw stderr, command line, and root stay internal. The JSON parser requires the fields needed by the branch being evaluated. A canonicalized `projectPath` must exactly equal the Lease root. Missing binary is `not_installed`; malformed/schema/command/root-identity errors are unknown/error. No observation or tool call initiates index work.

## Explicit actions

`build_index`, `update_index`, and `rebuild_index` are descriptor actions, each `local_human` + `provider_prepare` + `warmRuntime=false`. Provider prepare maps them one-to-one to `init --yes`, `sync`, and `index` with the canonical root as the sole path argument. The existing Manager owns operation identity, activity, cancellation, mutual exclusion, and duplicate-action single-flight. Exit success is insufficient: provider prepare issues a second typed status probe and only reports success when its expected postcondition is observed.

## Registration

P0-007 has not frozen a process capacity policy. Therefore production registration uses the existing shell-compatible stateless-command runtime model and zero instance policy, while publishing no query tool. This does not create a RuntimeSlot and does not revive `mcp/codegraph.rs`.
