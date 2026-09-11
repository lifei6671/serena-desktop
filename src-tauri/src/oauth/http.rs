use super::*;
use crate::remote::{McpAuthPolicy, Remote, Status};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Form, Path, Query, Request, State},
    http::{HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};
use tauri::Emitter;

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;

impl IntoResponse for OAuthError {
    fn into_response(self) -> Response {
        let status = match self.0 {
            "server_error" => StatusCode::INTERNAL_SERVER_ERROR,
            "temporarily_unavailable" => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::BAD_REQUEST,
        };
        (
            status,
            Json(json!({"error":self.0,"error_description":self.1})),
        )
            .into_response()
    }
}
fn unavailable() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error":"OAUTH_NOT_RUNNING"})),
    )
        .into_response()
}
pub fn router(remote: Arc<Remote>) -> Router {
    Router::new()
        .route("/.well-known/oauth-protected-resource", get(resource))
        .route("/.well-known/oauth-protected-resource/mcp", get(resource))
        .route("/.well-known/oauth-authorization-server", get(metadata))
        .route("/oauth/register", post(register))
        .route("/oauth/authorize", get(authorize))
        .route("/oauth/token", post(token))
        .route("/oauth/approval/{id}", get(poll))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(axum::middleware::from_fn(
            |request: Request, next: axum::middleware::Next| async move {
                secure(next.run(request).await)
            },
        ))
        .with_state(remote)
}
fn secure(mut response: Response) -> Response {
    let headers = response.headers_mut();
    for (key, value) in [
        ("cache-control", "no-store"),
        ("pragma", "no-cache"),
        ("referrer-policy", "no-referrer"),
        ("x-frame-options", "DENY"),
        ("x-content-type-options", "nosniff"),
    ] {
        headers.insert(key, HeaderValue::from_static(value));
    }
    if !headers.contains_key("content-security-policy") {
        headers.insert(
            "content-security-policy",
            HeaderValue::from_static("default-src 'self'; frame-ancestors 'none'; base-uri 'none'"),
        );
    }
    response
}
async fn resource(State(remote): State<Arc<Remote>>) -> Response {
    let inner = remote.inner.lock().unwrap();
    let Some(oauth) = &inner.oauth else {
        return unavailable();
    };
    Json(json!({"resource":oauth.context.mcp_resource,"authorization_servers":[oauth.context.public_origin],"scopes_supported":["serena:mcp"],"bearer_methods_supported":["header"]})).into_response()
}
async fn metadata(State(remote): State<Arc<Remote>>) -> Response {
    let inner = remote.inner.lock().unwrap();
    let Some(oauth) = &inner.oauth else {
        return unavailable();
    };
    let origin = &oauth.context.public_origin;
    Json(json!({"issuer":origin,"authorization_endpoint":format!("{origin}/oauth/authorize"),"token_endpoint":format!("{origin}/oauth/token"),"registration_endpoint":format!("{origin}/oauth/register"),"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"token_endpoint_auth_methods_supported":["none"],"code_challenge_methods_supported":["S256"],"scopes_supported":["serena:mcp","offline_access"]})).into_response()
}
async fn register(State(remote): State<Arc<Remote>>, Json(value): Json<Value>) -> Response {
    let mut inner = remote.inner.lock().unwrap();
    if inner.status != Status::Ready {
        return unavailable();
    }
    let Some(oauth) = &mut inner.oauth else {
        return unavailable();
    };
    match oauth.register(value) {
        Ok(value) => (StatusCode::CREATED, Json(value)).into_response(),
        Err(e) => e.into_response(),
    }
}
async fn authorize(
    State(remote): State<Arc<Remote>>,
    Query(request): Query<Authorization>,
) -> Response {
    let view = {
        let mut inner = remote.inner.lock().unwrap();
        if inner.status != Status::Ready {
            return unavailable();
        }
        let Some(oauth) = &mut inner.oauth else {
            return unavailable();
        };
        let redirect_uri = request.redirect_uri.clone();
        let state = request.state.clone();
        match oauth.authorize(request) {
            Ok(view) => view,
            // Runtime emits these only after validating the registered client and redirect.
            Err(e) if matches!(e.0, "invalid_scope" | "unsupported_response_type") => {
                let mut redirect = url::Url::parse(&redirect_uri).expect("validated redirect URI");
                redirect
                    .query_pairs_mut()
                    .append_pair("error", e.0)
                    .append_pair("state", &state);
                return (StatusCode::FOUND, [("location", redirect.as_str())]).into_response();
            }
            Err(e) => return e.into_response(),
        }
    };
    if let Some(app) = remote.app.get()
        && view.created
    {
        let _ = app.emit("remote-authorization", &view);
        if view.wake_window {
            crate::tray::show_main_window(app);
        }
    }
    let nonce = match random_id() {
        Ok(n) => n,
        Err(e) => return e.into_response(),
    };
    // Only our random id and numeric confirmation are interpolated, never client input.
    let page = format!(
        r#"<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>连接 SerenaDesktop</title><body><main><h1>请在本机 SerenaDesktop 中确认</h1><p>核对两处确认码一致后，在桌面应用中允许连接。</p><h2>{}</h2><p id="status" role="status">等待本机授权，120 秒后过期。</p></main><script nonce="{nonce}">
const status = document.getElementById('status');
const deadline = Date.now() + 120000;
async function poll() {{
  if (Date.now() > deadline) {{ status.textContent = '授权已过期，请返回客户端重新连接。'; return; }}
  try {{
    const response = await fetch('/oauth/approval/{}', {{cache:'no-store', credentials:'omit', headers:{{'Accept':'application/json', 'ngrok-skip-browser-warning':'1'}}}});
    if (!response.ok) {{ status.textContent = '远程访问已停止，请返回客户端重试。'; return; }}
    if (response.headers.get('content-type')?.split(';')[0].trim().toLowerCase() !== 'application/json') {{ status.textContent = '网关返回了非 JSON 页面，无法读取授权结果。请检查网关配置后重新连接。'; return; }}
    const result = await response.json();
    if ((result.status === 'approved' || result.status === 'denied') && result.redirect) {{ location.replace(result.redirect); return; }}
    if (result.status === 'expired') {{ status.textContent = '授权已过期，请返回客户端重新连接。'; return; }}
  }} catch {{ status.textContent = '连接暂时中断，正在等待本机响应…'; }}
  setTimeout(poll, 1000);
}}
setTimeout(poll, 1000);
</script></body></html>"#,
        view.confirmation_code, view.id
    );
    let mut response = Html(page).into_response();
    response.headers_mut().insert("content-security-policy", HeaderValue::from_str(&format!("default-src 'self'; script-src 'nonce-{nonce}'; frame-ancestors 'none'; base-uri 'none'")).unwrap());
    response
}
async fn poll(State(remote): State<Arc<Remote>>, Path(id): Path<String>) -> Response {
    let mut inner = remote.inner.lock().unwrap();
    let Some(oauth) = &mut inner.oauth else {
        return unavailable();
    };
    Json(oauth.poll(&id)).into_response()
}
async fn token(
    State(remote): State<Arc<Remote>>,
    Form(fields): Form<HashMap<String, String>>,
) -> Response {
    let mut inner = remote.inner.lock().unwrap();
    let Some(oauth) = &mut inner.oauth else {
        return unavailable();
    };
    match oauth.token(&fields) {
        Ok(value) => Json(value).into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn protect(
    State(remote): State<Arc<Remote>>,
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    {
        let mut inner = remote.inner.lock().unwrap();
        if inner.policy == McpAuthPolicy::Passthrough {
            // A revoked Desktop credential must never become an anonymous request.
            if request
                .headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.contains("sd_at_"))
            {
                return secure(
                    (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({"error":"OAUTH_ACCESS_TOKEN_INVALID"})),
                    )
                        .into_response(),
                );
            }
        } else {
            let token = request
                .headers()
                .get("authorization")
                .and_then(|h| h.to_str().ok())
                .and_then(|h| h.split_once(' '))
                .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("Bearer"))
                .map(|(_, token)| token);
            let valid = request.headers().get_all("authorization").iter().count() == 1
                && token.is_some_and(|t| inner.oauth.as_mut().is_some_and(|o| o.validate(t)));
            if !valid {
                let mut response = (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error":"OAUTH_ACCESS_TOKEN_INVALID"})),
                )
                    .into_response();
                if let Some(oauth) = &inner.oauth {
                    let challenge = format!(
                        "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp\", scope=\"serena:mcp\"",
                        oauth.context.public_origin
                    );
                    response.headers_mut().insert(
                        "www-authenticate",
                        HeaderValue::from_str(&challenge).unwrap(),
                    );
                }
                return secure(response);
            }
        }
    };
    next.run(request).await
}
