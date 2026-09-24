-- v10 为共享 Runtime 行增加显式平台与 containment 判别；历史行保持 Windows Job 语义。
ALTER TABLE runtime_instances ADD COLUMN runtime_platform TEXT NOT NULL DEFAULT 'windows'
    CHECK(runtime_platform IN ('windows','macos'));
ALTER TABLE runtime_instances ADD COLUMN containment_type TEXT NOT NULL DEFAULT 'windows_job'
    CHECK(containment_type IN ('windows_job','macos_process_group'));
ALTER TABLE runtime_instances ADD COLUMN process_identity_scheme TEXT NOT NULL DEFAULT 'windows_filetime_v1'
    CHECK(process_identity_scheme IN ('windows_filetime_v1','darwin_proc_bsd_start_v1'));
ALTER TABLE runtime_instances ADD COLUMN containment_process_group_id INTEGER;
ALTER TABLE runtime_instances ADD COLUMN containment_session_id INTEGER;
ALTER TABLE runtime_instances ADD COLUMN containment_verified_at INTEGER;

-- INSERT 和 UPDATE 共用同一组条件，确保平台私有字段不会形成错误等价关系。
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
                NEW.codex_pid IS NULL
                AND NEW.codex_process_start_token IS NULL
                AND NEW.containment_process_group_id IS NULL
                AND NEW.containment_session_id IS NULL
                AND NEW.containment_verified_at IS NULL
            )
            OR (
                NEW.codex_pid IS NOT NULL
                AND NEW.codex_process_start_token IS NOT NULL
                AND NEW.containment_process_group_id IS NOT NULL
                AND NEW.containment_session_id IS NOT NULL
                AND NEW.containment_verified_at IS NOT NULL
                AND NEW.codex_pid = NEW.containment_process_group_id
                AND NEW.codex_pid = NEW.containment_session_id
            )
        )
        AND (
            NEW.state IN ('preparing','unknown')
            OR (
                NEW.codex_pid IS NOT NULL
                AND NEW.codex_process_start_token IS NOT NULL
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
    codex_pid, codex_process_start_token, state,
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
                NEW.codex_pid IS NULL
                AND NEW.codex_process_start_token IS NULL
                AND NEW.containment_process_group_id IS NULL
                AND NEW.containment_session_id IS NULL
                AND NEW.containment_verified_at IS NULL
            )
            OR (
                NEW.codex_pid IS NOT NULL
                AND NEW.codex_process_start_token IS NOT NULL
                AND NEW.containment_process_group_id IS NOT NULL
                AND NEW.containment_session_id IS NOT NULL
                AND NEW.containment_verified_at IS NOT NULL
                AND NEW.codex_pid = NEW.containment_process_group_id
                AND NEW.codex_pid = NEW.containment_session_id
            )
        )
        AND (
            NEW.state IN ('preparing','unknown')
            OR (
                NEW.codex_pid IS NOT NULL
                AND NEW.codex_process_start_token IS NOT NULL
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

-- 强制所有历史行经过新触发器；异常将回滚本 migration 的列、触发器和版本号。
UPDATE runtime_instances SET runtime_platform = runtime_platform;
