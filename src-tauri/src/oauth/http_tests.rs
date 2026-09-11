use super::*;
use crate::{
    config::{AppPaths, ManagerConfig},
    mcp::Broker,
    remote::{McpAuthPolicy, RemotePublicContext, Status},
    serena::SupervisorState,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

#[tokio::test]
async fn broker_oauth_http_pkce_native_approval_and_json_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let paths = AppPaths {
        runtime_directory: root.join("runtime"),
        config_file: root.join("config.json"),
        log_directory: root.join("logs"),
        app_log: root.join("logs/app.log"),
        serena_log: root.join("logs/serena.log"),
    };
    let mut config = ManagerConfig::default();
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    config.broker.port = reserved.local_addr().unwrap().port();
    drop(reserved);
    crate::config::save(&paths.config_file, &config).unwrap();
    let broker = Arc::new(Broker::new(Arc::new(SupervisorState::new(paths).unwrap())));
    broker.start().await.unwrap();
    let context = RemotePublicContext::new("https://test.trycloudflare.com").unwrap();
    {
        let mut inner = broker.remote.inner.lock().unwrap();
        inner.oauth = Some(Runtime::open(context.clone(), root.join("oauth.json")).unwrap());
        inner.policy = McpAuthPolicy::EmbeddedOAuth;
        inner.status = Status::Ready;
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let base = format!("http://127.0.0.1:{}", config.broker.port);
    let mcp = format!("{base}/mcp");
    let response = client.post(&mcp).send().await.unwrap();
    assert_eq!(response.status(), 401);
    assert!(
        response.headers()["www-authenticate"]
            .to_str()
            .unwrap()
            .contains(&context.public_origin)
    );
    let response = client
        .get(format!("{base}/.well-known/oauth-authorization-server"))
        .header("host", "attacker.example")
        .header("x-forwarded-host", "attacker.example")
        .header("forwarded", "host=attacker.example;proto=http")
        .header("origin", "https://attacker.example")
        .header("referer", "https://attacker.example/private")
        .send()
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        response.json::<Value>().await.unwrap()["issuer"],
        context.public_origin
    );
    let registration = client.post(format!("{base}/oauth/register")).json(&json!({"client_name":"<script>untrusted</script>","redirect_uris":["https://client.example/cb"]})).send().await.unwrap();
    assert_eq!(registration.status(), 201);
    let registration: Value = registration.json().await.unwrap();
    let verifier = "a".repeat(43);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut authorize = url::Url::parse(&format!("{base}/oauth/authorize")).unwrap();
    authorize.query_pairs_mut().extend_pairs([
        ("client_id", registration["client_id"].as_str().unwrap()),
        ("redirect_uri", "https://client.example/cb"),
        ("response_type", "code"),
        ("scope", "serena:mcp"),
        ("resource", &context.mcp_resource),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("state", "client-state"),
    ]);
    for (field, value, expected_error) in [
        ("scope", "unknown", Some("invalid_scope")),
        ("response_type", "token", Some("unsupported_response_type")),
        ("client_id", "unknown-client", None),
        ("redirect_uri", "https://attacker.example/cb", None),
    ] {
        let mut invalid = authorize.clone();
        let pairs: Vec<(String, String)> = authorize
            .query_pairs()
            .map(|(key, original)| {
                let value = if key == field {
                    value.to_owned()
                } else if key == "state" {
                    "state & 中文=1".into()
                } else {
                    original.into_owned()
                };
                (key.into_owned(), value)
            })
            .collect();
        invalid.query_pairs_mut().clear().extend_pairs(pairs);
        let response = client.get(invalid).send().await.unwrap();
        if let Some(error) = expected_error {
            assert_eq!(response.status(), 302);
            let redirect =
                url::Url::parse(response.headers()["location"].to_str().unwrap()).unwrap();
            assert_eq!(
                redirect.origin().ascii_serialization(),
                "https://client.example"
            );
            let query: HashMap<_, _> = redirect.query_pairs().into_owned().collect();
            assert_eq!(query["error"], error);
            assert_eq!(query["state"], "state & 中文=1");
        } else {
            assert_eq!(response.status(), 400);
            assert!(response.headers().get("location").is_none());
        }
    }
    let page = client.get(authorize).send().await.unwrap();
    assert_eq!(page.status(), 200);
    assert_eq!(page.headers()["x-frame-options"], "DENY");
    assert!(
        page.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("script-src 'nonce-")
    );
    let html = page.text().await.unwrap();
    assert!(!html.contains("<script>untrusted"));
    let pending = broker.remote.snapshot().pending.remove(0);
    assert!(pending.refresh_allowed);
    assert_eq!(pending.scope, "serena:mcp");
    assert!(html.contains(&pending.confirmation_code));
    let poll = format!("{base}/oauth/approval/{}", pending.id);
    assert_eq!(
        client
            .get(&poll)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap()["status"],
        "pending"
    );
    assert_eq!(
        client
            .post(&poll)
            .json(&json!({"allow":true}))
            .send()
            .await
            .unwrap()
            .status(),
        405
    );
    broker
        .remote
        .inner
        .lock()
        .unwrap()
        .oauth
        .as_mut()
        .unwrap()
        .decide(&pending.id, true)
        .unwrap();
    let result: Value = client
        .get(&poll)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let redirect = url::Url::parse(result["redirect"].as_str().unwrap()).unwrap();
    let code = redirect
        .query_pairs()
        .find(|(k, _)| k == "code")
        .unwrap()
        .1
        .into_owned();
    let fields = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("client_id", registration["client_id"].as_str().unwrap()),
            ("redirect_uri", "https://client.example/cb"),
            ("resource", &context.mcp_resource),
            ("code_verifier", &verifier),
        ])
        .finish();
    let token = client
        .post(format!("{base}/oauth/token"))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(fields)
        .send()
        .await
        .unwrap();
    assert_eq!(token.status(), 200);
    assert_eq!(token.headers()["cache-control"], "no-store");
    let token: Value = token.json().await.unwrap();
    assert_eq!(token["scope"], "serena:mcp");
    let refresh = token["refresh_token"].as_str().unwrap();
    let access = token["access_token"].as_str().unwrap();
    let response = client
        .post(format!("{mcp}?access_token={access}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    for body in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"http-contract","version":"1"}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"workspace_list","arguments":{}}}),
    ] {
        let response = client
            .post(&mcp)
            .bearer_auth(access)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-11-25")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "application/json");
        let value: Value = response.json().await.unwrap();
        assert!(value.get("error").is_none(), "{value}");
        assert_ne!(value["result"]["isError"], true);
    }
    assert_eq!(
        client
            .get(&mcp)
            .bearer_auth(access)
            .header("accept", "text/event-stream")
            .send()
            .await
            .unwrap()
            .status(),
        405
    );
    // Simulate an expired access token and process restart using isolated test storage.
    {
        let mut inner = broker.remote.inner.lock().unwrap();
        inner.oauth.take();
        let path = root.join("oauth.json");
        let mut saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for record in saved["access"].as_object_mut().unwrap().values_mut() {
            record["expires_at_ms"] = json!(0);
        }
        std::fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        inner.oauth = Some(Runtime::open(context.clone(), path).unwrap());
    }
    assert_eq!(
        client
            .post(&mcp)
            .bearer_auth(access)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let refreshed = client
        .post(format!("{base}/oauth/token"))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh),
                    ("client_id", registration["client_id"].as_str().unwrap()),
                    ("resource", &context.mcp_resource),
                ])
                .finish(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(refreshed.status(), 200);
    assert_eq!(refreshed.headers()["cache-control"], "no-store");
    let refreshed: Value = refreshed.json().await.unwrap();
    assert_eq!(refreshed["scope"], "serena:mcp");
    assert_ne!(refreshed["refresh_token"], refresh);
    assert!(broker.remote.snapshot().pending.is_empty());
    let response = client.post(&mcp)
        .bearer_auth(refreshed["access_token"].as_str().unwrap())
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"workspace_list","arguments":{}}}))
        .send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_ne!(
        response.json::<Value>().await.unwrap()["result"]["isError"],
        true
    );
    let log = broker.log_snapshot().join("\n");
    assert!(!log.contains(refresh));
    assert!(!log.contains(refreshed["access_token"].as_str().unwrap()));
    assert!(!log.contains(access));
    assert!(!log.contains(&code));
    assert!(!log.contains(&verifier));
    {
        let mut inner = broker.remote.inner.lock().unwrap();
        inner.oauth = Some(Runtime::new(
            RemotePublicContext::new("https://new.trycloudflare.com").unwrap(),
        ));
    }
    assert_eq!(
        client
            .post(&mcp)
            .bearer_auth(access)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    {
        let mut inner = broker.remote.inner.lock().unwrap();
        inner.oauth = None;
        inner.policy = McpAuthPolicy::Passthrough;
    }
    assert_eq!(
        client
            .get(format!("{base}/.well-known/oauth-authorization-server"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        client
            .post(&mcp)
            .bearer_auth(access)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    broker.stop().await.unwrap();
}
#[tokio::test]
async fn oversized_state_is_not_reflected_for_unsupported_response_type() {
    let remote = Arc::new(Remote::default());
    let context = RemotePublicContext::new("https://fixture.example").unwrap();
    let mut oauth = Runtime::new(context.clone());
    let client = oauth
        .register(json!({"redirect_uris":["https://client.example/cb"]}))
        .unwrap();
    {
        let mut inner = remote.inner.lock().unwrap();
        inner.status = Status::Ready;
        inner.oauth = Some(oauth);
    }
    let response = authorize(
        State(remote),
        Query(Authorization {
            client_id: client["client_id"].as_str().unwrap().into(),
            redirect_uri: "https://client.example/cb".into(),
            resource: context.mcp_resource,
            response_type: "token".into(),
            code_challenge: URL_SAFE_NO_PAD.encode(Sha256::digest("a".repeat(43))),
            code_challenge_method: "S256".into(),
            state: "x".repeat(2049),
            scope: "serena:mcp".into(),
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(response.headers().get("location").is_none());
    let body = axum::body::to_bytes(response.into_body(), 512)
        .await
        .unwrap();
    assert!(body.len() < 256);
    assert!(
        std::str::from_utf8(&body)
            .unwrap()
            .contains("OAUTH_STATE_INVALID")
    );
}
