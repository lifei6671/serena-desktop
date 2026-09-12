-- Reserve before Runtime creation; runtime_instance_id intentionally has no FK.
CREATE TABLE execution_runtime_attempts (
    execution_id TEXT NOT NULL REFERENCES executions(id),
    runtime_instance_id TEXT PRIMARY KEY NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX execution_runtime_attempts_execution ON execution_runtime_attempts(execution_id);
CREATE TRIGGER execution_runtime_attempts_immutable_update BEFORE UPDATE ON execution_runtime_attempts
BEGIN SELECT RAISE(ABORT, 'RUNTIME_ATTEMPT_IMMUTABLE'); END;
CREATE TRIGGER execution_runtime_attempts_immutable_delete BEFORE DELETE ON execution_runtime_attempts
BEGIN SELECT RAISE(ABORT, 'RUNTIME_ATTEMPT_IMMUTABLE'); END;
