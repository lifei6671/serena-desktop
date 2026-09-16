-- v6 adds only generic Control Plane continuation lineage. Existing Provider
-- runtime identities and persisted request hashes are retained; historical rows
-- keep NULL parent_execution_id and no parent is inferred.
ALTER TABLE executions ADD COLUMN parent_execution_id TEXT;
