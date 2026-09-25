-- 冻结的 v11 数据行；测试 helper 仅应用现有 v1..v11 schema 后载入，供 v12 直接迁移使用。
INSERT INTO runtime_instances (
    id, owner_host_instance_id, job_name, job_session_id, job_creation_mode,
    job_handle_inheritable, job_kill_on_close, job_breakaway_allowed,
    job_policy_verified_at, codex_executable_path, codex_version,
    protocol_schema_sha256, codex_pid, codex_process_start_token, state,
    started_at, stopped_at, termination_evidence_type, termination_evidence_at,
    termination_evidence_state, created_at, updated_at
) VALUES (
    'runtime-windows-v11', 'host-v11', 'job-v11', 8,
    'proc_thread_attribute_job_list', 0, 1, 0, 101,
    'C:/Codex/codex.exe', '1.2.3',
    'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', 410,
    'windows-filetime-v11', 'terminated', 110, 240,
    'managed_job_destroyed', 241, 'complete', 100, 241
);
INSERT INTO runtime_instances (
    id, owner_host_instance_id, codex_executable_path, codex_version,
    codex_pid, codex_process_start_token, state, started_at, created_at, updated_at,
    runtime_platform, containment_type, process_identity_scheme,
    containment_process_group_id, containment_session_id, containment_verified_at
) VALUES (
    'runtime-macos-v11', 'host-v11', '/opt/codex', '1.2.4', 510,
    'darwin_proc_bsd_start_v1:100:200', 'running', 120, 100, 120,
    'macos', 'macos_process_group', 'darwin_proc_bsd_start_v1', 510, 510, 121
);

INSERT INTO executions (
    id, agent_id, request_key, request_hash, prompt, execution_profile_json,
    workspace_id, canonical_workspace_root, workspace_generation, provider, mode,
    runtime_instance_id, thread_id, turn_id, status, dispatch_state, revision,
    provider_terminal_status, provider_terminal_evidence_at,
    provider_terminal_evidence_runtime_instance_id, release_evidence_state,
    release_evidence_kind, release_evidence_json, final_result_json,
    result_completeness, created_at, updated_at, completed_at
) VALUES (
    'execution-completed-v11', 'agent-completed-v11', 'request-completed-v11',
    'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    'completed prompt', '{"a":1}', 'workspace-v11', 'C:/fixture-v11', 2,
    'codex', 'workspace_write', 'runtime-windows-v11', 'thread-v11', 'turn-v11',
    'completed', 'dispatched', 7, 'completed', 230, 'runtime-windows-v11',
    'complete', 'runtime_terminated', '{"runtime":"runtime-windows-v11"}',
    '{"result":"done"}', 'complete', 100, 250, 250
);
INSERT INTO executions (
    id, agent_id, request_key, request_hash, prompt, execution_profile_json,
    workspace_id, canonical_workspace_root, workspace_generation, provider, mode,
    status, dispatch_state, created_at, updated_at
) VALUES (
    'execution-pending-v11', 'agent-pending-v11', 'request-pending-v11',
    'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
    'pending prompt', '{}', 'workspace-v11', 'C:/pending-v11', 2,
    'codex', 'read_only', 'dispatch_pending', 'not_dispatched', 101, 101
);
INSERT INTO executions (
    id, agent_id, request_key, request_hash, prompt, execution_profile_json,
    workspace_id, canonical_workspace_root, workspace_generation, provider, mode,
    runtime_instance_id, thread_id, status, dispatch_state, revision,
    release_evidence_state, activity_summary_code, created_at, updated_at
) VALUES (
    'execution-unknown-v11', 'agent-unknown-v11', 'request-unknown-v11',
    'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
    'unknown prompt', '{"retry":false}', 'workspace-macos-v11', '/fixture-v11', 3,
    'codex', 'workspace_write', 'runtime-macos-v11', 'thread-unknown-v11',
    'unknown', 'uncertain', 4, 'incomplete', 'execution.reconciling', 102, 180
);

INSERT INTO workspace_claims (canonical_workspace_root, execution_id, claim_type, acquired_at)
VALUES ('/fixture-v11', 'execution-unknown-v11', 'exclusive_execution', 130);
INSERT INTO work_runs (
    id, workspace_id, canonical_workspace_root, workspace_generation,
    title, goal, status, revision, acceptance_json, created_at, updated_at, completed_at
) VALUES (
    'work-v11', 'workspace-v11', 'C:/fixture-v11', 2,
    'Frozen v11 work', 'migration baseline', 'completed', 3,
    '{"accepted":true}', 105, 260, 260
);
INSERT INTO work_execution_links (
    work_run_id, execution_id, parent_execution_id, delegation_context_json, created_at
) VALUES ('work-v11', 'execution-completed-v11', NULL, '{"role":"general"}', 106);

INSERT INTO execution_usage (
    execution_id, provider_id, input_tokens, cached_input_tokens,
    cache_write_input_tokens, output_tokens, reasoning_tokens, total_tokens,
    model_context_window, completeness, usage_revision, updated_at
) VALUES (
    'execution-completed-v11', 'codex', 12, 3, NULL, 5, 2, 17,
    32000, 'complete', 4, 245
);
INSERT INTO codex_thread_usage_epochs (
    runtime_instance_id, thread_id, latest_cumulative_json, latest_turn_id, captured_at
) VALUES ('runtime-windows-v11', 'thread-v11', '{"totalTokens":17}', 'turn-v11', 244);
INSERT INTO codex_execution_usage_state (
    execution_id, runtime_instance_id, thread_id, turn_id, baseline_kind,
    baseline_json, latest_cumulative_json, telemetry_state,
    terminal_at, freeze_at, last_event_at
) VALUES (
    'execution-completed-v11', 'runtime-windows-v11', 'thread-v11', 'turn-v11',
    'observed_same_epoch', '{"totalTokens":2}', '{"totalTokens":17}',
    'frozen', 250, 251, 244
);

INSERT INTO command_runs (
    id, request_key, request_hash, workspace_id, canonical_workspace_root,
    workspace_generation, mode, relative_cwd, execution_mode, timeout_ms,
    status, revision, runtime_platform, containment_type, pid, started_at,
    completed_at, exit_code, timed_out, termination_reason, stdout_total_bytes,
    stderr_total_bytes, stdout_sha256, stderr_sha256, created_at, updated_at
) VALUES (
    'command-v11', 'command-request-v11',
    'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
    'workspace-v11', 'C:/fixture-v11', 2, 'process', '.', 'sync', 30000,
    'completed', 2, 'windows', 'windows_job', 610, 150,
    160, 0, 0, 'exited', 5, 0,
    '2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824',
    NULL, 140, 160
);
INSERT INTO work_command_links (work_run_id, command_run_id, created_at)
VALUES ('work-v11', 'command-v11', 141);
