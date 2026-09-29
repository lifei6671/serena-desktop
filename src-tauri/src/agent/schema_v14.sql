-- v14 只增加 Provider 已确认的实际执行配置；历史 Execution 保持 NULL。
ALTER TABLE executions ADD COLUMN effective_execution_profile_json TEXT;
