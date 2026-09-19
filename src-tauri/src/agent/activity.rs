use rmcp::schemars::{self, JsonSchema};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const ACTIVITY_QUIET_AFTER_MS: i64 = 30_000;
pub const ACTIVITY_PROLONGED_AFTER_MS: i64 = 120_000;
/// 表示无法从非法 Activity 组合安全派生摘要的稳定错误码。
pub const AGENT_ACTIVITY_CONTRACT_ERROR: &str = "AGENT_ACTIVITY_CONTRACT_ERROR";

/// 表示执行进度，用于在 Activity 映射前固定终结中间态的优先级。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProgressPhase {
    Pending,
    Dispatching,
    Running,
    Finalizing,
    Reconciling,
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivitySilence {
    Fresh,
    Quiet,
    Prolonged,
}

impl ActivitySilence {
    pub const fn from_activity_age_ms(activity_age_ms: Option<i64>) -> Option<Self> {
        match activity_age_ms {
            None => None,
            Some(age) if age < ACTIVITY_QUIET_AFTER_MS => Some(Self::Fresh),
            Some(age) if age < ACTIVITY_PROLONGED_AFTER_MS => Some(Self::Quiet),
            Some(_) => Some(Self::Prolonged),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivityPhase {
    Provider,
    Tool,
}

impl ActivityPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::Tool => "tool",
        }
    }
}

impl TryFrom<&str> for ActivityPhase {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "provider" => Ok(Self::Provider),
            "tool" => Ok(Self::Tool),
            _ => Err(format!("Invalid persisted activity phase: {value}")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    Build,
    Test,
    Command,
    Read,
    Edit,
    Tool,
}

impl ToolCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Test => "test",
            Self::Command => "command",
            Self::Read => "read",
            Self::Edit => "edit",
            Self::Tool => "tool",
        }
    }
}

impl TryFrom<&str> for ToolCategory {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "build" => Ok(Self::Build),
            "test" => Ok(Self::Test),
            "command" => Ok(Self::Command),
            "read" => Ok(Self::Read),
            "edit" => Ok(Self::Edit),
            "tool" => Ok(Self::Tool),
            _ => Err(format!("Invalid persisted tool category: {value}")),
        }
    }
}

/// 按冻结优先级从进度与 Activity 派生唯一的安全 summaryCode。
pub const fn derive_summary_code(
    progress_phase: ProgressPhase,
    activity_phase: Option<ActivityPhase>,
    tool_category: Option<ToolCategory>,
) -> Result<Option<&'static str>, &'static str> {
    // Finalizing 与 Reconciling 必须先覆盖下层 Activity，包含非法的持久化组合。
    match progress_phase {
        ProgressPhase::Finalizing => return Ok(Some("execution.finalizing")),
        ProgressPhase::Reconciling => return Ok(Some("execution.reconciling")),
        _ => {}
    }

    match (activity_phase, tool_category) {
        (None, _) => Ok(None),
        (Some(ActivityPhase::Provider), None) => Ok(Some("provider.processing")),
        (Some(ActivityPhase::Tool), Some(ToolCategory::Read)) => Ok(Some("tool.read")),
        (Some(ActivityPhase::Tool), Some(ToolCategory::Edit)) => Ok(Some("tool.edit")),
        (Some(ActivityPhase::Tool), Some(ToolCategory::Command)) => Ok(Some("tool.command")),
        (Some(ActivityPhase::Tool), Some(ToolCategory::Build)) => Ok(Some("tool.build")),
        (Some(ActivityPhase::Tool), Some(ToolCategory::Test)) => Ok(Some("tool.test")),
        (Some(ActivityPhase::Tool), Some(ToolCategory::Tool)) => Ok(Some("tool.other")),
        (Some(ActivityPhase::Provider), Some(_)) | (Some(ActivityPhase::Tool), None) => {
            Err(AGENT_ACTIVITY_CONTRACT_ERROR)
        }
    }
}

/// 按固定 JSON 数组与 SHA-256 派生 Activity Revision v2，只编码语义元组。
pub fn derive_activity_revision(
    execution_id: &str,
    activity_phase: Option<ActivityPhase>,
    tool_category: Option<ToolCategory>,
    summary_code: Option<&str>,
) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(
        "agent-activity-v2",
        execution_id,
        activity_phase.map(ActivityPhase::as_str),
        tool_category.map(ToolCategory::as_str),
        summary_code,
    ))
    .map_err(|error| error.to_string())?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 验证静默等级的既有边界保持不变。
    #[test]
    fn silence_level_uses_fixed_activity_age_boundaries() {
        for (age, expected) in [
            (None, None),
            (Some(0), Some(ActivitySilence::Fresh)),
            (
                Some(ACTIVITY_QUIET_AFTER_MS - 1),
                Some(ActivitySilence::Fresh),
            ),
            (Some(ACTIVITY_QUIET_AFTER_MS), Some(ActivitySilence::Quiet)),
            (
                Some(ACTIVITY_PROLONGED_AFTER_MS - 1),
                Some(ActivitySilence::Quiet),
            ),
            (
                Some(ACTIVITY_PROLONGED_AFTER_MS),
                Some(ActivitySilence::Prolonged),
            ),
            (
                Some(ACTIVITY_PROLONGED_AFTER_MS + 1),
                Some(ActivitySilence::Prolonged),
            ),
        ] {
            assert_eq!(ActivitySilence::from_activity_age_ms(age), expected);
        }
    }

    /// 穷举全部进度、Activity 与工具类别组合，确保每组输入只有一个冻结结果。
    #[test]
    fn summary_code_mapping_is_deterministic_for_every_input_combination() {
        let activity_cases = [
            (None, None, Ok(None)),
            (None, Some(ToolCategory::Read), Ok(None)),
            (None, Some(ToolCategory::Edit), Ok(None)),
            (None, Some(ToolCategory::Command), Ok(None)),
            (None, Some(ToolCategory::Build), Ok(None)),
            (None, Some(ToolCategory::Test), Ok(None)),
            (None, Some(ToolCategory::Tool), Ok(None)),
            (
                Some(ActivityPhase::Provider),
                None,
                Ok(Some("provider.processing")),
            ),
            (
                Some(ActivityPhase::Provider),
                Some(ToolCategory::Read),
                Err(AGENT_ACTIVITY_CONTRACT_ERROR),
            ),
            (
                Some(ActivityPhase::Provider),
                Some(ToolCategory::Edit),
                Err(AGENT_ACTIVITY_CONTRACT_ERROR),
            ),
            (
                Some(ActivityPhase::Provider),
                Some(ToolCategory::Command),
                Err(AGENT_ACTIVITY_CONTRACT_ERROR),
            ),
            (
                Some(ActivityPhase::Provider),
                Some(ToolCategory::Build),
                Err(AGENT_ACTIVITY_CONTRACT_ERROR),
            ),
            (
                Some(ActivityPhase::Provider),
                Some(ToolCategory::Test),
                Err(AGENT_ACTIVITY_CONTRACT_ERROR),
            ),
            (
                Some(ActivityPhase::Provider),
                Some(ToolCategory::Tool),
                Err(AGENT_ACTIVITY_CONTRACT_ERROR),
            ),
            (
                Some(ActivityPhase::Tool),
                None,
                Err(AGENT_ACTIVITY_CONTRACT_ERROR),
            ),
            (
                Some(ActivityPhase::Tool),
                Some(ToolCategory::Read),
                Ok(Some("tool.read")),
            ),
            (
                Some(ActivityPhase::Tool),
                Some(ToolCategory::Edit),
                Ok(Some("tool.edit")),
            ),
            (
                Some(ActivityPhase::Tool),
                Some(ToolCategory::Command),
                Ok(Some("tool.command")),
            ),
            (
                Some(ActivityPhase::Tool),
                Some(ToolCategory::Build),
                Ok(Some("tool.build")),
            ),
            (
                Some(ActivityPhase::Tool),
                Some(ToolCategory::Test),
                Ok(Some("tool.test")),
            ),
            (
                Some(ActivityPhase::Tool),
                Some(ToolCategory::Tool),
                Ok(Some("tool.other")),
            ),
        ];
        let mut case_count = 0;

        for progress_phase in [
            ProgressPhase::Pending,
            ProgressPhase::Dispatching,
            ProgressPhase::Running,
            ProgressPhase::Finalizing,
            ProgressPhase::Reconciling,
            ProgressPhase::Terminal,
        ] {
            for (activity_phase, tool_category, activity_expected) in activity_cases {
                let expected = match progress_phase {
                    ProgressPhase::Finalizing => Ok(Some("execution.finalizing")),
                    ProgressPhase::Reconciling => Ok(Some("execution.reconciling")),
                    _ => activity_expected,
                };
                assert_eq!(
                    derive_summary_code(progress_phase, activity_phase, tool_category),
                    expected,
                    "progress={progress_phase:?}, activity={activity_phase:?}, tool={tool_category:?}"
                );
                case_count += 1;
            }
        }

        assert_eq!(case_count, 126);
    }

    /// 验证高优先级进度覆盖所有下层合法及非法 Activity 组合。
    #[test]
    fn finalizing_and_reconciling_override_every_activity_combination() {
        let activity_phases = [
            None,
            Some(ActivityPhase::Provider),
            Some(ActivityPhase::Tool),
        ];
        let tool_categories = [
            None,
            Some(ToolCategory::Read),
            Some(ToolCategory::Edit),
            Some(ToolCategory::Command),
            Some(ToolCategory::Build),
            Some(ToolCategory::Test),
            Some(ToolCategory::Tool),
        ];
        let mut case_count = 0;

        for (progress_phase, expected) in [
            (ProgressPhase::Finalizing, "execution.finalizing"),
            (ProgressPhase::Reconciling, "execution.reconciling"),
        ] {
            for activity_phase in activity_phases {
                for tool_category in tool_categories {
                    assert_eq!(
                        derive_summary_code(progress_phase, activity_phase, tool_category),
                        Ok(Some(expected))
                    );
                    case_count += 1;
                }
            }
        }

        assert_eq!(case_count, 42);
    }

    /// 验证非覆盖进度在无 Activity 时不生成摘要。
    #[test]
    fn non_overriding_progress_without_activity_returns_none() {
        for progress_phase in [
            ProgressPhase::Pending,
            ProgressPhase::Dispatching,
            ProgressPhase::Running,
            ProgressPhase::Terminal,
        ] {
            assert_eq!(derive_summary_code(progress_phase, None, None), Ok(None));
        }
    }

    /// 验证两个非法 Activity 组合都返回稳定错误码。
    #[test]
    fn invalid_activity_combinations_return_the_stable_contract_error() {
        for progress_phase in [
            ProgressPhase::Pending,
            ProgressPhase::Dispatching,
            ProgressPhase::Running,
            ProgressPhase::Terminal,
        ] {
            for tool_category in [
                ToolCategory::Read,
                ToolCategory::Edit,
                ToolCategory::Command,
                ToolCategory::Build,
                ToolCategory::Test,
                ToolCategory::Tool,
            ] {
                assert_eq!(
                    derive_summary_code(
                        progress_phase,
                        Some(ActivityPhase::Provider),
                        Some(tool_category)
                    ),
                    Err(AGENT_ACTIVITY_CONTRACT_ERROR)
                );
            }
            assert_eq!(
                derive_summary_code(progress_phase, Some(ActivityPhase::Tool), None),
                Err(AGENT_ACTIVITY_CONTRACT_ERROR)
            );
        }
    }

    /// 验证类型整理不改变既有 product 路径或 wire 序列化。
    #[test]
    fn progress_phase_serialization_and_product_reexport_remain_stable() {
        assert_eq!(
            serde_json::to_value(crate::agent::product::ProgressPhase::Finalizing).unwrap(),
            json!("finalizing")
        );
        assert_eq!(
            crate::agent::product::ProgressPhase::Terminal,
            ProgressPhase::Terminal
        );
    }

    /// 验证 v2 revision 只由冻结的语义 JSON 数组决定，并保留 JSON null。
    #[test]
    fn activity_revision_v2_uses_only_the_frozen_semantic_tuple() {
        assert_eq!(
            derive_activity_revision(
                "e",
                Some(ActivityPhase::Tool),
                Some(ToolCategory::Test),
                Some("tool.test"),
            )
            .unwrap(),
            "ded56df71bc1875c111bf734e82758961fa8858efc7a792da6ab6db4d1cbd176"
        );
        assert_eq!(
            derive_activity_revision("e", None, None, None).unwrap(),
            "f8870b107e96bc2c0ed7c65e0068786d4e1aa13b8fdfee9e742e7460b73a99f4"
        );
    }
}
