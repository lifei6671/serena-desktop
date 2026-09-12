mod manager;
#[allow(
    dead_code,
    reason = "ngrok persistence is intentionally not wired to UI or runtime in this unit"
)]
pub(crate) mod ngrok_store;
#[allow(dead_code, reason = "ngrok transport staged before lifecycle wiring")]
pub(crate) mod ngrok_tunnel;
mod process;
mod quick_tunnel;
pub use manager::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAccessMode {
    QuickTunnel,
    #[serde(rename = "self_hosted_oauth")]
    SelfHostedOAuth,
    #[default]
    McpOnly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelfHostedProvider {
    #[default]
    CustomHttps,
    Ngrok,
    TailscaleFunnel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityDeclaration {
    #[default]
    ExternalAuth,
    None,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RemoteAccessConfig {
    pub mode: RemoteAccessMode,
    pub self_hosted: SelfHostedConfig,
    pub mcp_only: McpOnlyConfig,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SelfHostedConfig {
    pub provider: SelfHostedProvider,
    pub public_origin: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct McpOnlyConfig {
    pub security_declaration: SecurityDeclaration,
    pub public_origin: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{RemoteAccessMode, SelfHostedProvider};

    #[test]
    fn mode_wire_names_match_frontend_contract() {
        for (name, mode) in [
            ("quick_tunnel", RemoteAccessMode::QuickTunnel),
            ("self_hosted_oauth", RemoteAccessMode::SelfHostedOAuth),
            ("mcp_only", RemoteAccessMode::McpOnly),
        ] {
            let value = serde_json::json!(name);
            assert_eq!(
                serde_json::from_value::<RemoteAccessMode>(value.clone()).unwrap(),
                mode
            );
            assert_eq!(serde_json::to_value(mode).unwrap(), value);
        }
    }

    #[test]
    fn self_hosted_provider_wire_names_round_trip() {
        for (name, provider) in [
            ("custom_https", SelfHostedProvider::CustomHttps),
            ("ngrok", SelfHostedProvider::Ngrok),
            ("tailscale_funnel", SelfHostedProvider::TailscaleFunnel),
        ] {
            let value = serde_json::json!(name);
            assert_eq!(
                serde_json::from_value::<SelfHostedProvider>(value.clone()).unwrap(),
                provider
            );
            assert_eq!(serde_json::to_value(provider).unwrap(), value);
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum McpAuthPolicy {
    EmbeddedOAuth,
    #[default]
    Passthrough,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePublicContext {
    pub public_origin: String,
    pub mcp_resource: String,
    pub instance_id: String,
}
impl RemotePublicContext {
    pub fn new(origin: &str) -> Result<Self, String> {
        let public_origin = validate_https_origin(origin)?;
        Ok(Self {
            mcp_resource: format!("{public_origin}/mcp"),
            public_origin,
            instance_id: crate::oauth::random_id().map_err(|e| e.1)?,
        })
    }
}

// Shared input validation only; an Origin does not imply an OAuth runtime.
pub(crate) fn validate_https_origin(origin: &str) -> Result<String, String> {
    let url = url::Url::parse(origin).map_err(|_| "PUBLIC_ORIGIN_INVALID")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("PUBLIC_ORIGIN_INVALID".into());
    }
    Ok(url.origin().ascii_serialization())
}
