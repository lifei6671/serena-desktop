use rmcp::schemars::{self, JsonSchema};
use serde::Serialize;

pub const ACTIVITY_QUIET_AFTER_MS: i64 = 30_000;
pub const ACTIVITY_PROLONGED_AFTER_MS: i64 = 120_000;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
