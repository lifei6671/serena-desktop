CREATE TABLE runtime_instances (
    id TEXT PRIMARY KEY NOT NULL,

    owner_host_instance_id TEXT NOT NULL,

    job_name TEXT UNIQUE,
    job_session_id INTEGER CHECK(job_session_id >= 0),

    job_creation_mode TEXT,
    job_handle_inheritable INTEGER,
    job_kill_on_close INTEGER,
    job_breakaway_allowed INTEGER,
    job_policy_verified_at INTEGER,
    codex_executable_path TEXT,
    codex_version TEXT,
    protocol_schema_sha256 TEXT,

    codex_pid INTEGER,
    codex_process_start_token TEXT,

    state TEXT NOT NULL CHECK(state IN
        ('preparing','starting','running','terminating','terminated','unknown')),

    started_at INTEGER,
    stopped_at INTEGER,

    termination_evidence_type TEXT,
    termination_evidence_at INTEGER,
    termination_evidence_state TEXT NOT NULL DEFAULT 'unknown'
        CHECK(termination_evidence_state IN ('unknown','complete')),
    last_error_code TEXT,
    last_error_message TEXT,

    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK(termination_evidence_state != 'complete' OR
        (termination_evidence_type IS NOT NULL AND termination_evidence_at IS NOT NULL)),

    CHECK (
        job_creation_mode =
        'proc_thread_attribute_job_list'
    ),

    CHECK (
        job_handle_inheritable = 0
    ),

    CHECK (
        job_kill_on_close = 1
    ),

    CHECK (
        job_breakaway_allowed = 0
    )
);

CREATE TABLE executions (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    request_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    prompt TEXT NOT NULL,
    execution_profile_json TEXT NOT NULL,

    workspace_id TEXT NOT NULL,
    canonical_workspace_root TEXT NOT NULL,

    provider TEXT NOT NULL CHECK(provider = 'codex'),
    mode TEXT NOT NULL CHECK(mode IN ('read_only','workspace_write')),

    runtime_instance_id TEXT,

    thread_id TEXT,
    turn_id TEXT,

    status TEXT NOT NULL CHECK(status IN
        ('dispatch_pending','running',
         'cancel_requested','cancelling','finalizing','reconciling',
         'completed','failed','cancelled','interrupted','unknown')),
    dispatch_state TEXT NOT NULL DEFAULT 'not_dispatched'
        CHECK(dispatch_state IN ('not_dispatched','dispatching','dispatched','uncertain')),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(revision >= 0),

    provider_terminal_status TEXT,
    provider_terminal_evidence_at INTEGER,
    provider_terminal_evidence_runtime_instance_id TEXT,

    background_cleanup_runtime_instance_id TEXT,
    background_cleanup_evidence_at INTEGER,
    background_cleanup_state TEXT NOT NULL DEFAULT 'unknown'
        CHECK(background_cleanup_state IN ('unknown','accepted','polling','empty','uncertain')),

    runtime_termination_evidence_runtime_instance_id TEXT,
    runtime_termination_evidence_at INTEGER,

    release_evidence_state TEXT NOT NULL
        DEFAULT 'incomplete' CHECK(release_evidence_state IN ('incomplete','complete')),
    release_evidence_kind TEXT CHECK(release_evidence_kind IN
        ('not_dispatched','same_runtime_cleanup','runtime_terminated','operator_override')),
    release_evidence_json TEXT,

    final_result_json TEXT,
    result_completeness TEXT NOT NULL DEFAULT 'unknown'
        CHECK(result_completeness IN ('unknown','partial','complete')),
    started_at INTEGER,
    error_code TEXT,
    error_message TEXT,

    interrupt_requested_at INTEGER,
    interrupt_ack_at INTEGER,
    interrupt_timeout_at INTEGER,
    interrupt_diagnostic TEXT,

    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER,

    UNIQUE(agent_id, request_key),
    UNIQUE(id, canonical_workspace_root),
    FOREIGN KEY(runtime_instance_id) REFERENCES runtime_instances(id) ON DELETE RESTRICT,
    FOREIGN KEY(provider_terminal_evidence_runtime_instance_id) REFERENCES runtime_instances(id),
    FOREIGN KEY(background_cleanup_runtime_instance_id) REFERENCES runtime_instances(id),
    FOREIGN KEY(runtime_termination_evidence_runtime_instance_id) REFERENCES runtime_instances(id),
    CHECK(background_cleanup_state != 'empty' OR
        (runtime_instance_id IS NOT NULL AND
         background_cleanup_runtime_instance_id IS NOT NULL AND
         background_cleanup_runtime_instance_id = runtime_instance_id AND
         background_cleanup_evidence_at IS NOT NULL)),
    CHECK(release_evidence_state != 'complete' OR
        (release_evidence_kind IS NOT NULL AND release_evidence_json IS NOT NULL))
);
CREATE INDEX executions_runtime_state ON executions(runtime_instance_id, status);
CREATE UNIQUE INDEX executions_one_unresolved_per_agent ON executions(agent_id)
WHERE status NOT IN ('completed','failed','cancelled','interrupted');

CREATE TRIGGER prevent_execution_runtime_rebind
BEFORE UPDATE OF runtime_instance_id
ON executions
WHEN
    OLD.runtime_instance_id IS NOT NULL
    AND NEW.runtime_instance_id
        IS NOT OLD.runtime_instance_id
BEGIN
    SELECT RAISE(
        ABORT,
        'execution runtime instance is immutable'
    );
END;

CREATE TABLE workspace_claims (
    canonical_workspace_root TEXT PRIMARY KEY NOT NULL,

    execution_id TEXT NOT NULL UNIQUE,
    claim_type TEXT NOT NULL CHECK(claim_type = 'exclusive_execution'),

    acquired_at INTEGER NOT NULL,

    FOREIGN KEY(execution_id, canonical_workspace_root)
        REFERENCES executions(id, canonical_workspace_root) ON DELETE RESTRICT
);
