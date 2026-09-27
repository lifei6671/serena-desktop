-- v13 仅保存 CodeBuddy 私有身份与检查来源，不回填历史 Execution。
CREATE TABLE codebuddy_execution_state (
    execution_id TEXT PRIMARY KEY NOT NULL REFERENCES executions(id) ON DELETE RESTRICT,
    runtime_instance_id TEXT REFERENCES runtime_instances(id) ON DELETE RESTRICT,
    acp_protocol_version INTEGER CHECK(acp_protocol_version IS NULL OR
        (typeof(acp_protocol_version) = 'integer' AND acp_protocol_version BETWEEN 0 AND 65535)),
    session_id TEXT CHECK(session_id IS NULL OR length(session_id) > 0),
    conversation_request_id TEXT NOT NULL CHECK(
        (length(conversation_request_id) = 32 AND conversation_request_id NOT GLOB '*[^0-9a-f]*'
         AND substr(conversation_request_id, 13, 1) = '7'
         AND substr(conversation_request_id, 17, 1) IN ('8','9','a','b'))),
    provider_request_id TEXT CHECK(provider_request_id IS NULL OR length(provider_request_id) > 0),
    provider_request_id_source TEXT,
    prompt_rpc_id TEXT CHECK(prompt_rpc_id IS NULL OR CASE WHEN json_valid(prompt_rpc_id)
        THEN json_type(prompt_rpc_id) = 'text' OR
             (json_type(prompt_rpc_id) = 'integer' AND typeof(json_extract(prompt_rpc_id, '$')) = 'integer')
        ELSE 0 END),
    prompt_state TEXT NOT NULL CHECK(prompt_state IN ('prepared','sent','uncertain','terminal_observed')),
    terminal_stop_reason TEXT CHECK(terminal_stop_reason IS NULL OR terminal_stop_reason IN
        ('end_turn','max_tokens','max_turn_requests','refusal','cancelled')),
    terminal_observed_at INTEGER,
    recovery_method TEXT CHECK(recovery_method IS NULL OR recovery_method = 'session/load'),
    recovery_state TEXT NOT NULL CHECK(recovery_state IN
        ('not_attempted','inspecting','partial','unknown','material_difference')),
    recovery_runtime_instance_id TEXT REFERENCES runtime_instances(id) ON DELETE RESTRICT,
    recovery_started_at INTEGER,
    recovery_finished_at INTEGER,
    revision INTEGER NOT NULL CHECK(typeof(revision) = 'integer' AND revision >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK((provider_request_id IS NULL AND provider_request_id_source IS NULL) OR
          (provider_request_id IS NOT NULL AND provider_request_id_source IS NOT NULL
           AND provider_request_id_source = 'exact_provider_observation')),
    CHECK(prompt_state = 'prepared' OR
          (runtime_instance_id IS NOT NULL AND acp_protocol_version IS NOT NULL AND session_id IS NOT NULL)),
    CHECK(prompt_state != 'prepared' OR prompt_rpc_id IS NULL),
    CHECK((prompt_state = 'terminal_observed' AND terminal_stop_reason IS NOT NULL AND terminal_observed_at IS NOT NULL)
       OR (prompt_state != 'terminal_observed' AND terminal_stop_reason IS NULL AND terminal_observed_at IS NULL)),
    CHECK((recovery_state = 'not_attempted' AND recovery_method IS NULL AND recovery_runtime_instance_id IS NULL
           AND recovery_started_at IS NULL AND recovery_finished_at IS NULL)
       OR (recovery_state != 'not_attempted' AND recovery_method IS NOT NULL AND recovery_runtime_instance_id IS NOT NULL
           AND recovery_started_at IS NOT NULL AND prompt_state != 'prepared'
           AND ((recovery_state = 'inspecting' AND recovery_finished_at IS NULL)
             OR (recovery_state != 'inspecting' AND recovery_finished_at IS NOT NULL))))
);

-- RPC id 仅属原连接；唯一认领只针对已知的 exact Session / Prompt pair。
CREATE UNIQUE INDEX codebuddy_execution_target_prompt
ON codebuddy_execution_state(session_id, conversation_request_id)
WHERE session_id IS NOT NULL AND conversation_request_id IS NOT NULL;
