-- v8 adds only Activity v2 current-summary storage and its bounded history.
-- Historical rows receive a deterministic current summary but never an invented
-- timeline event; their activity_sequence therefore remains the zero default.
ALTER TABLE executions ADD COLUMN activity_summary_code TEXT;
ALTER TABLE executions ADD COLUMN activity_sequence INTEGER NOT NULL DEFAULT 0
    CHECK(activity_sequence >= 0);

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

-- SQLite RAISE is trigger-only. This transient trigger makes malformed legacy
-- Activity pairs abort the enclosing v8 transaction rather than guessing a code.
CREATE TRIGGER validate_v8_activity_backfill
BEFORE UPDATE OF activity_summary_code ON executions
WHEN NOT (
        NEW.status = 'finalizing'
        OR NEW.status IN ('reconciling', 'unknown')
        OR (NEW.status = 'dispatch_pending' AND NEW.dispatch_state = 'uncertain')
    )
    AND NOT (
        NEW.activity_phase IS NULL
        OR (NEW.activity_phase = 'provider' AND NEW.tool_category IS NULL)
        OR (NEW.activity_phase = 'tool' AND NEW.tool_category IS NOT NULL AND NEW.tool_category IN
            ('read', 'edit', 'command', 'build', 'test', 'tool'))
    )
BEGIN
    SELECT RAISE(ABORT, 'AGENT_ACTIVITY_CONTRACT_ERROR');
END;

-- This CASE is the persisted status/dispatch_state projection already frozen in
-- Product, followed by the P3-001 summaryCode allowlist in priority order.
UPDATE executions
SET activity_summary_code = CASE
    WHEN status = 'finalizing' THEN 'execution.finalizing'
    WHEN status IN ('reconciling', 'unknown')
        OR (status = 'dispatch_pending' AND dispatch_state = 'uncertain')
        THEN 'execution.reconciling'
    WHEN activity_phase IS NULL THEN NULL
    WHEN activity_phase = 'provider' AND tool_category IS NULL THEN 'provider.processing'
    WHEN activity_phase = 'tool' AND tool_category = 'read' THEN 'tool.read'
    WHEN activity_phase = 'tool' AND tool_category = 'edit' THEN 'tool.edit'
    WHEN activity_phase = 'tool' AND tool_category = 'command' THEN 'tool.command'
    WHEN activity_phase = 'tool' AND tool_category = 'build' THEN 'tool.build'
    WHEN activity_phase = 'tool' AND tool_category = 'test' THEN 'tool.test'
    WHEN activity_phase = 'tool' AND tool_category = 'tool' THEN 'tool.other'
END;

-- Validation is a migration-only guard, not a new runtime mutation contract.
DROP TRIGGER validate_v8_activity_backfill;
