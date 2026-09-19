-- v9 只增加 Usage 持久化边界；历史 Execution 不回填 Usage 或 Provider-private 状态。
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

-- Provider-private epoch 快照，不承担同步 checkpoint 或 terminal coverage 语义。
CREATE TABLE codex_thread_usage_epochs (
    runtime_instance_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    latest_cumulative_json TEXT NOT NULL,
    latest_turn_id TEXT,
    captured_at INTEGER NOT NULL,
    PRIMARY KEY(runtime_instance_id, thread_id)
);

-- Provider-private Execution 状态；公共 Product 不读取其中的 identity 或 baseline。
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
