use rmcp::schemars::{self, JsonSchema};
use serde::Serialize;

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
