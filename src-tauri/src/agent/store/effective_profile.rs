use super::StateStore;
use crate::agent::execution::ExecutionProfile;
use rusqlite::{OptionalExtension, params};

impl StateStore {
    /// 为 exact Execution/Runtime 首次保存 Provider 已确认的实际执行配置。
    pub(crate) async fn set_effective_execution_profile(
        &self,
        execution_id: String,
        provider: String,
        runtime_id: String,
        profile: ExecutionProfile,
    ) -> Result<(), String> {
        profile
            .validate()
            .map_err(|_| "EFFECTIVE_EXECUTION_PROFILE_INVALID".to_string())?;
        if profile.model.is_none() && profile.reasoning.is_none() {
            return Err("EFFECTIVE_EXECUTION_PROFILE_INCOMPLETE".into());
        }
        let mut profile_value = profile.to_value();
        // Codex 未指定 reasoning 也是执行事实，显式保存 null；其他 Provider 保持稀疏表示。
        if provider == "codex" && profile.reasoning.is_none() {
            profile_value["reasoning"] = serde_json::Value::Null;
        }
        let profile_json = serde_json::to_string(&profile_value)
            .map_err(|_| "EFFECTIVE_EXECUTION_PROFILE_INVALID")?;

        self.write(move |transaction| {
            let execution: Option<(String, Option<String>, Option<String>)> = transaction
                .query_row(
                    "SELECT provider,runtime_instance_id,effective_execution_profile_json
                     FROM executions WHERE id=?1",
                    [&execution_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            let (execution_provider, execution_runtime, existing_json) =
                execution.ok_or("EXECUTION_NOT_FOUND")?;
            if execution_provider != provider {
                return Err("EFFECTIVE_EXECUTION_PROFILE_PROVIDER_MISMATCH".into());
            }
            if execution_runtime.as_deref() != Some(runtime_id.as_str()) {
                return Err("EFFECTIVE_EXECUTION_PROFILE_RUNTIME_MISMATCH".into());
            }
            let runtime_provider: Option<String> = transaction
                .query_row(
                    "SELECT provider FROM runtime_instances WHERE id=?1",
                    [&runtime_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            let runtime_provider =
                runtime_provider.ok_or("EFFECTIVE_EXECUTION_PROFILE_RUNTIME_NOT_FOUND")?;
            if runtime_provider != provider {
                return Err("EFFECTIVE_EXECUTION_PROFILE_RUNTIME_PROVIDER_MISMATCH".into());
            }

            if let Some(existing_json) = existing_json {
                let existing = ExecutionProfile::from_json(&existing_json)
                    .map_err(|_| "EFFECTIVE_EXECUTION_PROFILE_STORED_INVALID")?;
                if existing.model.is_none() && existing.reasoning.is_none() {
                    return Err("EFFECTIVE_EXECUTION_PROFILE_STORED_INVALID".into());
                }
                return if existing == profile {
                    Ok(())
                } else {
                    Err("EFFECTIVE_EXECUTION_PROFILE_CONFLICT".into())
                };
            }

            // CAS 只写 evidence 列；不触碰 revision、updated_at、状态或 Claim。
            let changed = transaction
                .execute(
                    "UPDATE executions SET effective_execution_profile_json=?2
                     WHERE id=?1 AND provider=?3 AND runtime_instance_id=?4
                       AND effective_execution_profile_json IS NULL",
                    params![execution_id, profile_json, provider, runtime_id],
                )
                .map_err(|error| error.to_string())?;
            if changed != 1 {
                return Err("EFFECTIVE_EXECUTION_PROFILE_CONFLICT".into());
            }
            Ok(())
        })
        .await
    }
}
