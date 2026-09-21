use super::{OAuthError, doh, validate_redirect};
use serde::Deserialize;
use std::{
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};
#[cfg(test)]
use std::{
    pin::Pin,
    sync::{Arc, Mutex, OnceLock},
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CLIENT_ID_BYTES: usize = 2048;
const MAX_DOCUMENT_BYTES: usize = 16 * 1024;
const MAX_CONCURRENT_RESOLUTIONS: usize = 4;
const MAX_LOG_VALUE_BYTES: usize = 256;
static RESOLUTION_SLOTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(MAX_CONCURRENT_RESOLUTIONS);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClientMetadata {
    pub(crate) client_id: String,
    pub(crate) client_name: String,
    pub(crate) redirect_uris: Vec<String>,
    pub(crate) refresh_allowed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CimdError(pub(crate) &'static str);

type Result<T> = std::result::Result<T, CimdError>;

#[cfg(test)]
pub(crate) type TestResolver = Arc<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = Option<Result<ClientMetadata>>> + Send>>
        + Send
        + Sync,
>;
#[cfg(test)]
static TEST_RESOLVER: OnceLock<Mutex<Option<TestResolver>>> = OnceLock::new();
#[cfg(test)]
static TEST_RESOLVER_SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
fn test_resolver_slot() -> &'static Mutex<Option<TestResolver>> {
    TEST_RESOLVER.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
pub(crate) struct TestResolverScope {
    previous: Option<TestResolver>,
    _serial: tokio::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for TestResolverScope {
    fn drop(&mut self) {
        *test_resolver_slot().lock().unwrap() = self.previous.take();
    }
}

fn error(code: &'static str) -> CimdError {
    CimdError(code)
}

#[derive(Deserialize)]
struct Document {
    client_id: String,
    client_name: String,
    redirect_uris: Vec<String>,
    #[serde(default)]
    grant_types: Option<Vec<String>>,
    #[serde(default)]
    response_types: Option<Vec<String>>,
    #[serde(default)]
    token_endpoint_auth_method: Option<String>,
}

/// Resolves a CIMD document without changing the current DCR-only OAuth flow.
#[cfg(test)]
pub(crate) async fn resolve_client_metadata(client_id: &str) -> Result<ClientMetadata> {
    resolve_client_metadata_with_logger(client_id, &|_, _| {}).await
}

pub(crate) async fn resolve_client_metadata_with_logger(
    client_id: &str,
    logger: &(dyn Fn(&str, &str) + Sync),
) -> Result<ClientMetadata> {
    // Do not queue unbounded resolver work: a permit covers the complete test or
    // production resolution, including DNS, TLS, HTTP, and the total timeout.
    let _permit = RESOLUTION_SLOTS
        .try_acquire()
        .map_err(|_| error("CIMD_RESOLUTION_THROTTLED"))?;
    #[cfg(test)]
    {
        let resolver = test_resolver_slot().lock().unwrap().clone();
        if let Some(resolver) = resolver
            && let Some(result) = resolver(client_id.to_owned()).await
        {
            return result;
        }
    }
    tokio::time::timeout(
        TOTAL_TIMEOUT,
        resolve_client_metadata_inner(client_id, logger),
    )
    .await
    .map_err(|_| {
        logger(
            "WARN",
            "event=cimd_metadata_failed stage=total_timeout code=CIMD_TOTAL_TIMEOUT",
        );
        error("CIMD_TOTAL_TIMEOUT")
    })?
}

#[cfg(test)]
pub(crate) async fn test_resolver_scope(resolver: Option<TestResolver>) -> TestResolverScope {
    let serial = TEST_RESOLVER_SERIAL.lock().await;
    let previous = std::mem::replace(&mut *test_resolver_slot().lock().unwrap(), resolver);
    TestResolverScope {
        previous,
        _serial: serial,
    }
}

pub(crate) fn is_client_id_metadata_url(client_id: &str) -> bool {
    validate_client_id(client_id).is_ok()
}

pub(crate) fn into_oauth_error(error: CimdError) -> OAuthError {
    match error.0 {
        "CIMD_RESOLUTION_THROTTLED" => OAuthError("temporarily_unavailable", error.0),
        _ => OAuthError("invalid_request", error.0),
    }
}

async fn resolve_client_metadata_inner(
    client_id: &str,
    logger: &(dyn Fn(&str, &str) + Sync),
) -> Result<ClientMetadata> {
    let url = validate_client_id(client_id)?;
    let addrs = resolve_public_addresses(&url).await.inspect_err(|error| {
        log_failure(logger, &url, "dns", error.0);
    })?;
    let bytes = fetch_document(&url, &addrs, logger).await?;
    parse_document(client_id, &bytes)
}

fn validate_client_id(client_id: &str) -> Result<url::Url> {
    if client_id.len() > MAX_CLIENT_ID_BYTES
        || raw_path(client_id).is_none()
        || has_dot_path_segment(client_id)
    {
        return Err(error("CIMD_CLIENT_ID_INVALID"));
    }
    let url = url::Url::parse(client_id).map_err(|_| error("CIMD_CLIENT_ID_INVALID"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(error("CIMD_CLIENT_ID_INVALID"));
    }
    Ok(url)
}

// This operates on the raw input so URL normalization cannot change the identity
// that is checked here before the eventual metadata fetch.
fn raw_path(client_id: &str) -> Option<&str> {
    let (_, after_scheme) = client_id.split_once("://")?;
    let path_start = after_scheme.find(['/', '?', '#'])?;
    if after_scheme.as_bytes()[path_start] != b'/' {
        return None;
    }
    Some(
        after_scheme[path_start..]
            .split(['?', '#'])
            .next()
            .expect("split always yields a first segment"),
    )
}

fn has_dot_path_segment(client_id: &str) -> bool {
    raw_path(client_id).is_some_and(|path| path.split('/').any(is_dot_path_segment))
}

fn is_dot_path_segment(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    let mut index = 0;
    let mut dots = 0;
    while index < bytes.len() {
        if bytes[index] == b'.' {
            index += 1;
        } else if index + 2 < bytes.len()
            && bytes[index] == b'%'
            && bytes[index + 1] == b'2'
            && matches!(bytes[index + 2], b'e' | b'E')
        {
            index += 3;
        } else {
            return false;
        }
        dots += 1;
    }
    matches!(dots, 1 | 2)
}

async fn resolve_public_addresses(url: &url::Url) -> Result<Vec<SocketAddr>> {
    let host = url.host_str().expect("validated client ID host");
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return Err(error("CIMD_ADDRESS_DISALLOWED"));
    }
    let port = url
        .port_or_known_default()
        .expect("HTTPS has a default port");
    if let Ok(ip) = host.parse::<IpAddr>() {
        return if is_disallowed_ip(ip) {
            Err(error("CIMD_ADDRESS_DISALLOWED"))
        } else {
            Ok(vec![SocketAddr::new(ip, 0)])
        };
    }
    let system_addresses: Vec<_> =
        tokio::time::timeout(REQUEST_TIMEOUT, tokio::net::lookup_host((host, port)))
            .await
            .map_err(|_| error("CIMD_DNS_TIMEOUT"))?
            .map_err(|_| error("CIMD_DNS_FAILED"))?
            .map(|addr| addr.ip())
            .collect();
    select_public_addresses_after_system_lookup(host, system_addresses, |host| {
        let host = host.to_owned();
        async move {
            doh::resolve(&host, is_disallowed_ip)
                .await
                .map_err(|_| error("CIMD_DOH_FALLBACK_FAILED"))
        }
    })
    .await
}

async fn select_public_addresses_after_system_lookup<F, Fut>(
    host: &str,
    system_addresses: Vec<IpAddr>,
    fallback: F,
) -> Result<Vec<SocketAddr>>
where
    F: FnOnce(&str) -> Fut,
    Fut: Future<Output = Result<Vec<IpAddr>>>,
{
    if let Ok(ip) = host.parse::<IpAddr>() {
        return if is_disallowed_ip(ip) {
            Err(error("CIMD_ADDRESS_DISALLOWED"))
        } else {
            Ok(vec![SocketAddr::new(ip, 0)])
        };
    }
    if !system_addresses.is_empty()
        && system_addresses
            .iter()
            .all(|address| !is_disallowed_ip(*address))
    {
        return Ok(system_addresses
            .into_iter()
            .map(|address| SocketAddr::new(address, 0))
            .collect());
    }
    let fallback_addresses = fallback(host).await?;
    if fallback_addresses.is_empty()
        || fallback_addresses
            .iter()
            .any(|address| is_disallowed_ip(*address))
    {
        return Err(error("CIMD_DOH_FALLBACK_FAILED"));
    }
    Ok(fallback_addresses
        .into_iter()
        .map(|address| SocketAddr::new(address, 0))
        .collect())
}

pub(super) fn is_disallowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_disallowed_ipv4(ip),
        IpAddr::V6(ip) => is_disallowed_ipv6(ip),
    }
}

fn is_disallowed_ipv6(ip: std::net::Ipv6Addr) -> bool {
    // Public IPv6 destinations are Global Unicast. This rejects the reserved,
    // loopback, link-local, unique-local, multicast, and IPv4-embedded spaces
    // without maintaining a partial deny list. The exceptions below cover the
    // IANA IPv6 Special-Purpose prefixes that fall within Global Unicast.
    !ipv6_in_prefix(ip, 0x2000_0000_0000_0000_0000_0000_0000_0000, 3)
        || [
            (0x2001_0000_0000_0000_0000_0000_0000_0000, 23),
            (0x2001_0db8_0000_0000_0000_0000_0000_0000, 32),
            (0x2002_0000_0000_0000_0000_0000_0000_0000, 16),
            (0x2620_004f_8000_0000_0000_0000_0000_0000, 48),
            (0x3fff_0000_0000_0000_0000_0000_0000_0000, 20),
        ]
        .into_iter()
        .any(|(network, prefix)| ipv6_in_prefix(ip, network, prefix))
}

fn ipv6_in_prefix(ip: std::net::Ipv6Addr, network: u128, prefix: u32) -> bool {
    let mask = u128::MAX << (128 - prefix);
    u128::from_be_bytes(ip.octets()) & mask == network & mask
}

fn is_disallowed_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_broadcast()
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0)
        || (octets[0] == 192 && octets[1] == 31 && octets[2] == 196)
        || (octets[0] == 192 && octets[1] == 52 && octets[2] == 193)
        || (octets[0] == 192 && octets[1] == 88 && octets[2] == 99)
        || (octets[0] == 192 && octets[1] == 175 && octets[2] == 48)
        || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
        || octets[0] >= 240
}

async fn fetch_document(
    url: &url::Url,
    addrs: &[SocketAddr],
    logger: &(dyn Fn(&str, &str) + Sync),
) -> Result<Vec<u8>> {
    // `resolve_to_addrs` overrides the resolver used by the connection itself.
    // Together with `no_proxy`, this binds the request to the addresses checked above.
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(REQUEST_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .resolve_to_addrs(url.host_str().expect("validated client ID host"), addrs)
        .build()
        .map_err(|_| {
            log_failure(logger, url, "client_build", "CIMD_REQUEST_FAILED");
            error("CIMD_REQUEST_FAILED")
        })?;
    fetch_document_with_client(client, url, logger).await
}

async fn fetch_document_with_client(
    client: reqwest::Client,
    url: &url::Url,
    logger: &(dyn Fn(&str, &str) + Sync),
) -> Result<Vec<u8>> {
    log_request(logger, url);
    let response = client.get(url.clone()).send().await.map_err(|_| {
        log_failure(logger, url, "send", "CIMD_REQUEST_FAILED");
        error("CIMD_REQUEST_FAILED")
    })?;
    if response.status() != reqwest::StatusCode::OK {
        log_status(logger, url, &response);
        return Err(error("CIMD_HTTP_STATUS_INVALID"));
    }
    if !response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(is_json_media_type)
    {
        return Err(error("CIMD_CONTENT_TYPE_INVALID"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DOCUMENT_BYTES as u64)
    {
        return Err(error("CIMD_RESPONSE_TOO_LARGE"));
    }
    let mut bytes = Vec::new();
    let mut response = response;
    while let Some(chunk) = response.chunk().await.map_err(|_| {
        log_failure(logger, url, "read", "CIMD_REQUEST_FAILED");
        error("CIMD_REQUEST_FAILED")
    })? {
        if bytes.len() + chunk.len() > MAX_DOCUMENT_BYTES {
            return Err(error("CIMD_RESPONSE_TOO_LARGE"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn log_request(logger: &(dyn Fn(&str, &str) + Sync), url: &url::Url) {
    logger(
        "INFO",
        &format!("event=cimd_metadata_request {}", log_target(url)),
    );
}

fn log_failure(logger: &(dyn Fn(&str, &str) + Sync), url: &url::Url, stage: &str, code: &str) {
    logger(
        "WARN",
        &format!(
            "event=cimd_metadata_failed stage={stage} code={code} {}",
            log_target(url)
        ),
    );
}

fn log_status(logger: &(dyn Fn(&str, &str) + Sync), url: &url::Url, response: &reqwest::Response) {
    let mut message = format!(
        "event=cimd_metadata_response status={} {}",
        response.status().as_u16(),
        log_target(url)
    );
    for (name, field) in [
        (reqwest::header::CONTENT_TYPE, "content_type"),
        (reqwest::header::SERVER, "server"),
        (reqwest::header::HeaderName::from_static("cf-ray"), "cf_ray"),
    ] {
        if let Some(value) = response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
        {
            message.push_str(&format!(" {field}={}", safe_log_value(value)));
        }
    }
    logger("WARN", &message);
}

fn log_target(url: &url::Url) -> String {
    format!(
        "scheme={} host={} path={}",
        safe_log_value(url.scheme()),
        safe_log_value(url.host_str().unwrap_or_default()),
        safe_log_value(url.path())
    )
}

fn safe_log_value(value: &str) -> String {
    let mut output = String::new();
    for character in value.chars() {
        let sanitized = if character.is_ascii_graphic() {
            character
        } else {
            '_'
        };
        if output.len() + sanitized.len_utf8() > MAX_LOG_VALUE_BYTES {
            output.truncate(MAX_LOG_VALUE_BYTES - 3);
            output.push_str("...");
            break;
        }
        output.push(sanitized);
    }
    output
}

fn is_json_media_type(value: &str) -> bool {
    let Some((kind, subtype)) = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .split_once('/')
    else {
        return false;
    };
    kind.eq_ignore_ascii_case("application")
        && is_media_type_token(subtype)
        && (subtype.eq_ignore_ascii_case("json")
            || (subtype.len() > "+json".len() && subtype.to_ascii_lowercase().ends_with("+json")))
}

fn is_media_type_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn parse_document(request_client_id: &str, bytes: &[u8]) -> Result<ClientMetadata> {
    let value = serde_json::from_slice(bytes).map_err(|_| error("CIMD_JSON_INVALID"))?;
    let document: Document =
        serde_json::from_value(value).map_err(|_| error("CIMD_METADATA_INVALID"))?;
    if document.client_id != request_client_id {
        return Err(error("CIMD_CLIENT_ID_MISMATCH"));
    }
    if document.client_name.len() > 200 || document.client_name.chars().any(char::is_control) {
        return Err(error("CIMD_CLIENT_INVALID"));
    }
    if document.redirect_uris.is_empty() || document.redirect_uris.len() > 8 {
        return Err(error("CIMD_REDIRECT_URI_INVALID"));
    }
    for redirect in &document.redirect_uris {
        validate_redirect(redirect).map_err(map_redirect_error)?;
    }
    if document
        .token_endpoint_auth_method
        .as_deref()
        .is_some_and(|method| method != "none")
    {
        return Err(error("CIMD_CLIENT_AUTH_UNSUPPORTED"));
    }
    if document
        .response_types
        .as_ref()
        .is_some_and(|types| !types.iter().any(|response_type| response_type == "code"))
    {
        return Err(error("CIMD_RESPONSE_TYPE_UNSUPPORTED"));
    }
    let refresh_allowed = match document.grant_types {
        None => false,
        Some(grants) => {
            if !grants.iter().any(|grant| grant == "authorization_code") {
                return Err(error("CIMD_GRANT_TYPE_UNSUPPORTED"));
            }
            grants.iter().any(|grant| grant == "refresh_token")
        }
    };
    Ok(ClientMetadata {
        client_id: document.client_id,
        client_name: document.client_name,
        redirect_uris: document.redirect_uris,
        refresh_allowed,
    })
}

fn map_redirect_error(_: OAuthError) -> CimdError {
    error("CIMD_REDIRECT_URI_INVALID")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use tokio_rustls::TlsAcceptor;

    fn valid_document(client_id: &str) -> String {
        format!(
            r#"{{"client_id":"{client_id}","client_name":"Fixture","redirect_uris":["https://client.example/callback"],"grant_types":["authorization_code"],"response_types":["code"],"token_endpoint_auth_method":"none"}}"#
        )
    }

    #[test]
    fn client_id_and_metadata_validation_are_strict() {
        for id in [
            "http://client.example/metadata",
            "https://client.example",
            "https://client.example:443",
            "https://client.example?query=value",
            "https://user@client.example/metadata",
            "https://client.example/metadata#fragment",
            "https://client.example/./metadata",
            "https://client.example/../metadata",
            "https://client.example/%2e/metadata",
            "https://client.example/%2E%2e/metadata",
        ] {
            assert_eq!(validate_client_id(id), Err(error("CIMD_CLIENT_ID_INVALID")));
            assert!(!is_client_id_metadata_url(id));
        }
        let oversized = format!("https://client.example/{}", "a".repeat(MAX_CLIENT_ID_BYTES));
        assert_eq!(
            validate_client_id(&oversized),
            Err(error("CIMD_CLIENT_ID_INVALID"))
        );
        assert!(validate_client_id("https://client.example/").is_ok());
        assert!(is_client_id_metadata_url("https://client.example/"));
        let id = "https://client.example/metadata";
        let metadata = parse_document(id, valid_document(id).as_bytes()).unwrap();
        assert_eq!(metadata.client_name, "Fixture");
        assert!(!metadata.refresh_allowed);
        let omitted_grant_types = format!(
            r#"{{"client_id":"{id}","client_name":"Fixture","redirect_uris":["https://client.example/callback"]}}"#
        );
        assert!(
            !parse_document(id, omitted_grant_types.as_bytes())
                .unwrap()
                .refresh_allowed
        );
        for (body, expected) in [
            ("{}", "CIMD_METADATA_INVALID"),
            (
                r#"{"client_id":"https://other.example/metadata","client_name":"Fixture","redirect_uris":["https://client.example/callback"]}"#,
                "CIMD_CLIENT_ID_MISMATCH",
            ),
            (
                r#"{"client_id":"https://client.example/metadata","client_name":"Fixture","redirect_uris":["http://client.example/callback"]}"#,
                "CIMD_REDIRECT_URI_INVALID",
            ),
            (
                r#"{"client_id":"https://client.example/metadata","client_name":"Fixture","redirect_uris":["https://client.example/callback"],"token_endpoint_auth_method":"client_secret_post"}"#,
                "CIMD_CLIENT_AUTH_UNSUPPORTED",
            ),
            (
                r#"{"client_id":"https://client.example/metadata","client_name":"Fixture","redirect_uris":["https://client.example/callback"],"grant_types":["refresh_token"]}"#,
                "CIMD_GRANT_TYPE_UNSUPPORTED",
            ),
            (
                r#"{"client_id":"https://client.example/metadata","client_name":"Fixture","redirect_uris":["https://client.example/callback"],"response_types":["token"]}"#,
                "CIMD_RESPONSE_TYPE_UNSUPPORTED",
            ),
        ] {
            assert_eq!(parse_document(id, body.as_bytes()), Err(error(expected)));
        }
        let extended = format!(
            r#"{{"client_id":"{id}","client_name":"Fixture","redirect_uris":["https://client.example/callback"],"grant_types":["authorization_code","refresh_token","urn:ietf:params:oauth:grant-type:jwt-bearer"],"response_types":["code","token"],"token_endpoint_auth_method":"none"}}"#
        );
        assert!(
            parse_document(id, extended.as_bytes())
                .unwrap()
                .refresh_allowed
        );
    }

    #[test]
    fn private_and_special_addresses_are_rejected_before_connection() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.1.1",
            "224.0.0.1",
            "0.0.0.0",
            "192.31.196.1",
            "192.52.193.1",
            "192.175.48.1",
            "::1",
            "fe80::1",
            "fc00::1",
            "ff02::1",
            "2001:2::1",
            "2001:db8::1",
            "2002::1",
            "2620:4f:8000::1",
            "3fff::1",
        ] {
            assert!(is_disallowed_ip(address.parse().unwrap()), "{address}");
        }
        assert!(!is_disallowed_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_disallowed_ip("2606:4700:4700::1111".parse().unwrap()));
    }

    #[tokio::test]
    async fn public_system_dns_does_not_invoke_doh_fallback() {
        let calls = Arc::new(AtomicUsize::new(0));
        let fallback_calls = Arc::clone(&calls);
        let addresses = select_public_addresses_after_system_lookup(
            "client.example",
            vec!["93.184.216.34".parse().unwrap()],
            move |_| {
                fallback_calls.fetch_add(1, Ordering::SeqCst);
                async { Ok(vec!["8.8.8.8".parse().unwrap()]) }
            },
        )
        .await
        .unwrap();
        assert_eq!(
            addresses,
            vec![SocketAddr::new("93.184.216.34".parse().unwrap(), 0)]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn synthetic_system_dns_uses_a_public_pinned_fallback() {
        let calls = Arc::new(AtomicUsize::new(0));
        let fallback_calls = Arc::clone(&calls);
        let addresses = select_public_addresses_after_system_lookup(
            "client.example",
            vec!["fc00::1".parse().unwrap()],
            move |_| {
                fallback_calls.fetch_add(1, Ordering::SeqCst);
                async { Ok(vec!["93.184.216.34".parse().unwrap()]) }
            },
        )
        .await
        .unwrap();
        assert_eq!(
            addresses,
            vec![SocketAddr::new("93.184.216.34".parse().unwrap(), 0)]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn disallowed_or_failed_fallbacks_fail_closed() {
        let disallowed = select_public_addresses_after_system_lookup(
            "client.example",
            vec!["198.18.0.1".parse().unwrap()],
            |_| async { Ok(vec!["10.0.0.1".parse().unwrap()]) },
        )
        .await;
        assert_eq!(disallowed, Err(error("CIMD_DOH_FALLBACK_FAILED")));
        let failed =
            select_public_addresses_after_system_lookup("client.example", Vec::new(), |_| async {
                Err(error("CIMD_DOH_FALLBACK_FAILED"))
            })
            .await;
        assert_eq!(failed, Err(error("CIMD_DOH_FALLBACK_FAILED")));
    }

    #[tokio::test]
    async fn disallowed_ip_literals_never_invoke_doh_fallback() {
        let calls = Arc::new(AtomicUsize::new(0));
        let fallback_calls = Arc::clone(&calls);
        let result =
            select_public_addresses_after_system_lookup("10.0.0.1", Vec::new(), move |_| {
                fallback_calls.fetch_add(1, Ordering::SeqCst);
                async { Ok(vec!["93.184.216.34".parse().unwrap()]) }
            })
            .await;
        assert_eq!(result, Err(error("CIMD_ADDRESS_DISALLOWED")));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn localhost_and_private_client_ids_are_rejected_by_the_production_resolver() {
        let _resolver = test_resolver_scope(None).await;
        for client_id in [
            "https://localhost/metadata",
            "https://127.0.0.1/metadata",
            "https://10.0.0.1/metadata",
        ] {
            assert_eq!(
                resolve_client_metadata(client_id).await,
                Err(error("CIMD_ADDRESS_DISALLOWED")),
                "{client_id}"
            );
        }
    }

    #[tokio::test]
    async fn resolution_slots_throttle_without_queueing_and_release() {
        let (entered_tx, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
        let barrier = Arc::new(tokio::sync::Barrier::new(MAX_CONCURRENT_RESOLUTIONS + 1));
        let block = Arc::new(AtomicBool::new(true));
        let resolver_barrier = Arc::clone(&barrier);
        let resolver_block = Arc::clone(&block);
        let _resolver = test_resolver_scope(Some(Arc::new(move |client_id| {
            let entered_tx = entered_tx.clone();
            let barrier = Arc::clone(&resolver_barrier);
            let block = Arc::clone(&resolver_block);
            Box::pin(async move {
                if block.load(Ordering::SeqCst) {
                    entered_tx.send(()).unwrap();
                    barrier.wait().await;
                }
                Some(Ok(ClientMetadata {
                    client_id,
                    client_name: "Fixture".into(),
                    redirect_uris: vec!["https://client.example/callback".into()],
                    refresh_allowed: false,
                }))
            })
        })))
        .await;
        let mut resolves = Vec::new();
        for index in 0..MAX_CONCURRENT_RESOLUTIONS {
            resolves.push(tokio::spawn(async move {
                resolve_client_metadata(&format!("https://client{index}.example/metadata")).await
            }));
        }
        for _ in 0..MAX_CONCURRENT_RESOLUTIONS {
            entered_rx.recv().await.unwrap();
        }
        let throttled = tokio::time::timeout(
            Duration::from_millis(100),
            resolve_client_metadata("https://client5.example/metadata"),
        )
        .await
        .expect("the fifth resolution must not queue")
        .unwrap_err();
        assert_eq!(throttled, error("CIMD_RESOLUTION_THROTTLED"));
        let response = into_oauth_error(throttled).into_response();
        assert_eq!(response.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        block.store(false, Ordering::SeqCst);
        barrier.wait().await;
        for resolve in resolves {
            assert!(resolve.await.unwrap().is_ok());
        }
        assert!(
            resolve_client_metadata("https://client6.example/metadata")
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_resolver_scope_restores_after_panic() {
        let stale = Arc::new(|_| {
            Box::pin(async { Some(Err(error("CIMD_STALE_RESOLVER"))) })
                as Pin<Box<dyn Future<Output = Option<Result<ClientMetadata>>> + Send>>
        });
        let panicking_scope = tokio::spawn(async move {
            let _resolver = test_resolver_scope(Some(stale)).await;
            panic!("resolver scope panic fixture");
        });
        assert!(panicking_scope.await.unwrap_err().is_panic());
        let scope = tokio::time::timeout(Duration::from_secs(1), test_resolver_scope(None))
            .await
            .expect("serialization lock must be released after panic");
        assert!(scope.previous.is_none(), "stale resolver must be restored");
        assert_eq!(
            resolve_client_metadata("https://127.0.0.1/metadata").await,
            Err(error("CIMD_ADDRESS_DISALLOWED"))
        );
    }

    async fn fixture(response: String) -> (url::Url, Vec<SocketAddr>, reqwest::Certificate) {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["foobar.com".into()]).unwrap();
        let certificate = reqwest::Certificate::from_der(cert.der()).unwrap();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                rustls::pki_types::PrivateKeyDer::Pkcs8(signing_key.serialize_der().into()),
            )
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = TlsAcceptor::from(Arc::new(config))
                .accept(stream)
                .await
                .unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            if response.is_empty() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            } else {
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (
            url::Url::parse(&format!("https://foobar.com:{}/metadata", address.port())).unwrap(),
            vec![SocketAddr::new(address.ip(), 0)],
            certificate,
        )
    }

    async fn fetch_fixture(response: String) -> Result<Vec<u8>> {
        let (url, addrs, certificate) = fixture(response).await;
        fetch_document_with_root(&url, &addrs, certificate).await
    }

    async fn fetch_document_with_root(
        url: &url::Url,
        addrs: &[SocketAddr],
        certificate: reqwest::Certificate,
    ) -> Result<Vec<u8>> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(REQUEST_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .add_root_certificate(certificate)
            .resolve_to_addrs(url.host_str().unwrap(), addrs)
            .build()
            .unwrap();
        fetch_document_with_client(client, url, &|_, _| {}).await
    }

    #[tokio::test]
    async fn fixture_rejects_redirect_oversize_and_invalid_json() {
        for (response, expected) in [
            (
                "HTTP/1.1 302 Found\r\nLocation: https://example.com/\r\nContent-Length: 0\r\n\r\n"
                    .to_owned(),
                "CIMD_HTTP_STATUS_INVALID",
            ),
            (
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                    MAX_DOCUMENT_BYTES + 1
                ),
                "CIMD_RESPONSE_TOO_LARGE",
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1\r\n\r\n{"
                    .to_owned(),
                "CIMD_JSON_INVALID",
            ),
        ] {
            let body = fetch_fixture(response).await;
            let result = match body {
                Ok(bytes) => parse_document("https://foobar.com/metadata", &bytes),
                Err(error) => Err(error),
            };
            assert_eq!(result, Err(error(expected)));
        }
    }

    #[tokio::test]
    async fn fixture_requires_200_and_a_json_media_type() {
        for content_type in [
            "Application/JSON; charset=utf-8",
            "application/client-id+json",
        ] {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: 2\r\n\r\n{{}}"
            );
            assert_eq!(fetch_fixture(response).await.unwrap(), b"{}");
        }
        for (response, expected) in [
            (
                "HTTP/1.1 204 No Content\r\nContent-Type: application/json\r\nContent-Length: 0\r\n\r\n"
                    .to_owned(),
                "CIMD_HTTP_STATUS_INVALID",
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Type: text/json\r\nContent-Length: 2\r\n\r\n{}"
                    .to_owned(),
                "CIMD_CONTENT_TYPE_INVALID",
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Type: application/client id+json\r\nContent-Length: 2\r\n\r\n{}"
                    .to_owned(),
                "CIMD_CONTENT_TYPE_INVALID",
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Type: application /json\r\nContent-Length: 2\r\n\r\n{}"
                    .to_owned(),
                "CIMD_CONTENT_TYPE_INVALID",
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Type: application/ json\r\nContent-Length: 2\r\n\r\n{}"
                    .to_owned(),
                "CIMD_CONTENT_TYPE_INVALID",
            ),
        ] {
            assert_eq!(fetch_fixture(response).await, Err(error(expected)));
        }
    }

    #[tokio::test]
    async fn fixture_timeout_is_rejected() {
        let (url, addrs, certificate) = fixture(String::new()).await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_millis(10))
            .timeout(Duration::from_millis(10))
            .add_root_certificate(certificate)
            .resolve_to_addrs(url.host_str().unwrap(), &addrs)
            .build()
            .unwrap();
        assert_eq!(
            fetch_document_with_client(client, &url, &|_, _| {}).await,
            Err(error("CIMD_REQUEST_FAILED"))
        );
    }

    #[tokio::test]
    async fn non_200_diagnostic_uses_the_broker_log_queue_without_sensitive_values() {
        let response = "HTTP/1.1 403 Forbidden\r\nContent-Type: text/html\r\nServer: example-server\r\nCF-RAY: example-ray\r\nSet-Cookie: session=set_cookie_secret\r\nContent-Length: 17\r\n\r\nresponse_body_secret".to_owned();
        let (mut url, addrs, certificate) = fixture(response).await;
        url.set_query(Some("token=query_secret"));
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(REQUEST_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .add_root_certificate(certificate)
            .resolve_to_addrs(url.host_str().unwrap(), &addrs)
            .build()
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let paths = crate::config::AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file: directory.path().join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs/app.log"),
            serena_log: directory.path().join("logs/serena.log"),
        };
        crate::config::save(&paths.config_file, &crate::config::ManagerConfig::default()).unwrap();
        let broker = Arc::new(crate::mcp::Broker::new(Arc::new(
            crate::serena::SupervisorState::new(paths).unwrap(),
        )));
        broker.remote.attach_broker_for_test(&broker);
        let logger = |level: &str, message: &str| broker.remote.log_mcp(level, message);

        assert_eq!(
            fetch_document_with_client(client, &url, &logger).await,
            Err(error("CIMD_HTTP_STATUS_INVALID"))
        );
        let logs = broker.log_snapshot();
        let diagnostic = logs
            .iter()
            .find(|line| line.contains("event=cimd_metadata_response"))
            .unwrap();
        assert!(diagnostic.contains("status=403"));
        assert!(diagnostic.contains("scheme=https host=foobar.com path=/metadata"));
        assert!(diagnostic.contains("content_type=text/html"));
        assert!(diagnostic.contains("server=example-server"));
        assert!(diagnostic.contains("cf_ray=example-ray"));
        for secret in [
            "query_secret",
            "response_body_secret",
            "set_cookie_secret",
            "Set-Cookie",
        ] {
            assert!(
                !diagnostic.contains(secret),
                "sensitive value leaked: {secret}"
            );
        }
    }

    #[test]
    fn log_values_are_bounded_and_cannot_inject_lines() {
        let value = safe_log_value(&format!("ok\r\nWARN injected {}", "x".repeat(512)));
        assert!(!value.contains(['\r', '\n']));
        assert!(value.ends_with("..."));
        let message = format!("event=cimd_metadata_response server={value}");
        assert_eq!(message.lines().count(), 1);
    }
}
