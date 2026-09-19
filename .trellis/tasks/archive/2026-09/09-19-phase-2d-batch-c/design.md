# P2D Batch C Design

## Authority and boundaries

Health is a local Tauri projection keyed only by the final explicit workspace ID. `DesktopSelectedWorkspace` determines the page target but never substitutes payload authority. The existing Rust manager and DTO contract remain unchanged.

## UI projection

`ProjectPanel` owns a small effect lifecycle: observe when its selected workspace changes; subscribe to the local safe activity event; unlisten on replacement/unmount; suppress stale completion updates. It renders provider cards by iterating `health.providers`, and stage/action rows by their arrays. A generic label/tone mapping handles stable DTO enum values without inspecting provider IDs. The three state dimensions retain independent badge labels.

Actions call the local prepare IPC with `{ workspaceId, providerId, actionId }`. The result supplies an opaque operation ID for cancellation only. Terminal action feedback triggers a fresh observe. Activity is matched by safe workspace/provider/action identity and does not expose raw provider diagnostics.

## Cleanup verification

Use existing manager admission, stop-flight, and provider test fixtures. Add only deterministic integration assertions for Remove and Host shutdown: entry/handle retention after failure, all live slots stopped after shutdown, A/B isolation, and direct-child CodeGraph containment. Any failing semantic must be repaired in the existing owner path, without a parallel cleanup mechanism.
