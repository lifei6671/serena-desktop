-- v7 freezes the Workspace Registry generation alongside the already persisted
-- workspace ID and canonical root. Historical rows define generation 1 as the
-- path-authority baseline; this migration never infers current registry state.
ALTER TABLE executions ADD COLUMN workspace_generation INTEGER NOT NULL DEFAULT 1 CHECK(workspace_generation >= 1);
ALTER TABLE work_runs ADD COLUMN workspace_generation INTEGER NOT NULL DEFAULT 1 CHECK(workspace_generation >= 1);
