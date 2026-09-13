use super::cimd::ClientMetadata;
use crate::remote::RemotePublicContext;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

mod persistence;

const AUTH_TTL: Duration = Duration::from_secs(120);
const ACCESS_TTL: Duration = Duration::from_secs(3600);
const GRANT_TTL: Duration = Duration::from_secs(72 * 60 * 60);
const CLIENT_TTL: Duration = Duration::from_secs(86400);
const MAX_CLIENTS: usize = 256;
const MAX_PENDING: usize = 32;
const MAX_CREDENTIALS: usize = 4096;

pub fn random_id() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| OAuthError("server_error", "OAUTH_RANDOM_FAILED"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
fn hash(text: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(text.as_bytes()))
}

#[derive(Debug)]
pub struct OAuthError(pub &'static str, pub &'static str);
type Result<T> = std::result::Result<T, OAuthError>;
fn invalid(code: &'static str) -> OAuthError {
    OAuthError("invalid_request", code)
}

pub(super) fn validate_redirect(text: &str) -> Result<()> {
    let url = url::Url::parse(text).map_err(|_| invalid("OAUTH_REDIRECT_URI_INVALID"))?;
    let loopback = match url.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(ip)) => ip == std::net::Ipv4Addr::LOCALHOST,
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if text.len() > 2048
        || url.host_str().is_none()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
    {
        return Err(invalid("OAUTH_REDIRECT_URI_INVALID"));
    }
    Ok(())
}

struct Client {
    name: String,
    redirects: Vec<String>,
    refresh_allowed: bool,
    expires: Instant,
}
#[derive(Clone)]
struct ClientSnapshot {
    name: String,
    redirects: Vec<String>,
    refresh_allowed: bool,
}
#[derive(Clone, Deserialize, PartialEq, Eq)]
pub struct Authorization {
    pub client_id: String,
    pub redirect_uri: String,
    pub resource: String,
    pub response_type: String,
    pub code_challenge: String,
    pub code_challenge_method: String,
    #[serde(default)]
    pub state: String,
    #[serde(default = "default_scope")]
    pub scope: String,
}
fn default_scope() -> String {
    "serena:mcp".into()
}
fn validate_authorization_state(request: &Authorization) -> Result<()> {
    if request.state.len() > 2048 || request.state.chars().any(char::is_control) {
        return Err(invalid("OAUTH_STATE_INVALID"));
    }
    Ok(())
}
fn validate_authorization_pkce(request: &Authorization) -> Result<()> {
    if request.code_challenge_method != "S256"
        || URL_SAFE_NO_PAD
            .decode(&request.code_challenge)
            .map_or(true, |value| value.len() != 32)
    {
        return Err(invalid("OAUTH_PKCE_INVALID"));
    }
    Ok(())
}
fn client_id_hostname(client_id: &str) -> Option<String> {
    let url = url::Url::parse(client_id).ok()?;
    if url.scheme() == "https" {
        url.host_str().map(str::to_owned)
    } else {
        None
    }
}
struct Pending {
    request: Authorization,
    client_name: String,
    refresh_allowed: bool,
    confirmation: String,
    expires: Instant,
    // Redirect contains a one-use code only after local approval.
    outcome: Option<(bool, String)>,
}
struct Code {
    request: Authorization,
    refresh_allowed: bool,
    expires: Instant,
}
struct Access {
    family: String,
    expires: Instant,
}
struct Refresh {
    family: String,
    used: bool,
    expires: Instant,
}
struct Grant {
    client: String,
    scope: String,
    refresh_allowed: bool,
    expires: Instant,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingView {
    pub id: String,
    pub client_name: String,
    pub client_id_hostname: Option<String>,
    pub redirect_uri: String,
    pub confirmation_code: String,
    pub expires_in_seconds: u64,
    pub scope: String,
    pub refresh_allowed: bool,
    #[serde(skip)]
    pub(crate) wake_window: bool,
    #[serde(skip)]
    pub(crate) created: bool,
}

pub struct Runtime {
    storage: Option<persistence::Store>,
    storage_failed: bool,
    pending_created: std::collections::VecDeque<Instant>,
    last_wake: Option<Instant>,
    probe: Option<(String, Instant)>,
    pub context: RemotePublicContext,
    clients: HashMap<String, Client>,
    pending: HashMap<String, Pending>,
    codes: HashMap<String, Code>,
    used_codes: HashMap<String, Instant>,
    access: HashMap<String, Access>,
    refresh: HashMap<String, Refresh>,
    grants: HashMap<String, Grant>,
}
impl Runtime {
    pub fn new(context: RemotePublicContext) -> Self {
        Self {
            storage: None,
            storage_failed: false,
            pending_created: std::collections::VecDeque::new(),
            last_wake: None,
            probe: None,
            context,
            clients: HashMap::new(),
            pending: HashMap::new(),
            codes: HashMap::new(),
            used_codes: HashMap::new(),
            access: HashMap::new(),
            refresh: HashMap::new(),
            grants: HashMap::new(),
        }
    }
    fn prune(&mut self) {
        let now = Instant::now();
        self.pending.retain(|_, p| p.expires > now);
        self.codes.retain(|_, c| c.expires > now);
        self.used_codes.retain(|_, expires| *expires > now);
        self.grants.retain(|_, g| g.expires > now);
        self.access
            .retain(|_, a| a.expires > now && self.grants.contains_key(&a.family));
        self.refresh
            .retain(|_, r| r.expires > now && self.grants.contains_key(&r.family));
        self.clients.retain(|_, client| client.expires > now);
    }
    pub fn register(&mut self, value: Value) -> Result<Value> {
        self.ensure_storage()?;
        self.prune();
        if self.clients.len() >= MAX_CLIENTS {
            return Err(OAuthError(
                "temporarily_unavailable",
                "OAUTH_CAPACITY_EXCEEDED",
            ));
        }
        if value
            .get("token_endpoint_auth_method")
            .is_some_and(|v| v != "none")
        {
            return Err(invalid("OAUTH_CLIENT_INVALID"));
        }
        if let Some(grants) = value.get("grant_types") {
            let Some(grants) = grants.as_array() else {
                return Err(invalid("OAUTH_CLIENT_INVALID"));
            };
            if !grants.contains(&json!("authorization_code"))
                || grants
                    .iter()
                    .any(|g| g != "authorization_code" && g != "refresh_token")
            {
                return Err(invalid("OAUTH_CLIENT_INVALID"));
            }
        }
        // Preserve the existing default registration response (both flows), but
        // honor clients that explicitly register authorization_code only.
        let refresh_allowed = value
            .get("grant_types")
            .is_none_or(|grants| grants.as_array().unwrap().contains(&json!("refresh_token")));
        if value
            .get("response_types")
            .is_some_and(|v| v != &json!(["code"]))
        {
            return Err(invalid("OAUTH_CLIENT_INVALID"));
        }
        let redirects = value["redirect_uris"]
            .as_array()
            .ok_or(invalid("OAUTH_REDIRECT_URI_INVALID"))?;
        if redirects.is_empty() || redirects.len() > 8 {
            return Err(invalid("OAUTH_REDIRECT_URI_INVALID"));
        }
        let redirects = redirects
            .iter()
            .map(|v| {
                let text = v.as_str().ok_or(invalid("OAUTH_REDIRECT_URI_INVALID"))?;
                validate_redirect(text)?;
                Ok(text.to_owned())
            })
            .collect::<Result<Vec<_>>>()?;
        let name = value
            .get("client_name")
            .and_then(Value::as_str)
            .unwrap_or("未命名客户端");
        if name.len() > 200 || name.chars().any(char::is_control) {
            return Err(invalid("OAUTH_CLIENT_INVALID"));
        }
        let id = random_id()?;
        let grant_types = if refresh_allowed {
            json!(["authorization_code", "refresh_token"])
        } else {
            json!(["authorization_code"])
        };
        let response = json!({"client_id":id,"client_name":name,"redirect_uris":redirects,"token_endpoint_auth_method":"none","grant_types":grant_types,"response_types":["code"]});
        self.clients.insert(
            id,
            Client {
                name: name.into(),
                redirects,
                refresh_allowed,
                expires: Instant::now() + CLIENT_TTL,
            },
        );
        self.persist()?;
        Ok(response)
    }
    pub(crate) fn has_registered_client(&self, client_id: &str) -> bool {
        self.clients.contains_key(client_id)
    }
    pub(crate) fn preflight_cimd_authorization(&self, request: &Authorization) -> Result<()> {
        self.validate_authorization_resource(request)?;
        validate_authorization_state(request)?;
        validate_authorization_pkce(request)?;
        validate_redirect(&request.redirect_uri)
    }
    pub fn authorize(&mut self, request: Authorization) -> Result<PendingView> {
        self.ensure_storage()?;
        self.prune();
        let client = self
            .clients
            .get(&request.client_id)
            .map(|client| ClientSnapshot {
                name: client.name.clone(),
                redirects: client.redirects.clone(),
                refresh_allowed: client.refresh_allowed,
            })
            .ok_or(invalid("OAUTH_CLIENT_INVALID"))?;
        self.authorize_resolved(request, client)
    }
    pub(crate) fn authorize_cimd(
        &mut self,
        request: Authorization,
        metadata: ClientMetadata,
    ) -> Result<PendingView> {
        self.ensure_storage()?;
        self.prune();
        if metadata.client_id != request.client_id {
            return Err(invalid("OAUTH_CLIENT_INVALID"));
        }
        self.authorize_resolved(
            request,
            ClientSnapshot {
                name: metadata.client_name,
                redirects: metadata.redirect_uris,
                refresh_allowed: metadata.refresh_allowed,
            },
        )
    }
    fn authorize_resolved(
        &mut self,
        mut request: Authorization,
        client: ClientSnapshot,
    ) -> Result<PendingView> {
        if !client.redirects.contains(&request.redirect_uri) {
            return Err(invalid("OAUTH_REDIRECT_URI_INVALID"));
        }
        self.validate_authorization_resource(&request)?;
        // Validate reflected fields before any error eligible for redirection.
        validate_authorization_state(&request)?;
        if request.response_type != "code" {
            return Err(OAuthError(
                "unsupported_response_type",
                "OAUTH_CLIENT_INVALID",
            ));
        }
        validate_authorization_pkce(&request)?;
        let scopes: HashSet<_> = request.scope.split_whitespace().collect();
        if !scopes.contains("serena:mcp")
            || (scopes.contains("offline_access") && !client.refresh_allowed)
            || scopes
                .iter()
                .any(|s| !matches!(*s, "serena:mcp" | "offline_access"))
        {
            return Err(OAuthError("invalid_scope", "OAUTH_SCOPE_INVALID"));
        }
        request.scope = if scopes.contains("offline_access") {
            "serena:mcp offline_access"
        } else {
            "serena:mcp"
        }
        .into();
        // Only active browser requests share a confirmation flow.
        if let Some((id, pending)) = self.pending.iter().find(|(_, p)| {
            p.outcome.is_none()
                && p.request == request
                && p.client_name == client.name
                && p.refresh_allowed == client.refresh_allowed
        }) {
            return Ok(PendingView {
                id: id.clone(),
                client_name: client.name.clone(),
                client_id_hostname: client_id_hostname(&pending.request.client_id),
                redirect_uri: request.redirect_uri.clone(),
                confirmation_code: pending.confirmation.clone(),
                expires_in_seconds: pending
                    .expires
                    .saturating_duration_since(Instant::now())
                    .as_secs(),
                scope: request.scope.clone(),
                refresh_allowed: client.refresh_allowed,
                wake_window: false,
                created: false,
            });
        }
        let now = Instant::now();
        while self
            .pending_created
            .front()
            .is_some_and(|at| now.duration_since(*at) >= Duration::from_secs(10))
        {
            self.pending_created.pop_front();
        }
        if self.pending_created.len() >= 4 {
            return Err(OAuthError(
                "temporarily_unavailable",
                "OAUTH_AUTHORIZATION_THROTTLED",
            ));
        }
        if self.pending.len() >= MAX_PENDING
            || self.grants.len() >= MAX_CLIENTS
            || self.codes.len() + self.used_codes.len() >= MAX_CLIENTS
        {
            return Err(OAuthError(
                "temporarily_unavailable",
                "OAUTH_CAPACITY_EXCEEDED",
            ));
        }
        let id = random_id()?;
        let mut bytes = [0u8; 4];
        getrandom::fill(&mut bytes)
            .map_err(|_| OAuthError("server_error", "OAUTH_RANDOM_FAILED"))?;
        let confirmation = format!("{:06}", u32::from_le_bytes(bytes) % 1_000_000);
        let view = PendingView {
            id: id.clone(),
            client_name: client.name.clone(),
            client_id_hostname: client_id_hostname(&request.client_id),
            redirect_uri: request.redirect_uri.clone(),
            confirmation_code: confirmation.clone(),
            expires_in_seconds: 120,
            scope: request.scope.clone(),
            refresh_allowed: client.refresh_allowed,
            created: true,
            wake_window: self
                .last_wake
                .is_none_or(|at| now.duration_since(at) >= Duration::from_secs(5)),
        };
        self.pending_created.push_back(now);
        if view.wake_window {
            self.last_wake = Some(now);
        }
        self.pending.insert(
            id,
            Pending {
                request,
                client_name: client.name,
                refresh_allowed: client.refresh_allowed,
                confirmation,
                expires: Instant::now() + AUTH_TTL,
                outcome: None,
            },
        );
        Ok(view)
    }
    fn validate_authorization_resource(&self, request: &Authorization) -> Result<()> {
        if request.resource != self.context.mcp_resource {
            return Err(invalid("OAUTH_RESOURCE_INVALID"));
        }
        Ok(())
    }
    pub fn pending(&mut self) -> Vec<PendingView> {
        self.prune();
        let mut pending: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, p)| p.outcome.is_none())
            .collect();
        pending.sort_by_key(|(_, p)| p.expires);
        pending
            .into_iter()
            .map(|(id, p)| PendingView {
                id: id.clone(),
                client_name: p.client_name.clone(),
                client_id_hostname: client_id_hostname(&p.request.client_id),
                redirect_uri: p.request.redirect_uri.clone(),
                confirmation_code: p.confirmation.clone(),
                scope: p.request.scope.clone(),
                refresh_allowed: p.refresh_allowed,
                wake_window: false,
                created: false,
                expires_in_seconds: p
                    .expires
                    .saturating_duration_since(Instant::now())
                    .as_secs(),
            })
            .collect()
    }
    pub fn decide(&mut self, id: &str, allow: bool) -> Result<()> {
        self.ensure_storage()?;
        self.prune();
        let pending = self
            .pending
            .get_mut(id)
            .ok_or(invalid("OAUTH_AUTHORIZATION_EXPIRED"))?;
        if pending.outcome.is_some() {
            return Err(invalid("OAUTH_AUTHORIZATION_CONSUMED"));
        }
        let mut redirect = url::Url::parse(&pending.request.redirect_uri)
            .map_err(|_| invalid("OAUTH_REDIRECT_URI_INVALID"))?;
        if allow {
            let code = random_id()?;
            self.codes.insert(
                hash(&code),
                Code {
                    request: pending.request.clone(),
                    refresh_allowed: pending.refresh_allowed,
                    expires: Instant::now() + AUTH_TTL,
                },
            );
            redirect.query_pairs_mut().append_pair("code", &code);
        } else {
            redirect
                .query_pairs_mut()
                .append_pair("error", "access_denied");
        }
        if !pending.request.state.is_empty() {
            redirect
                .query_pairs_mut()
                .append_pair("state", &pending.request.state);
        }
        pending.outcome = Some((allow, redirect.into()));
        Ok(())
    }
    pub fn poll(&mut self, id: &str) -> Value {
        self.prune();
        match self.pending.get(id) {
            None => json!({"status":"expired"}),
            Some(Pending {
                outcome: Some((allow, redirect)),
                ..
            }) => json!({"status":if *allow {"approved"} else {"denied"},"redirect":redirect}),
            Some(_) => json!({"status":"pending"}),
        }
    }
    pub fn token(&mut self, fields: &HashMap<String, String>) -> Result<Value> {
        self.ensure_storage()?;
        self.prune();
        let get = |key: &str| fields.get(key).map(String::as_str).unwrap_or("");
        if get("resource") != self.context.mcp_resource {
            return Err(invalid("OAUTH_RESOURCE_INVALID"));
        }
        match get("grant_type") {
            "authorization_code" => {
                // Invalid client bindings consume once; temporary server capacity does not.
                let key = hash(get("code"));
                if self.used_codes.contains_key(&key) {
                    return Err(OAuthError("invalid_grant", "OAUTH_CODE_REPLAY"));
                }
                let code = self
                    .codes
                    .get(&key)
                    .ok_or(OAuthError("invalid_grant", "OAUTH_CODE_INVALID"))?;
                if code.request.client_id != get("client_id")
                    || code.request.redirect_uri != get("redirect_uri")
                {
                    let expires = code.expires;
                    self.codes.remove(&key);
                    self.used_codes.insert(key, expires);
                    return Err(OAuthError("invalid_grant", "OAUTH_CODE_INVALID"));
                }
                let verifier = get("code_verifier");
                if !(43..=128).contains(&verifier.len())
                    || !verifier
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
                    || hash(verifier) != code.request.code_challenge
                {
                    let expires = code.expires;
                    self.codes.remove(&key);
                    self.used_codes.insert(key, expires);
                    return Err(OAuthError("invalid_grant", "OAUTH_PKCE_INVALID"));
                }
                let refresh_allowed = code.refresh_allowed;
                if self.access.len() >= MAX_CREDENTIALS
                    || (refresh_allowed && self.refresh.len() >= MAX_CREDENTIALS)
                {
                    return Err(OAuthError(
                        "temporarily_unavailable",
                        "OAUTH_CAPACITY_EXCEEDED",
                    ));
                }
                let code = self.codes.remove(&key).expect("validated code");
                self.used_codes.insert(key, code.expires);
                let family = random_id()?;
                self.grants.insert(
                    family.clone(),
                    Grant {
                        client: code.request.client_id,
                        scope: code.request.scope,
                        refresh_allowed: code.refresh_allowed,
                        expires: Instant::now() + GRANT_TTL,
                    },
                );
                let result = self.issue(family);
                self.persist()?;
                result
            }
            "refresh_token" => {
                let key = hash(get("refresh_token"));
                let refresh = self
                    .refresh
                    .get(&key)
                    .ok_or(OAuthError("invalid_grant", "OAUTH_REFRESH_TOKEN_INVALID"))?;
                let family = refresh.family.clone();
                let grant = self
                    .grants
                    .get(&family)
                    .ok_or(OAuthError("invalid_grant", "OAUTH_REFRESH_TOKEN_INVALID"))?;
                if grant.client != get("client_id") {
                    return Err(OAuthError("invalid_grant", "OAUTH_REFRESH_TOKEN_INVALID"));
                }
                if refresh.used {
                    self.grants.remove(&family);
                    self.prune();
                    self.persist()?;
                    return Err(OAuthError("invalid_grant", "OAUTH_REFRESH_TOKEN_REPLAY"));
                }
                if fields.get("scope").is_some_and(|s| s != &grant.scope) {
                    return Err(OAuthError("invalid_scope", "OAUTH_SCOPE_INVALID"));
                }
                if self.access.len() >= MAX_CREDENTIALS || self.refresh.len() >= MAX_CREDENTIALS {
                    return Err(OAuthError(
                        "temporarily_unavailable",
                        "OAUTH_CAPACITY_EXCEEDED",
                    ));
                }
                self.refresh.get_mut(&key).unwrap().used = true;
                let previous_expiry = self.grants[&family].expires;
                self.grants.get_mut(&family).unwrap().expires = Instant::now() + GRANT_TTL;
                let result = self.issue(family.clone());
                if result.is_err() {
                    self.grants.get_mut(&family).unwrap().expires = previous_expiry;
                }
                self.persist()?;
                result
            }
            _ => Err(OAuthError("unsupported_grant_type", "OAUTH_GRANT_INVALID")),
        }
    }
    fn issue(&mut self, family: String) -> Result<Value> {
        let access = format!("sd_at_{}", random_id()?);
        let grant = &self.grants[&family];
        // A freshly approved code or an existing refresh token proves consent.
        // Resource scope stays unchanged; offline_access is not an OAuth prerequisite.
        let refresh = if grant.refresh_allowed {
            Some(format!("sd_rt_{}", random_id()?))
        } else {
            None
        };
        let lifetime = ACCESS_TTL.min(grant.expires.saturating_duration_since(Instant::now()));
        let refresh_expiry = grant.expires;
        let mut result = json!({"access_token":access,"token_type":"Bearer","expires_in":lifetime.as_secs(),"scope":grant.scope});
        self.access.insert(
            hash(&access),
            Access {
                family: family.clone(),
                expires: Instant::now() + lifetime,
            },
        );
        if let Some(refresh) = refresh {
            self.refresh.insert(
                hash(&refresh),
                Refresh {
                    family,
                    used: false,
                    expires: refresh_expiry,
                },
            );
            result["refresh_token"] = json!(refresh);
        }
        Ok(result)
    }
    pub fn validate(&mut self, token: &str) -> bool {
        if self.storage_failed {
            return false;
        }
        self.prune();
        if self
            .probe
            .as_ref()
            .is_some_and(|(digest, expires)| *expires > Instant::now() && *digest == hash(token))
        {
            return true;
        }
        self.access
            .get(&hash(token))
            .is_some_and(|a| self.grants.contains_key(&a.family))
    }
    pub fn client_count(&mut self) -> usize {
        self.prune();
        self.grants
            .values()
            .map(|g| &g.client)
            .collect::<HashSet<_>>()
            .len()
    }
    pub(crate) fn probe_credential(&mut self) -> Result<String> {
        self.ensure_storage()?;
        let token = format!("sd_at_probe_{}", random_id()?);
        self.probe = Some((hash(&token), Instant::now() + Duration::from_secs(60)));
        Ok(token)
    }
    pub(crate) fn revoke_probe(&mut self) {
        self.probe = None;
    }
}

#[cfg(test)]
mod tests;
