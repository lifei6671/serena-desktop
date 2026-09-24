-- v11 introduces durable CommandRun receipts without reusing Agent Execution lifecycle tables.
-- Command output content remains live-memory only; durable evidence stores byte counts and SHA-256 digests.
CREATE TABLE command_runs (
    id TEXT PRIMARY KEY NOT NULL,
    request_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    canonical_workspace_root TEXT NOT NULL,
    workspace_generation INTEGER NOT NULL CHECK(workspace_generation >= 1),
    mode TEXT NOT NULL CHECK(mode IN ('process','shell')),
    relative_cwd TEXT NOT NULL,
    execution_mode TEXT NOT NULL CHECK(execution_mode IN ('auto','sync','async')),
    timeout_ms INTEGER NOT NULL CHECK(timeout_ms > 0),
    status TEXT NOT NULL CHECK(status IN (
        'starting','running','cancelling',
        'completed','failed','cancelled','interrupted','unknown'
    )),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(revision >= 0),
    runtime_platform TEXT NOT NULL CHECK(runtime_platform IN ('windows','macos','other')),
    containment_type TEXT NOT NULL,
    pid INTEGER CHECK(pid IS NULL OR pid > 0),
    started_at INTEGER,
    completed_at INTEGER,
    exit_code INTEGER,
    timed_out INTEGER NOT NULL DEFAULT 0 CHECK(timed_out IN (0,1)),
    termination_reason TEXT,
    stdout_total_bytes INTEGER NOT NULL DEFAULT 0 CHECK(stdout_total_bytes >= 0),
    stderr_total_bytes INTEGER NOT NULL DEFAULT 0 CHECK(stderr_total_bytes >= 0),
    stdout_sha256 TEXT,
    stderr_sha256 TEXT,
    error_code TEXT,
    error_message TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(workspace_id, request_key)
);
CREATE INDEX command_runs_workspace_created
ON command_runs(workspace_id, created_at DESC, id);
CREATE TABLE work_command_links (
    work_run_id TEXT NOT NULL,
    command_run_id TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    PRIMARY KEY(work_run_id, command_run_id),
    FOREIGN KEY(work_run_id) REFERENCES work_runs(id) ON DELETE RESTRICT,
    FOREIGN KEY(command_run_id) REFERENCES command_runs(id) ON DELETE RESTRICT
);
