use ngrok::{
    Session,
    config::Binding,
    forwarder::Forwarder,
    prelude::{EndpointInfo, ForwarderBuilder, TunnelCloser},
    session::ConnectError,
    tunnel::HttpTunnel,
};
use std::{fmt, future::Future, pin::Pin, time::Duration};
use url::Url;

use super::ngrok_store::NgrokAuthToken;

const CONNECT_FAILED: &str = "NGROK_CONNECT_FAILED";
const CONNECT_TIMEOUT: &str = "NGROK_CONNECT_TIMEOUT";
const AUTH_FAILED: &str = "NGROK_AUTH_FAILED";
const NETWORK_FAILED: &str = "NGROK_NETWORK_FAILED";
const TUNNEL_START_FAILED: &str = "NGROK_TUNNEL_START_FAILED";
const PUBLIC_ORIGIN_INVALID: &str = "NGROK_PUBLIC_ORIGIN_INVALID";
const TUNNEL_DISCONNECTED: &str = "NGROK_TUNNEL_DISCONNECTED";
const TUNNEL_STOP_FAILED: &str = "NGROK_TUNNEL_STOP_FAILED";

const NGROK_CONNECT_DEADLINE: Duration = Duration::from_secs(15);
const NGROK_LISTEN_DEADLINE: Duration = Duration::from_secs(15);
const NGROK_RESOURCE_CLOSE_DEADLINE: Duration = Duration::from_secs(5);
pub(crate) const NGROK_TUNNEL_CLOSE_DEADLINE: Duration = Duration::from_secs(12);

pub(crate) trait NgrokTunnelHandle: Send {
    fn origin(&self) -> &str;

    fn wait(&mut self) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;

    fn close(&mut self) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;
}

pub(crate) struct NgrokConnectorFailure {
    code: String,
    retained_tunnel: Option<Box<dyn NgrokTunnelHandle>>,
}

impl NgrokConnectorFailure {
    pub(crate) fn new(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            retained_tunnel: None,
        }
    }

    fn with_retained(code: &'static str, retained_tunnel: Box<dyn NgrokTunnelHandle>) -> Self {
        Self {
            code: code.to_string(),
            retained_tunnel: Some(retained_tunnel),
        }
    }

    pub(crate) fn into_parts(self) -> (String, Option<Box<dyn NgrokTunnelHandle>>) {
        (self.code, self.retained_tunnel)
    }
}

impl fmt::Debug for NgrokConnectorFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NgrokConnectorFailure")
            .field("code", &self.code)
            .field("retained_tunnel", &self.retained_tunnel.is_some())
            .finish()
    }
}

pub(crate) type NgrokConnectFuture<'a> = Pin<
    Box<
        dyn Future<Output = Result<Box<dyn NgrokTunnelHandle>, NgrokConnectorFailure>>
            + Send
            + 'a,
    >,
>;

pub(crate) trait NgrokConnector: Send + Sync {
    fn connect<'a>(
        &'a self,
        auth_token: &'a NgrokAuthToken,
        broker_port: u16,
    ) -> NgrokConnectFuture<'a>;
}

pub(crate) struct SdkNgrokConnector;

pub(crate) struct NgrokTunnel {
    session: Session,
    forwarder: Forwarder<HttpTunnel>,
    origin: String,
}

impl NgrokTunnel {
    pub(crate) async fn start(
        auth_token: &NgrokAuthToken,
        broker_port: u16,
    ) -> Result<Self, NgrokConnectorFailure> {
        let target = loopback_target_url(broker_port).map_err(NgrokConnectorFailure::new)?;
        let mut builder = Session::builder();
        builder.authtoken(auth_token.expose());
        let mut session = run_sdk_connect(NGROK_CONNECT_DEADLINE, builder.connect())
            .await
            .map_err(NgrokConnectorFailure::new)?;
        let mut forwarder = match run_sdk_operation(
            NGROK_LISTEN_DEADLINE,
            session
                .http_endpoint()
                .binding(Binding::Public)
                .listen_and_forward(target),
            TUNNEL_START_FAILED,
        )
        .await
        {
            Ok(forwarder) => forwarder,
            Err(error) => {
                return match close_session(&mut session).await {
                    Ok(()) => Err(NgrokConnectorFailure::new(error)),
                    Err(()) => Err(NgrokConnectorFailure::with_retained(
                        TUNNEL_STOP_FAILED,
                        Box::new(NgrokSessionHandle { session }),
                    )),
                };
            }
        };
        let origin = match normalize_public_origin(forwarder.url()) {
            Ok(origin) => origin,
            Err(error) => {
                return match close_tunnel_resources(&mut forwarder, &mut session).await {
                    Ok(()) => Err(NgrokConnectorFailure::new(error)),
                    Err(()) => Err(NgrokConnectorFailure::with_retained(
                        TUNNEL_STOP_FAILED,
                        Box::new(Self {
                            session,
                            forwarder,
                            origin: String::new(),
                        }),
                    )),
                };
            }
        };

        Ok(Self {
            session,
            forwarder,
            origin,
        })
    }

    pub(crate) fn origin(&self) -> &str {
        &self.origin
    }

    pub(crate) async fn wait(&mut self) -> Result<(), String> {
        match self.forwarder.join().await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) | Err(_) => Err(TUNNEL_DISCONNECTED.to_string()),
        }
    }

    pub(crate) async fn close(&mut self) -> Result<(), String> {
        if close_tunnel_resources(&mut self.forwarder, &mut self.session)
            .await
            .is_err()
        {
            return Err(TUNNEL_STOP_FAILED.to_string());
        }
        Ok(())
    }
}

impl NgrokTunnelHandle for NgrokTunnel {
    fn origin(&self) -> &str {
        NgrokTunnel::origin(self)
    }

    fn wait(&mut self) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        Box::pin(NgrokTunnel::wait(self))
    }

    fn close(&mut self) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        Box::pin(NgrokTunnel::close(self))
    }
}

struct NgrokSessionHandle {
    session: Session,
}

impl NgrokTunnelHandle for NgrokSessionHandle {
    fn origin(&self) -> &str {
        ""
    }

    fn wait(&mut self) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        Box::pin(async { Err(TUNNEL_DISCONNECTED.to_string()) })
    }

    fn close(&mut self) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        Box::pin(async move {
            close_session(&mut self.session)
                .await
                .map_err(|()| TUNNEL_STOP_FAILED.to_string())
        })
    }
}

impl NgrokConnector for SdkNgrokConnector {
    fn connect<'a>(
        &'a self,
        auth_token: &'a NgrokAuthToken,
        broker_port: u16,
    ) -> NgrokConnectFuture<'a> {
        Box::pin(async move {
            NgrokTunnel::start(auth_token, broker_port)
                .await
                .map(|tunnel| Box::new(tunnel) as Box<dyn NgrokTunnelHandle>)
        })
    }
}

async fn close_tunnel_resources(
    forwarder: &mut Forwarder<HttpTunnel>,
    session: &mut Session,
) -> Result<(), ()> {
    let forwarder_result =
        tokio::time::timeout(NGROK_RESOURCE_CLOSE_DEADLINE, forwarder.close()).await;
    let session_result = tokio::time::timeout(NGROK_RESOURCE_CLOSE_DEADLINE, session.close()).await;
    if matches!(forwarder_result, Ok(Ok(()))) && matches!(session_result, Ok(Ok(()))) {
        Ok(())
    } else {
        Err(())
    }
}

async fn close_session(session: &mut Session) -> Result<(), ()> {
    match tokio::time::timeout(NGROK_RESOURCE_CLOSE_DEADLINE, session.close()).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) | Err(_) => Err(()),
    }
}

async fn run_sdk_operation<T, E, F>(
    deadline: Duration,
    operation: F,
    failure_code: &'static str,
) -> Result<T, String>
where
    F: Future<Output = Result<T, E>>,
{
    match tokio::time::timeout(deadline, operation).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) | Err(_) => Err(failure_code.to_string()),
    }
}

async fn run_sdk_connect<T, F>(deadline: Duration, operation: F) -> Result<T, String>
where
    F: Future<Output = Result<T, ConnectError>>,
{
    match tokio::time::timeout(deadline, operation).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(connect_failure_code(&error).to_string()),
        Err(_) => Err(CONNECT_TIMEOUT.to_string()),
    }
}

fn connect_failure_code(error: &ConnectError) -> &'static str {
    match error {
        ConnectError::Auth(_) => AUTH_FAILED,
        ConnectError::Tcp(_) | ConnectError::Tls(_) | ConnectError::ProxyConnect(_) => {
            NETWORK_FAILED
        }
        ConnectError::Start(_) | ConnectError::Rebind(_) | ConnectError::Canceled | _ => {
            CONNECT_FAILED
        }
    }
}

fn loopback_target_url(broker_port: u16) -> Result<Url, String> {
    Url::parse(&format!("http://127.0.0.1:{broker_port}"))
        .map_err(|_| TUNNEL_START_FAILED.to_string())
}

fn normalize_public_origin(value: &str) -> Result<String, String> {
    super::validate_https_origin(value).map_err(|_| PUBLIC_ORIGIN_INVALID.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    struct FakeTunnel {
        origin: String,
        waited: Arc<AtomicBool>,
        closed: Arc<AtomicBool>,
    }

    impl NgrokTunnelHandle for FakeTunnel {
        fn origin(&self) -> &str {
            &self.origin
        }

        fn wait(&mut self) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
            self.waited.store(true, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }

        fn close(&mut self) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
            self.closed.store(true, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    struct FakeConnector {
        waited: Arc<AtomicBool>,
        closed: Arc<AtomicBool>,
    }

    impl NgrokConnector for FakeConnector {
        fn connect<'a>(
            &'a self,
            _auth_token: &'a NgrokAuthToken,
            broker_port: u16,
        ) -> NgrokConnectFuture<'a> {
            let tunnel = FakeTunnel {
                origin: format!("https://fake-{broker_port}.example.com"),
                waited: Arc::clone(&self.waited),
                closed: Arc::clone(&self.closed),
            };
            Box::pin(async move { Ok(Box::new(tunnel) as Box<dyn NgrokTunnelHandle>) })
        }
    }

    #[test]
    fn sdk_connector_is_usable_as_trait_object() {
        let connector: Arc<dyn NgrokConnector> = Arc::new(SdkNgrokConnector);

        assert_eq!(Arc::strong_count(&connector), 1);
    }

    #[tokio::test]
    async fn fake_connector_and_tunnel_work_as_trait_objects() {
        let waited = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        let connector: Arc<dyn NgrokConnector> = Arc::new(FakeConnector {
            waited: Arc::clone(&waited),
            closed: Arc::clone(&closed),
        });
        let auth_token = NgrokAuthToken::new("fake-test-token".into());

        let mut tunnel = connector.connect(&auth_token, 9120).await.unwrap();

        assert_eq!(tunnel.origin(), "https://fake-9120.example.com");
        tunnel.wait().await.unwrap();
        tunnel.close().await.unwrap();
        assert!(waited.load(Ordering::SeqCst));
        assert!(closed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn connector_failure_preserves_a_retryable_tunnel_handle() {
        let waited = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        let failure = NgrokConnectorFailure::with_retained(
            TUNNEL_STOP_FAILED,
            Box::new(FakeTunnel {
                origin: "https://retained.example".to_string(),
                waited,
                closed: Arc::clone(&closed),
            }),
        );

        let (code, retained) = failure.into_parts();
        assert_eq!(code, TUNNEL_STOP_FAILED);
        let mut retained = retained.expect("cleanup failure must retain its handle");
        retained.close().await.unwrap();
        assert!(closed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn sdk_connect_timeout_uses_stable_error() {
        let error = run_sdk_connect(
            Duration::ZERO,
            std::future::pending::<Result<(), ConnectError>>(),
        )
        .await
        .unwrap_err();

        assert_eq!(error, CONNECT_TIMEOUT);
    }

    #[tokio::test]
    async fn sdk_connect_tcp_failure_uses_network_error() {
        let error = run_sdk_connect(
            Duration::ZERO,
            async {
                Err::<(), _>(ConnectError::Tcp(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    "test tcp failure",
                )))
            },
        )
        .await
        .unwrap_err();

        assert_eq!(error, NETWORK_FAILED);
    }

    #[tokio::test]
    async fn sdk_connect_tls_failure_uses_network_error() {
        let error = run_sdk_connect(
            Duration::ZERO,
            async {
                Err::<(), _>(ConnectError::Tls(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "test tls failure",
                )))
            },
        )
        .await
        .unwrap_err();

        assert_eq!(error, NETWORK_FAILED);
    }

    #[tokio::test]
    async fn sdk_connect_cancelled_failure_uses_generic_error() {
        let error = run_sdk_connect(Duration::ZERO, async {
            Err::<(), _>(ConnectError::Canceled)
        })
        .await
        .unwrap_err();

        assert_eq!(error, CONNECT_FAILED);
    }

    #[tokio::test]
    async fn sdk_listen_timeout_uses_stable_error() {
        let error = run_sdk_operation(
            Duration::ZERO,
            std::future::pending::<Result<(), ()>>(),
            TUNNEL_START_FAILED,
        )
        .await
        .unwrap_err();

        assert_eq!(error, TUNNEL_START_FAILED);
    }

    #[test]
    fn loopback_target_uses_exact_broker_endpoint() {
        let target = loopback_target_url(9120).unwrap();

        assert_eq!(target.scheme(), "http");
        assert_eq!(target.host_str(), Some("127.0.0.1"));
        assert_eq!(target.port(), Some(9120));
        assert_eq!(target.path(), "/");
        assert!(target.query().is_none());
        assert!(target.fragment().is_none());
        assert!(target.username().is_empty());
        assert!(target.password().is_none());
    }

    #[test]
    fn https_ngrok_origin_is_normalized() {
        assert_eq!(
            normalize_public_origin("https://Example.NGROK.app:443/").unwrap(),
            "https://example.ngrok.app"
        );
    }

    #[test]
    fn invalid_public_origins_use_stable_error() {
        for value in [
            "http://example.ngrok.app",
            "https://example.ngrok.app/path",
            "https://example.ngrok.app?query=value",
            "https://example.ngrok.app#fragment",
            "https://user@example.ngrok.app",
        ] {
            assert_eq!(
                normalize_public_origin(value).unwrap_err(),
                PUBLIC_ORIGIN_INVALID
            );
        }
    }

    #[test]
    fn token_debug_and_helper_errors_do_not_expose_token() {
        let token = NgrokAuthToken::new("test-secret-ngrok-token".into());

        let debug = format!("{token:?}");
        let error = normalize_public_origin(token.expose()).unwrap_err();

        assert!(!debug.contains(token.expose()));
        assert!(!error.contains(token.expose()));
        assert_eq!(error, PUBLIC_ORIGIN_INVALID);
    }
}
