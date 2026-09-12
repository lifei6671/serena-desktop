CREATE TABLE work_runs (
    id TEXT PRIMARY KEY NOT NULL,

    workspace_id TEXT NOT NULL,
    canonical_workspace_root TEXT NOT NULL,

    title TEXT NOT NULL,
    goal TEXT,

    status TEXT NOT NULL CHECK (
        status IN ('active', 'completed', 'failed', 'cancelled')
    ),

    revision INTEGER NOT NULL DEFAULT 0,

    acceptance_json TEXT,

    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER
);

CREATE TABLE work_execution_links (
    work_run_id TEXT NOT NULL,
    execution_id TEXT NOT NULL UNIQUE,

    parent_execution_id TEXT,

    delegation_context_json TEXT,

    created_at INTEGER NOT NULL,

    PRIMARY KEY(work_run_id, execution_id),

    FOREIGN KEY(work_run_id)
        REFERENCES work_runs(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(execution_id)
        REFERENCES executions(id)
        ON DELETE RESTRICT
);
