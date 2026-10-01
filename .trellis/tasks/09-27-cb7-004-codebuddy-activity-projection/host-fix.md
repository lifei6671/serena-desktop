# Host narrow correction — same CB7-004 delivery unit

User identified that initial SessionUpdate::ToolCall ignored typed call.status and always published Tool. Prior target B68A6632F495AFDD6DE6201F733579578DDBF1D04062C4CCA5439B8602F675D4 and FULL_SCOPE result are superseded. Original clean delivery baseline remains c890919ff8cfedab6f73682f257ff1cc889d9027 on feat/codebuddy; do not redefine baseline or create a new task. Resume confirmed all 8 prior frozen source hashes before editing (host-fix-baseline.json).

Only activity.rs and activity/tests.rs changed relative to prior frozen sources (host-fix-source-scope.json). ToolCall first remembers structured ToolKind category under the existing capacity limit, then matches typed status: Pending/InProgress -> Tool(category), Completed/Failed -> Provider(None), future typed status -> drop. This matches ToolCallUpdate lifecycle semantics. No title/name/rawInput/rawOutput/command classification, Store/terminal/Claim/Usage/result/capability/public execute change.

New test initial_tool_status_preserves_category_for_followup exercises four known statuses across Read, Execute + cargo test, Execute + cargo build, Other + Read: 16 cases. Each asserts exact phase/category, then checks no-kind Pending increment inherits the same structured category even after an initial Completed/Failed frame. Existing full kind/text-free/Host fixture tests remain intact.

Required validation commands are appended to validation-results.jsonl under host-fix-*; every Cargo command receives existing C:/Users/lifei/.cargo/bin and C:/nvm4w/nodejs on process PATH. No installation, WSL, real CodeBuddy/Host probe, commit or push. Prior broad TaskManager/Codex baseline failures remain historical evidence in this same unit; their untouched scopes are not rerun for this narrow change.

The task remains in_progress until new frozen-target independent read-only FULL_SCOPE review reports P0-P3=0 and final source/context freshness matches. Historical snapshots are preserved as pre-host-fix-* files; no previous PASSED result is reused as current approval.
