use rmcp::schemars;
use serde::{Deserialize, Deserializer, Serialize, de};

pub(crate) mod control;
pub mod port;
pub mod registry;
pub mod telemetry;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, rmcp::schemars::JsonSchema)]
#[serde(transparent)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(value: String) -> Result<Self, String> {
        validate_provider_id(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ProviderId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

fn validate_provider_id(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("provider id must not be empty".into());
    }
    if value
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err("provider id must not contain whitespace or control characters".into());
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderDescriptor {
    pub id: ProviderId,
    pub display_name: String,
    pub version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub can_execute: bool,
    pub can_continue: bool,
    pub can_cancel: bool,
    pub can_recover: bool,
    pub activity: bool,
    pub token_usage: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderExecutionContext {
    pub execution_id: String,
}

/// Provider 配置目录查询所需的已解析工作目录；Workspace authority 由 Product 层冻结。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderConfigurationCatalogContext {
    pub cwd: String,
}

/// Provider-neutral 的可选值。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionConfigurationOption {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// 模型及其专属推理选项；hidden 只表达 Provider 目录事实。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionModelOption {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub is_default: bool,
    pub hidden: bool,
    pub reasoning_options: Vec<ExecutionConfigurationOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_reasoning: Option<String>,
}

/// 与普通 Provider health 目录分离的只读执行配置目录。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionConfigurationCatalog {
    pub provider_id: ProviderId,
    pub models: Vec<ExecutionModelOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    pub reasoning_options: Vec<ExecutionConfigurationOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_reasoning: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderCancelContext {
    pub execution_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderStartupContext {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[expect(
    clippy::enum_variant_names,
    reason = "六个变体名称与冻结的 AGENT_PROVIDER_* wire 错误码一一对应。"
)]
pub enum ProviderErrorCode {
    AgentProviderNotFound,
    AgentProviderDisabled,
    AgentProviderUnavailable,
    AgentProviderCapabilityUnsupported,
    AgentProviderContractError,
    AgentProviderOperationFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderError {
    pub code: ProviderErrorCode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderOutcome {
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderResultCompleteness {
    Unknown,
    Partial,
    Complete,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderRunResult {
    pub execution_id: String,
    pub outcome: ProviderOutcome,
    pub result: Option<serde_json::Value>,
    pub result_completeness: ProviderResultCompleteness,
    pub diagnostic_code: Option<String>,
}

#[cfg(test)]
mod tests;
