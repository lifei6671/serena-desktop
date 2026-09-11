use super::*;
use axum::{body::Body, http::StatusCode, response::IntoResponse};
use serde_json::json;

#[tokio::test]
async fn probe_failures_identify_stage_without_exposing_body_or_credential() {
    for (failed_stage, category, status) in [
        ("oauth_metadata", "invalid_json", 200),
        ("oauth_metadata", "metadata_mismatch", 200),
        ("resource_metadata", "metadata_mismatch", 200),
        ("unauthorized_mcp", "oauth_challenge_mismatch", 401),
        ("initialize", "host_reject", 403),
        ("initialize", "origin_reject", 403),
        ("initialize", "oauth_reject", 401),
        ("initialize", "mcp_result", 200),
        ("tools_list", "mcp_result", 200),
        ("tools_list", "http_status", 502),
        ("oauth_metadata", "proxy", 407),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let public_origin = origin.clone();
        let app = axum::Router::new().fallback(move |request: axum::extract::Request| {
            let origin = public_origin.clone();
            async move {
                let authenticated = request.headers().contains_key("authorization");
                let path = request.uri().path().to_owned();
                let body = axum::body::to_bytes(request.into_body(), 4096).await.unwrap();
                let body: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
                let stage = if path.ends_with("oauth-authorization-server") {
                    "oauth_metadata"
                } else if path.ends_with("oauth-protected-resource/mcp") {
                    "resource_metadata"
                } else if !authenticated {
                    "unauthorized_mcp"
                } else if body["method"] == "initialize" {
                    "initialize"
                } else {
                    "tools_list"
                };
                if stage == failed_stage {
                    let body = match category {
                        "host_reject" => "Forbidden: Host header is not allowed",
                        "origin_reject" => "Origin is not allowed",
                        "metadata_mismatch" => "{\"issuer\":\"PRIVATE_UPSTREAM_BODY\"}",
                        "mcp_result" => "{\"id\":1,\"error\":{\"message\":\"PRIVATE_UPSTREAM_BODY\"}}",
                        _ => "PRIVATE_UPSTREAM_BODY",
                    };
                    return axum::http::Response::builder().status(status)
                        .header("content-type", "application/json").body(Body::from(body)).unwrap();
                }
                match stage {
                    "oauth_metadata" => axum::Json(json!({"issuer":origin,"token_endpoint":format!("{origin}/oauth/token")})).into_response(),
                    "resource_metadata" => axum::Json(json!({"resource":format!("{origin}/mcp"),"authorization_servers":[origin]})).into_response(),
                    "unauthorized_mcp" => (StatusCode::UNAUTHORIZED, [("www-authenticate",format!("Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\", scope=\"serena:mcp\""))]).into_response(),
                    "initialize" => axum::Json(json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25"}})).into_response(),
                    _ => axum::Json(json!({"jsonrpc":"2.0","id":2,"result":{"tools":[]}})).into_response(),
                }
            }
        });
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let context = RemotePublicContext {
            public_origin: origin.clone(),
            mcp_resource: format!("{origin}/mcp"),
            instance_id: "fixture".into(),
        };
        let error = probe(&context, "PRIVATE_PROBE_CREDENTIAL")
            .await
            .unwrap_err();
        server.abort();
        assert!(
            error.contains(&format!("stage={failed_stage} category={category}")),
            "{error}"
        );
        assert!(error.contains(&format!("status={status}")), "{error}");
        assert!(error.contains("host=127.0.0.1 elapsed_ms="));
        assert!(!error.contains("PRIVATE_"));
    }
}

#[tokio::test]
async fn probe_connection_and_timeout_errors_are_sanitized() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let url = format!("http://{address}/?proxy=PRIVATE_CODE");
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(50))
        .build()
        .unwrap();
    let error = client.get(&url).send().await.unwrap_err();
    let stage = ProbeStage::new("oauth_metadata", "127.0.0.1");
    let evidence = stage.network(&error);
    assert!(evidence.contains("category=timeout"), "{evidence}");
    assert!(!evidence.contains("PRIVATE_CODE"));
    drop(listener);
    let error = client
        .get(url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .unwrap_err();
    let evidence = stage.network(&error);
    assert!(evidence.contains("category=connect"), "{evidence}");
    assert!(!evidence.contains("PRIVATE_CODE"));
}

#[test]
fn wrapped_network_causes_only_yield_fixed_categories() {
    for (message, category) in [
        ("dns error: failed to lookup PRIVATE_HOST", "dns"),
        ("invalid peer certificate: PRIVATE_DETAIL", "tls"),
        ("proxy authentication failed PRIVATE_PASSWORD", "proxy"),
    ] {
        let error = std::io::Error::other(message);
        assert_eq!(network_category(&error), Some(category));
    }
    assert_eq!(
        network_category(&std::io::Error::other("unknown private detail")),
        None
    );
}

#[tokio::test]
#[ignore = "read-only public TLS/proxy diagnosis; requires REMOTE_NETWORK_DIAGNOSTIC_ORIGIN"]
async fn public_network_proxy_and_tls_diagnostic() {
    let origin =
        std::env::var("REMOTE_NETWORK_DIAGNOSTIC_ORIGIN").expect("diagnostic Origin required");
    let origin = validate_https_origin(&origin).unwrap();
    let url = url::Url::parse(&origin).unwrap();
    let host = url.host_str().unwrap();
    for direct in [false, true] {
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10));
        if direct {
            builder = builder.no_proxy();
        }
        let client = builder.build().unwrap();
        let stage = ProbeStage::new("oauth_metadata", host);
        match client
            .get(format!("{origin}/.well-known/oauth-authorization-server"))
            .send()
            .await
        {
            Ok(response) => println!(
                "network diagnostic proxy={} TLS/HTTP reached status={}",
                if direct {
                    "disabled_for_test"
                } else {
                    "system_default"
                },
                response.status().as_u16()
            ),
            Err(error) => {
                let mut source = std::error::Error::source(&error);
                let mut cause = "unclassified";
                let mut os_error = None;
                while let Some(error) = source {
                    let text = error.to_string().to_ascii_lowercase();
                    if let Some((_, tail)) = text.split_once("(os error ") {
                        os_error = tail
                            .split(')')
                            .next()
                            .and_then(|code| code.parse::<i32>().ok());
                    }
                    for (pattern, label) in [
                        ("eof", "eof"),
                        ("unexpected eof", "unexpected_eof"),
                        ("close_notify", "missing_close_notify"),
                        ("unknownissuer", "unknown_issuer"),
                        ("revocation", "revocation"),
                        ("expired", "expired"),
                        ("notvalidforname", "hostname_mismatch"),
                    ] {
                        if text.contains(pattern) {
                            cause = label;
                        }
                    }
                    source = error.source();
                }
                println!(
                    "{} proxy={} cause={cause} os_error={os_error:?}",
                    stage.network(&error),
                    if direct {
                        "disabled_for_test"
                    } else {
                        "system_default"
                    }
                );
            }
        }
    }
}
