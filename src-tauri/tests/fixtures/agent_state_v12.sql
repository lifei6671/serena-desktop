-- 冻结 v12 输入：既有 v1..v12 schemas + frozen v11 rows；CB6-004 不动态调用当前 migration 构造此输入。
BEGIN TRANSACTION;
CREATE TABLE codex_execution_usage_state (
    execution_id TEXT PRIMARY KEY NOT NULL,
    runtime_instance_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    turn_id TEXT,
    baseline_kind TEXT NOT NULL CHECK(baseline_kind IN ('fresh_zero','observed_same_epoch','unknown')),
    baseline_json TEXT,
    latest_cumulative_json TEXT,
    telemetry_state TEXT NOT NULL CHECK(telemetry_state IN ('accepting','terminal_grace','frozen')),
    terminal_at INTEGER,
    freeze_at INTEGER,
    last_event_at INTEGER,

    FOREIGN KEY(execution_id) REFERENCES executions(id) ON DELETE RESTRICT
);
INSERT INTO "codex_execution_usage_state" VALUES('execution-completed-v11','runtime-windows-v11','thread-v11','turn-v11','observed_same_epoch','{"totalTokens":2}','{"totalTokens":17}','frozen',250,251,244);
CREATE TABLE codex_thread_usage_epochs (
    runtime_instance_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    latest_cumulative_json TEXT NOT NULL,
    latest_turn_id TEXT,
    captured_at INTEGER NOT NULL,
    PRIMARY KEY(runtime_instance_id, thread_id)
);
INSERT INTO "codex_thread_usage_epochs" VALUES('runtime-windows-v11','thread-v11','{"totalTokens":17}','turn-v11',244);
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
INSERT INTO "command_runs" VALUES('command-v11','command-request-v11','dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd','workspace-v11','C:/fixture-v11',2,'process','.','sync',30000,'completed',2,'windows','windows_job',610,150,160,0,0,'exited',5,0,'2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824',NULL,NULL,NULL,140,160);
CREATE TABLE execution_activity_events (
    execution_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    activity_phase TEXT,
    tool_category TEXT,
    summary_code TEXT,
    activity_revision TEXT NOT NULL,
    observed_at INTEGER NOT NULL,
    PRIMARY KEY(execution_id, sequence),
    FOREIGN KEY(execution_id) REFERENCES executions(id) ON DELETE RESTRICT
);
CREATE TABLE execution_runtime_attempts (
    execution_id TEXT NOT NULL REFERENCES executions(id),
    runtime_instance_id TEXT PRIMARY KEY NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE execution_usage (
    execution_id TEXT PRIMARY KEY NOT NULL,
    provider_id TEXT NOT NULL,

    input_tokens INTEGER CHECK(input_tokens IS NULL OR input_tokens >= 0),
    cached_input_tokens INTEGER CHECK(cached_input_tokens IS NULL OR cached_input_tokens >= 0),
    cache_write_input_tokens INTEGER CHECK(cache_write_input_tokens IS NULL OR cache_write_input_tokens >= 0),
    output_tokens INTEGER CHECK(output_tokens IS NULL OR output_tokens >= 0),
    reasoning_tokens INTEGER CHECK(reasoning_tokens IS NULL OR reasoning_tokens >= 0),
    total_tokens INTEGER CHECK(total_tokens IS NULL OR total_tokens >= 0),
    model_context_window INTEGER CHECK(model_context_window IS NULL OR model_context_window >= 0),

    completeness TEXT NOT NULL CHECK(completeness IN ('unknown','partial','complete')),
    usage_revision INTEGER NOT NULL DEFAULT 0 CHECK(usage_revision >= 0),
    updated_at INTEGER NOT NULL,

    FOREIGN KEY(execution_id) REFERENCES executions(id) ON DELETE RESTRICT
);
INSERT INTO "execution_usage" VALUES('execution-completed-v11','codex',12,3,NULL,5,2,17,32000,'complete',4,245);
CREATE TABLE "executions" (
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
INSERT INTO "executions" VALUES('execution-completed-v11','agent-completed-v11','request-completed-v11','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','completed prompt','{"a":1}','workspace-v11','C:/fixture-v11','codex','workspace_write','runtime-windows-v11','thread-v11','turn-v11','completed','dispatched',7,'completed',230,'runtime-windows-v11',NULL,NULL,'unknown',NULL,NULL,'complete','runtime_terminated','{"runtime":"runtime-windows-v11"}','{"result":"done"}','complete',NULL,NULL,NULL,NULL,NULL,NULL,NULL,100,250,250,NULL,NULL,NULL,NULL,2,NULL,0,'general');
INSERT INTO "executions" VALUES('execution-pending-v11','agent-pending-v11','request-pending-v11','bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb','pending prompt','{}','workspace-v11','C:/pending-v11','codex','read_only',NULL,NULL,NULL,'dispatch_pending','not_dispatched',0,NULL,NULL,NULL,NULL,NULL,'unknown',NULL,NULL,'incomplete',NULL,NULL,NULL,'unknown',NULL,NULL,NULL,NULL,NULL,NULL,NULL,101,101,NULL,NULL,NULL,NULL,NULL,2,NULL,0,'general');
INSERT INTO "executions" VALUES('execution-unknown-v11','agent-unknown-v11','request-unknown-v11','cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc','unknown prompt','{"retry":false}','workspace-macos-v11','/fixture-v11','codex','workspace_write','runtime-macos-v11','thread-unknown-v11',NULL,'unknown','uncertain',4,NULL,NULL,NULL,NULL,NULL,'unknown',NULL,NULL,'incomplete',NULL,NULL,NULL,'unknown',NULL,NULL,NULL,NULL,NULL,NULL,NULL,102,180,NULL,NULL,NULL,NULL,NULL,3,'execution.reconciling',0,'general');
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
    executable_path TEXT,
    executable_version TEXT,
    protocol_contract_sha256 TEXT,

    process_id INTEGER,
    process_start_token TEXT,

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
    updated_at INTEGER NOT NULL, runtime_platform TEXT NOT NULL DEFAULT 'windows'
    CHECK(runtime_platform IN ('windows','macos')), containment_type TEXT NOT NULL DEFAULT 'windows_job'
    CHECK(containment_type IN ('windows_job','macos_process_group')), process_identity_scheme TEXT NOT NULL DEFAULT 'windows_filetime_v1'
    CHECK(process_identity_scheme IN ('windows_filetime_v1','darwin_proc_bsd_start_v1')), containment_process_group_id INTEGER, containment_session_id INTEGER, containment_verified_at INTEGER, provider TEXT NOT NULL DEFAULT 'codex',
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
INSERT INTO "runtime_instances" VALUES('runtime-windows-v11','host-v11','job-v11',8,'proc_thread_attribute_job_list',0,1,0,101,'C:/Codex/codex.exe','1.2.3','eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',410,'windows-filetime-v11','terminated',110,240,'managed_job_destroyed',241,'complete',NULL,NULL,100,241,'windows','windows_job','windows_filetime_v1',NULL,NULL,NULL,'codex');
INSERT INTO "runtime_instances" VALUES('runtime-macos-v11','host-v11',NULL,NULL,NULL,NULL,NULL,NULL,NULL,'/opt/codex','1.2.4',NULL,510,'darwin_proc_bsd_start_v1:100:200','running',120,NULL,NULL,NULL,'unknown',NULL,NULL,100,120,'macos','macos_process_group','darwin_proc_bsd_start_v1',510,510,121,'codex');
CREATE TABLE thread_names (thread_id TEXT PRIMARY KEY NOT NULL, name TEXT);
CREATE TABLE work_command_links (
    work_run_id TEXT NOT NULL,
    command_run_id TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    PRIMARY KEY(work_run_id, command_run_id),
    FOREIGN KEY(work_run_id) REFERENCES work_runs(id) ON DELETE RESTRICT,
    FOREIGN KEY(command_run_id) REFERENCES command_runs(id) ON DELETE RESTRICT
);
INSERT INTO "work_command_links" VALUES('work-v11','command-v11',141);
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
INSERT INTO "work_execution_links" VALUES('work-v11','execution-completed-v11',NULL,'{"role":"general"}',106);
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
, workspace_generation INTEGER NOT NULL DEFAULT 1 CHECK(workspace_generation >= 1));
INSERT INTO "work_runs" VALUES('work-v11','workspace-v11','C:/fixture-v11','Frozen v11 work','migration baseline','completed',3,'{"accepted":true}',105,260,260,2);
CREATE TABLE workspace_claims (
    canonical_workspace_root TEXT PRIMARY KEY NOT NULL,

    execution_id TEXT NOT NULL UNIQUE,
    claim_type TEXT NOT NULL CHECK(claim_type = 'exclusive_execution'),

    acquired_at INTEGER NOT NULL,

    FOREIGN KEY(execution_id, canonical_workspace_root)
        REFERENCES executions(id, canonical_workspace_root) ON DELETE RESTRICT
);
INSERT INTO "workspace_claims" VALUES('/fixture-v11','execution-unknown-v11','exclusive_execution',130);
CREATE INDEX execution_runtime_attempts_execution ON execution_runtime_attempts(execution_id);
CREATE TRIGGER execution_runtime_attempts_immutable_update BEFORE UPDATE ON execution_runtime_attempts
BEGIN SELECT RAISE(ABORT, 'RUNTIME_ATTEMPT_IMMUTABLE'); END;
CREATE TRIGGER execution_runtime_attempts_immutable_delete BEFORE DELETE ON execution_runtime_attempts
BEGIN SELECT RAISE(ABORT, 'RUNTIME_ATTEMPT_IMMUTABLE'); END;
CREATE TRIGGER runtime_instances_v10_validate_insert
BEFORE INSERT ON runtime_instances
WHEN NOT (
    (
        NEW.runtime_platform = 'windows'
        AND NEW.containment_type = 'windows_job'
        AND NEW.process_identity_scheme = 'windows_filetime_v1'
        AND NEW.containment_process_group_id IS NULL
        AND NEW.containment_session_id IS NULL
        AND NEW.containment_verified_at IS NULL
        AND (
            NEW.termination_evidence_state != 'complete'
            OR (
                NEW.state = 'terminated'
                AND NEW.termination_evidence_type IN ('job_active_processes_zero','managed_job_destroyed')
            )
        )
    )
    OR
    (
        NEW.runtime_platform = 'macos'
        AND NEW.containment_type = 'macos_process_group'
        AND NEW.process_identity_scheme = 'darwin_proc_bsd_start_v1'
        AND NEW.job_name IS NULL
        AND NEW.job_session_id IS NULL
        AND NEW.job_creation_mode IS NULL
        AND NEW.job_handle_inheritable IS NULL
        AND NEW.job_kill_on_close IS NULL
        AND NEW.job_breakaway_allowed IS NULL
        AND NEW.job_policy_verified_at IS NULL
        AND (
            (
                NEW.process_id IS NULL
                AND NEW.process_start_token IS NULL
                AND NEW.containment_process_group_id IS NULL
                AND NEW.containment_session_id IS NULL
                AND NEW.containment_verified_at IS NULL
            )
            OR (
                NEW.process_id IS NOT NULL
                AND NEW.process_start_token IS NOT NULL
                AND NEW.containment_process_group_id IS NOT NULL
                AND NEW.containment_session_id IS NOT NULL
                AND NEW.containment_verified_at IS NOT NULL
                AND NEW.process_id = NEW.containment_process_group_id
                AND NEW.process_id = NEW.containment_session_id
            )
        )
        AND (
            NEW.state IN ('preparing','unknown')
            OR (
                NEW.process_id IS NOT NULL
                AND NEW.process_start_token IS NOT NULL
                AND NEW.containment_process_group_id IS NOT NULL
                AND NEW.containment_session_id IS NOT NULL
                AND NEW.containment_verified_at IS NOT NULL
            )
        )
        AND (
            NEW.termination_evidence_state != 'complete'
            OR (
                NEW.state = 'terminated'
                AND NEW.termination_evidence_type IN (
                    'macos_live_process_group_empty',
                    'macos_recovered_process_group_empty'
                )
            )
        )
    )
)
BEGIN
    SELECT RAISE(ABORT, 'RUNTIME_PLATFORM_EVIDENCE_INVALID');
END;
CREATE TRIGGER runtime_instances_v10_validate_update
BEFORE UPDATE OF
    runtime_platform, containment_type, process_identity_scheme,
    containment_process_group_id, containment_session_id, containment_verified_at,
    job_name, job_session_id, job_creation_mode, job_handle_inheritable,
    job_kill_on_close, job_breakaway_allowed, job_policy_verified_at,
    process_id, process_start_token, state,
    termination_evidence_type, termination_evidence_at, termination_evidence_state
ON runtime_instances
WHEN NOT (
    (
        NEW.runtime_platform = 'windows'
        AND NEW.containment_type = 'windows_job'
        AND NEW.process_identity_scheme = 'windows_filetime_v1'
        AND NEW.containment_process_group_id IS NULL
        AND NEW.containment_session_id IS NULL
        AND NEW.containment_verified_at IS NULL
        AND (
            NEW.termination_evidence_state != 'complete'
            OR (
                NEW.state = 'terminated'
                AND NEW.termination_evidence_type IN ('job_active_processes_zero','managed_job_destroyed')
            )
        )
    )
    OR
    (
        NEW.runtime_platform = 'macos'
        AND NEW.containment_type = 'macos_process_group'
        AND NEW.process_identity_scheme = 'darwin_proc_bsd_start_v1'
        AND NEW.job_name IS NULL
        AND NEW.job_session_id IS NULL
        AND NEW.job_creation_mode IS NULL
        AND NEW.job_handle_inheritable IS NULL
        AND NEW.job_kill_on_close IS NULL
        AND NEW.job_breakaway_allowed IS NULL
        AND NEW.job_policy_verified_at IS NULL
        AND (
            (
                NEW.process_id IS NULL
                AND NEW.process_start_token IS NULL
                AND NEW.containment_process_group_id IS NULL
                AND NEW.containment_session_id IS NULL
                AND NEW.containment_verified_at IS NULL
            )
            OR (
                NEW.process_id IS NOT NULL
                AND NEW.process_start_token IS NOT NULL
                AND NEW.containment_process_group_id IS NOT NULL
                AND NEW.containment_session_id IS NOT NULL
                AND NEW.containment_verified_at IS NOT NULL
                AND NEW.process_id = NEW.containment_process_group_id
                AND NEW.process_id = NEW.containment_session_id
            )
        )
        AND (
            NEW.state IN ('preparing','unknown')
            OR (
                NEW.process_id IS NOT NULL
                AND NEW.process_start_token IS NOT NULL
                AND NEW.containment_process_group_id IS NOT NULL
                AND NEW.containment_session_id IS NOT NULL
                AND NEW.containment_verified_at IS NOT NULL
            )
        )
        AND (
            NEW.termination_evidence_state != 'complete'
            OR (
                NEW.state = 'terminated'
                AND NEW.termination_evidence_type IN (
                    'macos_live_process_group_empty',
                    'macos_recovered_process_group_empty'
                )
            )
        )
    )
)
BEGIN
    SELECT RAISE(ABORT, 'RUNTIME_PLATFORM_EVIDENCE_INVALID');
END;
CREATE INDEX command_runs_workspace_created
ON command_runs(workspace_id, created_at DESC, id);
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
COMMIT;
PRAGMA user_version=12;
