ALTER TABLE executions ADD COLUMN last_activity_at INTEGER;
ALTER TABLE executions ADD COLUMN activity_phase TEXT
    CHECK(activity_phase IN ('provider','tool'));
ALTER TABLE executions ADD COLUMN tool_category TEXT
    CHECK(tool_category IN ('build','test','command','read','edit','tool'));
