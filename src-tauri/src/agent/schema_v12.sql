-- v12 仅改变 Provider/Role 表达。历史证据按原值复制，不生成释放或终止证据。
ALTER TABLE runtime_instances ADD COLUMN provider TEXT NOT NULL DEFAULT 'codex';
ALTER TABLE runtime_instances RENAME COLUMN codex_executable_path TO executable_path;
ALTER TABLE runtime_instances RENAME COLUMN codex_version TO executable_version;
ALTER TABLE runtime_instances RENAME COLUMN codex_pid TO process_id;
ALTER TABLE runtime_instances RENAME COLUMN codex_process_start_token TO process_start_token;
ALTER TABLE runtime_instances RENAME COLUMN protocol_schema_sha256 TO protocol_contract_sha256;

-- executions.provider 的旧 CHECK 必须重建父表才能移除。外键在外层迁移事务前
-- 暂停，并在提交前执行 foreign_key_check；所有子表与原始历史列值均保留。
CREATE TABLE executions_v12 (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    request_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    prompt TEXT NOT NULL,
    execution_profile_json TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    canonical_workspace_root TEXT NOT NULL,
    provider TEXT NOT NULL,
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
    release_evidence_state TEXT NOT NULL DEFAULT 'incomplete'
        CHECK(release_evidence_state IN ('incomplete','complete')),
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
    last_activity_at INTEGER,
    activity_phase TEXT CHECK(activity_phase IN ('provider','tool')),
    tool_category TEXT CHECK(tool_category IN ('build','test','command','read','edit','tool')),
    parent_execution_id TEXT,
    workspace_generation INTEGER NOT NULL DEFAULT 1 CHECK(workspace_generation >= 1),
    activity_summary_code TEXT,
    activity_sequence INTEGER NOT NULL DEFAULT 0 CHECK(activity_sequence >= 0),
    task_role TEXT NOT NULL DEFAULT 'general'
        CHECK(task_role IN ('development','testing','review','analysis','general')),
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

-- v11 的 46 列顺序在此显式列出，request_hash 与所有证据值原样复制。
INSERT INTO executions_v12 (
    id, agent_id, request_key, request_hash, prompt, execution_profile_json,
    workspace_id, canonical_workspace_root, provider, mode, runtime_instance_id,
    thread_id, turn_id, status, dispatch_state, revision,
    provider_terminal_status, provider_terminal_evidence_at,
    provider_terminal_evidence_runtime_instance_id,
    background_cleanup_runtime_instance_id, background_cleanup_evidence_at,
    background_cleanup_state, runtime_termination_evidence_runtime_instance_id,
    runtime_termination_evidence_at, release_evidence_state, release_evidence_kind,
    release_evidence_json, final_result_json, result_completeness, started_at,
    error_code, error_message, interrupt_requested_at, interrupt_ack_at,
    interrupt_timeout_at, interrupt_diagnostic, created_at, updated_at,
    completed_at, last_activity_at, activity_phase, tool_category,
    parent_execution_id, workspace_generation, activity_summary_code,
    activity_sequence, task_role
)
SELECT id, agent_id, request_key, request_hash, prompt, execution_profile_json,
    workspace_id, canonical_workspace_root, provider, mode, runtime_instance_id,
    thread_id, turn_id, status, dispatch_state, revision,
    provider_terminal_status, provider_terminal_evidence_at,
    provider_terminal_evidence_runtime_instance_id,
    background_cleanup_runtime_instance_id, background_cleanup_evidence_at,
    background_cleanup_state, runtime_termination_evidence_runtime_instance_id,
    runtime_termination_evidence_at, release_evidence_state, release_evidence_kind,
    release_evidence_json, final_result_json, result_completeness, started_at,
    error_code, error_message, interrupt_requested_at, interrupt_ack_at,
    interrupt_timeout_at, interrupt_diagnostic, created_at, updated_at,
    completed_at, last_activity_at, activity_phase, tool_category,
    parent_execution_id, workspace_generation, activity_summary_code,
    activity_sequence, 'general'
FROM executions;

DROP TABLE executions;
ALTER TABLE executions_v12 RENAME TO executions;
CREATE INDEX executions_runtime_state ON executions(runtime_instance_id, status);
CREATE UNIQUE INDEX executions_one_unresolved_per_agent ON executions(agent_id)
    WHERE status NOT IN ('completed','failed','cancelled','interrupted');
CREATE TRIGGER prevent_execution_runtime_rebind
BEFORE UPDATE OF runtime_instance_id ON executions
WHEN OLD.runtime_instance_id IS NOT NULL
    AND NEW.runtime_instance_id IS NOT OLD.runtime_instance_id
BEGIN
    SELECT RAISE(ABORT, 'execution runtime instance is immutable');
END;
