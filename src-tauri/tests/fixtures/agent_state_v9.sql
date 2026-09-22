PRAGMA foreign_keys = ON;

-- 冻结的 v9 相关表结构与一组完整 Windows Runtime/Execution/Claim 数据。
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
    state TEXT NOT NULL CHECK(state IN ('preparing','starting','running','terminating','terminated','unknown')),
    started_at INTEGER,
    stopped_at INTEGER,
    termination_evidence_type TEXT,
    termination_evidence_at INTEGER,
    termination_evidence_state TEXT NOT NULL DEFAULT 'unknown' CHECK(termination_evidence_state IN ('unknown','complete')),
    last_error_code TEXT,
    last_error_message TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK(termination_evidence_state != 'complete' OR (termination_evidence_type IS NOT NULL AND termination_evidence_at IS NOT NULL)),
    CHECK(job_creation_mode = 'proc_thread_attribute_job_list'),
    CHECK(job_handle_inheritable = 0),
    CHECK(job_kill_on_close = 1),
    CHECK(job_breakaway_allowed = 0)
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
    status TEXT NOT NULL,
    dispatch_state TEXT NOT NULL DEFAULT 'not_dispatched',
    revision INTEGER NOT NULL DEFAULT 0,
    provider_terminal_status TEXT,
    provider_terminal_evidence_at INTEGER,
    provider_terminal_evidence_runtime_instance_id TEXT,
    background_cleanup_runtime_instance_id TEXT,
    background_cleanup_evidence_at INTEGER,
    background_cleanup_state TEXT NOT NULL DEFAULT 'unknown',
    runtime_termination_evidence_runtime_instance_id TEXT,
    runtime_termination_evidence_at INTEGER,
    release_evidence_state TEXT NOT NULL DEFAULT 'incomplete',
    release_evidence_kind TEXT,
    release_evidence_json TEXT,
    final_result_json TEXT,
    result_completeness TEXT NOT NULL DEFAULT 'unknown',
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
    last_activity_at INTEGER,
    activity_phase TEXT,
    tool_category TEXT,
    parent_execution_id TEXT,
    workspace_generation INTEGER NOT NULL DEFAULT 1,
    activity_summary_code TEXT,
    activity_sequence INTEGER NOT NULL DEFAULT 0,
    UNIQUE(agent_id, request_key),
    UNIQUE(id, canonical_workspace_root),
    FOREIGN KEY(runtime_instance_id) REFERENCES runtime_instances(id) ON DELETE RESTRICT
);

CREATE TABLE workspace_claims (
    canonical_workspace_root TEXT PRIMARY KEY NOT NULL,
    execution_id TEXT NOT NULL UNIQUE,
    claim_type TEXT NOT NULL CHECK(claim_type = 'exclusive_execution'),
    acquired_at INTEGER NOT NULL,
    FOREIGN KEY(execution_id, canonical_workspace_root) REFERENCES executions(id, canonical_workspace_root) ON DELETE RESTRICT
);

CREATE TABLE execution_usage (
    execution_id TEXT PRIMARY KEY NOT NULL,
    provider_id TEXT NOT NULL,
    completeness TEXT NOT NULL,
    usage_revision INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(execution_id) REFERENCES executions(id) ON DELETE RESTRICT
);

CREATE TABLE codex_thread_usage_epochs (
    runtime_instance_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    latest_cumulative_json TEXT NOT NULL,
    latest_turn_id TEXT,
    captured_at INTEGER NOT NULL,
    PRIMARY KEY(runtime_instance_id, thread_id)
);

CREATE TABLE codex_execution_usage_state (
    execution_id TEXT PRIMARY KEY NOT NULL,
    runtime_instance_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    turn_id TEXT,
    baseline_kind TEXT NOT NULL,
    baseline_json TEXT,
    latest_cumulative_json TEXT,
    telemetry_state TEXT NOT NULL,
    terminal_at INTEGER,
    freeze_at INTEGER,
    last_event_at INTEGER,
    FOREIGN KEY(execution_id) REFERENCES executions(id) ON DELETE RESTRICT
);

INSERT INTO runtime_instances (
    id, owner_host_instance_id, job_name, job_session_id, job_creation_mode,
    job_handle_inheritable, job_kill_on_close, job_breakaway_allowed,
    job_policy_verified_at, codex_pid, codex_process_start_token, state,
    stopped_at, termination_evidence_type, termination_evidence_at,
    termination_evidence_state, created_at, updated_at
) VALUES (
    'runtime-v9', 'old-host', 'Global\\SerenaCodex-v9', 7,
    'proc_thread_attribute_job_list', 0, 1, 0, 100, 4242, 'filetime:123',
    'terminated', 200, 'managed_job_destroyed', 200, 'complete', 10, 200
);

INSERT INTO executions (
    id, agent_id, request_key, request_hash, prompt, execution_profile_json,
    workspace_id, canonical_workspace_root, provider, mode, runtime_instance_id,
    status, dispatch_state, revision, runtime_termination_evidence_runtime_instance_id,
    runtime_termination_evidence_at, release_evidence_state, release_evidence_kind,
    release_evidence_json, result_completeness, created_at, updated_at
) VALUES (
    'execution-v9', 'agent-v9', 'request-v9', 'hash-v9', 'fixture', '{}',
    'workspace-v9', '/fixture/workspace', 'codex', 'workspace_write', 'runtime-v9',
    'reconciling', 'dispatched', 3, 'runtime-v9', 200, 'incomplete', NULL,
    NULL, 'unknown', 10, 200
);

INSERT INTO workspace_claims (
    canonical_workspace_root, execution_id, claim_type, acquired_at
) VALUES ('/fixture/workspace', 'execution-v9', 'exclusive_execution', 10);

PRAGMA user_version = 9;
