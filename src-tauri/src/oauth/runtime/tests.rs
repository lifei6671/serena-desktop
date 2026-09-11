use super::*;

fn runtime() -> Runtime {
    Runtime::new(RemotePublicContext::new("https://test.trycloudflare.com").unwrap())
}
fn request(oauth: &mut Runtime) -> Authorization {
    let client = oauth.register(json!({"client_name":"Test","redirect_uris":["https://client.example/callback"],"token_endpoint_auth_method":"none"})).unwrap();
    Authorization {
        client_id: client["client_id"].as_str().unwrap().into(),
        redirect_uri: "https://client.example/callback".into(),
        resource: oauth.context.mcp_resource.clone(),
        response_type: "code".into(),
        code_challenge: hash(&"a".repeat(43)),
        code_challenge_method: "S256".into(),
        state: "bound state".into(),
        scope: "serena:mcp offline_access".into(),
    }
}
fn approved(oauth: &mut Runtime, request: Authorization) -> HashMap<String, String> {
    let pending = oauth.authorize(request.clone()).unwrap();
    assert_eq!(oauth.poll(&pending.id)["status"], "pending");
    oauth.decide(&pending.id, true).unwrap();
    let poll = oauth.poll(&pending.id);
    let url = url::Url::parse(poll["redirect"].as_str().unwrap()).unwrap();
    let values: HashMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(values["state"], request.state);
    HashMap::from([
        ("grant_type".into(), "authorization_code".into()),
        ("client_id".into(), request.client_id),
        ("redirect_uri".into(), request.redirect_uri),
        ("resource".into(), request.resource),
        ("code_verifier".into(), "a".repeat(43)),
        ("code".into(), values["code"].clone()),
    ])
}
#[test]
fn pkce_exchange_rotation_replay_revokes_family() {
    let mut oauth = runtime();
    let request = request(&mut oauth);
    let fields = approved(&mut oauth, request);
    let tokens = oauth.token(&fields).unwrap();
    assert!(oauth.validate(tokens["access_token"].as_str().unwrap()));
    assert!(oauth.token(&fields).is_err());
    let refresh = HashMap::from([
        ("grant_type".into(), "refresh_token".into()),
        ("client_id".into(), fields["client_id"].clone()),
        ("resource".into(), fields["resource"].clone()),
        (
            "refresh_token".into(),
            tokens["refresh_token"].as_str().unwrap().into(),
        ),
    ]);
    let second = oauth.token(&refresh).unwrap();
    assert_ne!(tokens["refresh_token"], second["refresh_token"]);
    assert_eq!(
        oauth.token(&refresh).unwrap_err().1,
        "OAUTH_REFRESH_TOKEN_REPLAY"
    );
    assert!(!oauth.validate(tokens["access_token"].as_str().unwrap()));
    assert!(!oauth.validate(second["access_token"].as_str().unwrap()));
    assert_eq!(oauth.client_count(), 0);
}
#[test]
fn request_validation_and_local_approval_are_required() {
    let mut oauth = runtime();
    let request = request(&mut oauth);
    for field in ["resource", "redirect", "pkce", "scope"] {
        let mut bad = request.clone();
        match field {
            "resource" => bad.resource.push_str("/other"),
            "redirect" => bad.redirect_uri.push_str("/other"),
            "pkce" => bad.code_challenge_method = "plain".into(),
            _ => bad.scope = "admin".into(),
        }
        assert!(oauth.authorize(bad).is_err());
    }
    let view = oauth.authorize(request.clone()).unwrap();
    assert_eq!(oauth.pending().len(), 1);
    assert!(oauth.codes.is_empty());
    oauth.decide(&view.id, false).unwrap();
    assert_eq!(oauth.poll(&view.id)["status"], "denied");
    assert!(oauth.codes.is_empty());
    assert!(oauth.decide(&view.id, true).is_err());
    let mut request = request;
    request.state.push_str(" new");
    let view = oauth.authorize(request).unwrap();
    oauth.pending.get_mut(&view.id).unwrap().expires = Instant::now() - Duration::from_secs(1);
    assert_eq!(
        oauth.decide(&view.id, true).unwrap_err().1,
        "OAUTH_AUTHORIZATION_EXPIRED"
    );
    assert_eq!(oauth.poll(&view.id)["status"], "expired");
}
#[test]
fn wrong_token_bindings_and_wrong_verifier_fail() {
    for field in ["resource", "client_id", "redirect_uri", "code_verifier"] {
        let mut oauth = runtime();
        let request = request(&mut oauth);
        let mut fields = approved(&mut oauth, request);
        let original = fields.clone();
        fields.insert(field.into(), "wrong".into());
        assert!(oauth.token(&fields).is_err(), "{field}");
        assert!(oauth.access.is_empty());
        if field == "code_verifier" {
            assert_eq!(oauth.token(&original).unwrap_err().1, "OAUTH_CODE_REPLAY");
        }
    }
}
#[test]
fn expired_credentials_and_new_runtime_reject_old_tokens() {
    let mut oauth = runtime();
    let request = request(&mut oauth);
    let fields = approved(&mut oauth, request);
    let tokens = oauth.token(&fields).unwrap();
    let access = tokens["access_token"].as_str().unwrap();
    assert!(!runtime().validate(access));
    assert!(
        !Runtime::new(RemotePublicContext::new("https://new.trycloudflare.com").unwrap())
            .validate(access)
    );
    oauth.access.get_mut(&hash(access)).unwrap().expires = Instant::now() - Duration::from_secs(1);
    assert!(!oauth.validate(access));
    assert!(!oauth.validate("unknown"));
}
#[test]
fn registration_rejects_unsafe_redirects_and_caps_public_state() {
    let mut oauth = runtime();
    for redirect in [
        "javascript:alert(1)",
        "http://public.example/cb",
        "https://example/cb#fragment",
        "https://user:pass@example/cb",
    ] {
        assert!(oauth.register(json!({"redirect_uris":[redirect]})).is_err());
    }
    assert!(
        oauth
            .register(json!({"redirect_uris":["http://127.0.0.1:8989/cb"]}))
            .is_ok()
    );
    let request = request(&mut oauth);
    for n in 0..MAX_PENDING {
        oauth.pending_created.clear();
        let mut unique = request.clone();
        unique.state = n.to_string();
        oauth.authorize(unique).unwrap();
    }
    oauth.pending_created.clear();
    assert_eq!(
        oauth.authorize(request).unwrap_err().1,
        "OAUTH_CAPACITY_EXCEEDED"
    );
}

#[test]
fn refresh_replay_revokes_even_when_credential_capacity_is_full() {
    let mut oauth = runtime();
    let request = request(&mut oauth);
    let fields = approved(&mut oauth, request);
    let tokens = oauth.token(&fields).unwrap();
    let refresh_key = hash(tokens["refresh_token"].as_str().unwrap());
    let family = oauth.refresh[&refresh_key].family.clone();
    oauth.refresh.get_mut(&refresh_key).unwrap().used = true;
    for n in 1..MAX_CREDENTIALS {
        oauth.refresh.insert(
            format!("filled-{n}"),
            Refresh {
                family: family.clone(),
                used: true,
                expires: Instant::now() + GRANT_TTL,
            },
        );
    }
    let refresh = HashMap::from([
        ("grant_type".into(), "refresh_token".into()),
        ("client_id".into(), fields["client_id"].clone()),
        ("resource".into(), fields["resource"].clone()),
        (
            "refresh_token".into(),
            tokens["refresh_token"].as_str().unwrap().into(),
        ),
    ]);
    assert_eq!(oauth.refresh.len(), MAX_CREDENTIALS);
    assert_eq!(
        oauth.token(&refresh).unwrap_err().1,
        "OAUTH_REFRESH_TOKEN_REPLAY"
    );
    assert!(!oauth.validate(tokens["access_token"].as_str().unwrap()));
    assert!(oauth.grants.is_empty());
}

impl Runtime {
    pub(crate) fn test_access_token(&mut self) -> String {
        let request = request(self);
        let fields = approved(self, request);
        self.token(&fields).unwrap()["access_token"]
            .as_str()
            .unwrap()
            .into()
    }
}

#[test]
fn internal_probe_credential_is_ephemeral_and_not_a_user_grant() {
    let mut oauth = runtime();
    let credential = oauth.probe_credential().unwrap();
    assert!(oauth.validate(&credential));
    assert_eq!(oauth.client_count(), 0);
    assert!(oauth.pending().is_empty());
    assert!(oauth.clients.is_empty());
    assert!(oauth.grants.is_empty());
    oauth.revoke_probe();
    assert!(!oauth.validate(&credential));
    let credential = oauth.probe_credential().unwrap();
    oauth.probe.as_mut().unwrap().1 = Instant::now() - Duration::from_secs(1);
    assert!(!oauth.validate(&credential));
}

#[test]
fn expired_unused_registrations_release_client_capacity() {
    let mut oauth = runtime();
    let registration = json!({"redirect_uris":["https://client.example/callback"]});
    for _ in 0..MAX_CLIENTS {
        oauth.register(registration.clone()).unwrap();
    }
    assert_eq!(
        oauth.register(registration.clone()).unwrap_err().1,
        "OAUTH_CAPACITY_EXCEEDED"
    );
    for client in oauth.clients.values_mut() {
        client.expires = Instant::now() - Duration::from_secs(1);
    }
    let new_client = oauth.register(registration).unwrap();
    assert_eq!(oauth.clients.len(), 1);
    assert!(
        oauth
            .clients
            .contains_key(new_client["client_id"].as_str().unwrap())
    );
}

#[test]
fn expired_registration_is_retained_for_pending_authorization() {
    let mut oauth = runtime();
    let request = request(&mut oauth);
    let id = request.client_id.clone();
    let pending = oauth.authorize(request).unwrap();
    oauth.clients.get_mut(&id).unwrap().expires = Instant::now() - Duration::from_secs(1);
    assert_eq!(oauth.pending()[0].id, pending.id);
    assert!(oauth.clients.contains_key(&id));
    assert!(oauth.codes.is_empty());
    assert!(oauth.grants.is_empty());
    oauth.pending.get_mut(&pending.id).unwrap().expires = Instant::now() - Duration::from_secs(1);
    assert!(oauth.pending().is_empty());
    assert!(!oauth.clients.contains_key(&id));
}

#[test]
fn expired_registration_is_retained_for_unredeemed_code() {
    let mut oauth = runtime();
    let request = request(&mut oauth);
    let id = request.client_id.clone();
    let fields = approved(&mut oauth, request);
    oauth
        .pending
        .values_mut()
        .for_each(|pending| pending.expires = Instant::now() - Duration::from_secs(1));
    oauth.clients.get_mut(&id).unwrap().expires = Instant::now() - Duration::from_secs(1);
    oauth.prune();
    assert!(oauth.pending.is_empty());
    assert!(oauth.grants.is_empty());
    assert!(oauth.clients.contains_key(&id));
    let token = oauth.token(&fields).unwrap();
    assert!(oauth.validate(token["access_token"].as_str().unwrap()));
}

#[test]
fn expired_registration_is_retained_until_its_grant_expires() {
    let mut oauth = runtime();
    let request = request(&mut oauth);
    let id = request.client_id.clone();
    let fields = approved(&mut oauth, request);
    let token = oauth.token(&fields).unwrap();
    oauth
        .pending
        .values_mut()
        .for_each(|pending| pending.expires = Instant::now() - Duration::from_secs(1));
    oauth.clients.get_mut(&id).unwrap().expires = Instant::now() - Duration::from_secs(1);
    oauth.prune();
    assert!(oauth.pending.is_empty());
    assert!(oauth.codes.is_empty());
    let refresh = HashMap::from([
        ("grant_type".into(), "refresh_token".into()),
        ("client_id".into(), id.clone()),
        ("resource".into(), fields["resource"].clone()),
        (
            "refresh_token".into(),
            token["refresh_token"].as_str().unwrap().into(),
        ),
    ]);
    let rotated = oauth.token(&refresh).unwrap();
    assert!(oauth.validate(rotated["access_token"].as_str().unwrap()));
    for grant in oauth.grants.values_mut() {
        grant.expires = Instant::now() - Duration::from_secs(1);
    }
    assert!(!oauth.validate(rotated["access_token"].as_str().unwrap()));
    assert!(!oauth.clients.contains_key(&id));
    assert!(oauth.refresh.is_empty());
}

#[test]
fn approved_refresh_clients_receive_tokens_without_offline_access_scope() {
    for scope in ["serena:mcp", "serena:mcp offline_access"] {
        let mut oauth = runtime();
        let mut request = request(&mut oauth);
        request.scope = scope.into();
        let fields = approved(&mut oauth, request);
        let token = oauth.token(&fields).unwrap();
        assert_eq!(token["scope"], scope);
        assert!(oauth.validate(token["access_token"].as_str().unwrap()));
        let access = token["access_token"].as_str().unwrap();
        assert!(oauth.access.contains_key(&hash(access)));
        assert!(!oauth.access.contains_key(access));
        assert!(
            token["refresh_token"]
                .as_str()
                .unwrap()
                .starts_with("sd_rt_")
        );
        assert_eq!(oauth.refresh.len(), 1);
        assert!(token["expires_in"].as_u64().unwrap() <= 3600);
        let rotated = oauth.token(&refresh_request(&fields, &token)).unwrap();
        assert_eq!(
            rotated["scope"], scope,
            "refresh must not expand resource scope"
        );
    }
}

#[test]
fn loopback_redirects_use_typed_url_hosts() {
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let redirect = format!(
        "http://[::1]:{}/callback",
        reserved.local_addr().unwrap().port()
    );
    assert!(
        runtime()
            .register(json!({"redirect_uris":[redirect]}))
            .is_ok()
    );
    let ipv6 = url::Url::parse("http://[::1]:8989/callback").unwrap();
    // url 2.x currently serializes host_str with brackets; Host avoids depending
    // on that textual representation when identifying the loopback address.
    assert_eq!(ipv6.host_str(), Some("[::1]"));
    assert_eq!(
        ipv6.host(),
        Some(url::Host::Ipv6(std::net::Ipv6Addr::LOCALHOST))
    );
    for redirect in [
        "http://localhost:8989/callback",
        "http://127.0.0.1:8989/callback",
        "http://[::1]:8989/callback",
        "http://[0:0:0:0:0:0:0:1]:8989/callback",
    ] {
        assert!(
            runtime()
                .register(json!({"redirect_uris":[redirect]}))
                .is_ok(),
            "{redirect}"
        );
    }
    for redirect in [
        "http://[::2]:8989/callback",
        "http://[2001:db8::1]/callback",
        "http://192.168.1.1/callback",
        "http://example.com/callback",
    ] {
        assert!(
            runtime()
                .register(json!({"redirect_uris":[redirect]}))
                .is_err(),
            "{redirect}"
        );
    }
}

#[test]
fn registration_controls_refresh_consent_and_survives_restart() {
    for refresh_allowed in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oauth.json");
        let mut oauth = persisted_runtime(&path);
        let grants = if refresh_allowed {
            json!(["authorization_code", "refresh_token"])
        } else {
            json!(["authorization_code"])
        };
        let registration = oauth
            .register(
                json!({"redirect_uris":["https://client.example/callback"],"grant_types":grants}),
            )
            .unwrap();
        assert_eq!(registration["grant_types"], grants);
        let mut req = request(&mut oauth);
        req.client_id = registration["client_id"].as_str().unwrap().into();
        req.scope = "serena:mcp".into();
        let view = oauth.authorize(req.clone()).unwrap();
        assert_eq!(view.refresh_allowed, refresh_allowed);
        assert_eq!(
            oauth.authorize(req.clone()).unwrap().refresh_allowed,
            refresh_allowed
        );
        assert_eq!(oauth.pending()[0].refresh_allowed, refresh_allowed);
        assert!(oauth.access.is_empty());
        assert!(oauth.refresh.is_empty(), "browser request is not approval");
        let fields = approved(&mut oauth, req.clone());
        let tokens = oauth.token(&fields).unwrap();
        assert_eq!(tokens.get("refresh_token").is_some(), refresh_allowed);
        drop(oauth);
        let mut oauth = persisted_runtime(&path);
        assert!(oauth.validate(tokens["access_token"].as_str().unwrap()));
        assert_eq!(
            oauth.authorize(req.clone()).unwrap().refresh_allowed,
            refresh_allowed
        );
        if refresh_allowed {
            assert!(oauth.token(&refresh_request(&fields, &tokens)).is_ok());
        } else {
            req.scope = "serena:mcp offline_access".into();
            assert_eq!(oauth.authorize(req).unwrap_err().0, "invalid_scope");
            assert!(oauth.refresh.is_empty());
        }
    }
}

#[test]
fn basic_scope_refresh_after_restart_and_access_expiry_needs_no_new_approval() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oauth.json");
    let mut oauth = persisted_runtime(&path);
    let mut req = request(&mut oauth);
    req.scope = "serena:mcp".into();
    let fields = approved(&mut oauth, req);
    let tokens = oauth.token(&fields).unwrap();
    let access = tokens["access_token"].as_str().unwrap();
    oauth.access.get_mut(&hash(access)).unwrap().expires = Instant::now() - Duration::from_secs(1);
    oauth.grants.values_mut().next().unwrap().expires = Instant::now() + Duration::from_secs(60);
    oauth.persist().unwrap();
    drop(oauth);
    let mut oauth = persisted_runtime(&path);
    assert!(!oauth.validate(access));
    let request = refresh_request(&fields, &tokens);
    for field in ["client_id", "resource", "scope"] {
        let mut bad = request.clone();
        bad.insert(field.into(), "wrong".into());
        assert!(oauth.token(&bad).is_err());
    }
    let renewed = oauth.token(&request).unwrap();
    assert_eq!(renewed["scope"], "serena:mcp");
    assert!(oauth.validate(renewed["access_token"].as_str().unwrap()));
    assert!(oauth.pending().is_empty());
    assert!(oauth.codes.is_empty());
    assert!(
        oauth
            .grants
            .values()
            .next()
            .unwrap()
            .expires
            .duration_since(Instant::now())
            > Duration::from_secs(72 * 3600 - 2)
    );
    drop(oauth);
    let mut oauth = persisted_runtime(&path);
    assert_eq!(
        oauth.token(&request).unwrap_err().1,
        "OAUTH_REFRESH_TOKEN_REPLAY"
    );
    drop(oauth);
    let mut oauth = persisted_runtime(&path);
    assert!(!oauth.validate(renewed["access_token"].as_str().unwrap()));
    assert!(oauth.token(&refresh_request(&fields, &renewed)).is_err());
}

#[test]
fn legacy_basic_scope_grants_are_not_silently_upgraded_on_load() {
    for version in [1, 2] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oauth.json");
        let mut oauth = persisted_runtime(&path);
        let mut req = request(&mut oauth);
        req.scope = "serena:mcp".into();
        let fields = approved(&mut oauth, req.clone());
        let tokens = oauth.token(&fields).unwrap();
        drop(oauth);
        let mut saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        saved["version"] = json!(version);
        saved["refresh"] = json!({}); // Old basic-scope grants never issued refresh tokens.
        for client in saved["clients"].as_object_mut().unwrap().values_mut() {
            client.as_object_mut().unwrap().remove("refresh_allowed");
        }
        std::fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        let mut oauth = persisted_runtime(&path);
        assert!(oauth.validate(tokens["access_token"].as_str().unwrap()));
        assert!(oauth.refresh.is_empty());
        assert!(oauth.token(&refresh_request(&fields, &tokens)).is_err());
        oauth.persist().unwrap();
        let migrated: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(migrated["grants"], saved["grants"]);
        assert_eq!(migrated["access"], saved["access"]);
        assert_eq!(migrated["refresh"], json!({}));
        // Existing registrations remain usable, but a NEW native approval is required.
        let fields = approved(&mut oauth, req);
        assert!(oauth.token(&fields).unwrap().get("refresh_token").is_some());
    }
}

#[test]
fn current_storage_requires_refresh_policy_and_rejects_unregistered_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oauth.json");
    let mut oauth = persisted_runtime(&path);
    let mut req = request(&mut oauth);
    req.scope = "serena:mcp".into();
    let fields = approved(&mut oauth, req);
    oauth.token(&fields).unwrap();
    drop(oauth);
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for policy in [Value::Null, json!(false), json!("true")] {
        let mut bad = saved.clone();
        bad["clients"][&fields["client_id"]]["refresh_allowed"] = policy;
        std::fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(
            Runtime::open(
                RemotePublicContext::new("https://test.trycloudflare.com").unwrap(),
                path.clone()
            )
            .is_err()
        );
    }
}

fn persisted_runtime(path: &std::path::Path) -> Runtime {
    Runtime::open(
        RemotePublicContext::new("https://test.trycloudflare.com").unwrap(),
        path.to_owned(),
    )
    .unwrap()
}

fn refresh_request(fields: &HashMap<String, String>, token: &Value) -> HashMap<String, String> {
    HashMap::from([
        ("grant_type".into(), "refresh_token".into()),
        ("client_id".into(), fields["client_id"].clone()),
        ("resource".into(), fields["resource"].clone()),
        (
            "refresh_token".into(),
            token["refresh_token"].as_str().unwrap().into(),
        ),
    ])
}

#[test]
fn durable_tokens_survive_restart_and_rotation_replay_stays_revoked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oauth.json");
    let mut oauth = persisted_runtime(&path);
    let request = request(&mut oauth);
    let fields = approved(&mut oauth, request);
    let tokens = oauth.token(&fields).unwrap();
    let access = tokens["access_token"].as_str().unwrap();
    let original_expiry: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let original_expiry = original_expiry["access"][hash(access)]["expires_at_ms"]
        .as_u64()
        .unwrap();
    let instance_id = oauth.context.instance_id.clone();
    drop(oauth);

    let mut oauth = persisted_runtime(&path);
    assert_ne!(instance_id, oauth.context.instance_id);
    assert!(oauth.validate(access));
    assert!(oauth.pending().is_empty());
    assert!(oauth.codes.is_empty());
    let refresh = refresh_request(&fields, &tokens);
    let rotated = oauth.token(&refresh).unwrap();
    assert_ne!(tokens["refresh_token"], rotated["refresh_token"]);
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let re_saved_expiry = saved["access"][hash(access)]["expires_at_ms"]
        .as_u64()
        .unwrap();
    assert_eq!(
        re_saved_expiry, original_expiry,
        "reload must not renew access TTL"
    );
    drop(oauth);

    let mut oauth = persisted_runtime(&path);
    assert!(oauth.validate(rotated["access_token"].as_str().unwrap()));
    assert_eq!(
        oauth.token(&refresh).unwrap_err().1,
        "OAUTH_REFRESH_TOKEN_REPLAY"
    );
    drop(oauth);
    let mut oauth = persisted_runtime(&path);
    assert!(!oauth.validate(access));
    assert!(!oauth.validate(rotated["access_token"].as_str().unwrap()));
    assert!(oauth.token(&refresh_request(&fields, &rotated)).is_err());
}

#[test]
fn durable_snapshot_excludes_plain_credentials_and_transient_flows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oauth.json");
    let mut oauth = persisted_runtime(&path);
    let request = request(&mut oauth);
    let fields = approved(&mut oauth, request);
    let probe = oauth.probe_credential().unwrap();
    let tokens = oauth.token(&fields).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    for secret in [
        tokens["access_token"].as_str().unwrap(),
        tokens["refresh_token"].as_str().unwrap(),
        &fields["code"],
        &fields["code_verifier"],
        &probe,
        &oauth.context.instance_id,
    ] {
        assert!(!text.contains(secret));
    }
    let saved: Value = serde_json::from_str(&text).unwrap();
    for field in ["pending", "codes", "used_codes", "probe", "instanceId"] {
        assert!(saved.get(field).is_none());
    }
    drop(oauth);
    let mut oauth = persisted_runtime(&path);
    assert!(!oauth.validate(&probe));
    assert!(
        oauth.token(&fields).is_err(),
        "codes must not survive restart"
    );
}

#[test]
fn persisted_absolute_expiry_is_pruned_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oauth.json");
    let mut oauth = persisted_runtime(&path);
    let request = request(&mut oauth);
    let fields = approved(&mut oauth, request);
    let token = oauth.token(&fields).unwrap();
    drop(oauth);
    let mut saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for records in ["clients", "grants", "access"] {
        for record in saved[records].as_object_mut().unwrap().values_mut() {
            record["expires_at_ms"] = json!(0);
        }
    }
    std::fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut oauth = persisted_runtime(&path);
    assert!(!oauth.validate(token["access_token"].as_str().unwrap()));
    assert!(oauth.token(&refresh_request(&fields, &token)).is_err());
    assert!(oauth.clients.is_empty());
    assert!(oauth.grants.is_empty());
    assert!(oauth.refresh.is_empty());
}

#[test]
fn corrupt_incompatible_or_different_origin_store_is_never_reset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oauth.json");
    let mut oauth = persisted_runtime(&path);
    request(&mut oauth);
    drop(oauth);
    let bytes = std::fs::read(&path).unwrap();
    let different_origin = RemotePublicContext::new("https://different.example").unwrap();
    assert!(matches!(
        Runtime::open(different_origin, path.clone()),
        Err(OAuthError("server_error", "OAUTH_STORAGE_ORIGIN_MISMATCH"))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let mut version: Value = serde_json::from_slice(&bytes).unwrap();
    version["version"] = json!(999);
    for contents in [
        b"broken json".to_vec(),
        serde_json::to_vec(&version).unwrap(),
    ] {
        std::fs::write(&path, &contents).unwrap();
        assert!(
            Runtime::open(
                RemotePublicContext::new("https://test.trycloudflare.com").unwrap(),
                path.clone()
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), contents);
    }
}

#[test]
fn stored_client_metadata_must_pass_registration_validation_without_rewriting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oauth.json");
    let mut oauth = persisted_runtime(&path);
    request(&mut oauth);
    drop(oauth);
    let baseline: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for (field, invalid) in [
        ("redirects", json!(["not a url"])),
        ("redirects", json!(["http://public.example/callback"])),
        (
            "redirects",
            json!(["https://user:password@example.com/callback"]),
        ),
        (
            "redirects",
            json!(["https://example.com/callback#fragment"]),
        ),
        ("redirects", json!([])),
        ("redirects", json!(vec!["https://example.com/callback"; 9])),
        ("name", json!("control\nname")),
        ("name", json!("a".repeat(201))),
    ] {
        let mut saved = baseline.clone();
        saved["clients"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap()[field] = invalid;
        let bytes = serde_json::to_vec(&saved).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            Runtime::open(
                RemotePublicContext::new("https://test.trycloudflare.com").unwrap(),
                path.clone()
            ),
            Err(OAuthError("server_error", "OAUTH_STORAGE_INVALID"))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn token_save_failure_returns_no_credentials_and_fails_current_runtime_closed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oauth.json");
    let mut oauth = persisted_runtime(&path);
    let request = request(&mut oauth);
    let fields = approved(&mut oauth, request);
    let token = oauth.token(&fields).unwrap();
    let probe = oauth.probe_credential().unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let error = oauth.token(&refresh_request(&fields, &token)).unwrap_err();
    assert_eq!(
        (error.0, error.1),
        ("server_error", "OAUTH_STORAGE_WRITE_FAILED")
    );
    assert!(!oauth.validate(token["access_token"].as_str().unwrap()));
    assert!(!oauth.validate(&probe));
    assert!(oauth.probe_credential().is_err());
    assert!(oauth.token(&refresh_request(&fields, &token)).is_err());
    assert!(oauth.pending().is_empty());
    assert!(
        oauth
            .register(json!({"redirect_uris":["https://client.example/callback"]}))
            .is_err()
    );
}

#[test]
fn registration_is_durable_and_failed_registration_cannot_leave_active_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oauth.json");
    let mut oauth = persisted_runtime(&path);
    let request = request(&mut oauth);
    drop(oauth);
    let mut oauth = persisted_runtime(&path);
    assert!(oauth.authorize(request).is_ok());
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(
        oauth
            .register(json!({"redirect_uris":["https://other.example/callback"]}))
            .unwrap_err()
            .1,
        "OAUTH_STORAGE_WRITE_FAILED"
    );
    assert!(oauth.pending().is_empty());
}

#[test]
fn explicit_revocation_removes_durable_grants_and_can_clear_corrupt_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oauth.json");
    let mut oauth = persisted_runtime(&path);
    let request = request(&mut oauth);
    let fields = approved(&mut oauth, request);
    let token = oauth.token(&fields).unwrap();
    oauth.revoke_persisted().unwrap();
    assert!(!path.exists());
    assert!(!oauth.validate(token["access_token"].as_str().unwrap()));
    drop(oauth);
    assert!(!persisted_runtime(&path).validate(token["access_token"].as_str().unwrap()));
    std::fs::write(&path, b"corrupt").unwrap();
    Runtime::clear_store(&path).unwrap();
    Runtime::clear_store(&path).unwrap();
    assert!(!path.exists());
}

#[test]
fn successful_refresh_renews_a_72_hour_window_without_shortening_access_tokens() {
    let mut oauth = runtime();
    let request = request(&mut oauth);
    let fields = approved(&mut oauth, request);
    let mut tokens = oauth.token(&fields).unwrap();
    assert!(
        oauth.grants.values().next().unwrap().expires
            >= Instant::now() + Duration::from_secs(72 * 3600 - 2)
    );
    // Each cycle models a client returning just before its idle window closes.
    for _ in 0..3 {
        oauth.grants.values_mut().next().unwrap().expires =
            Instant::now() + Duration::from_secs(60);
        let refresh = refresh_request(&fields, &tokens);
        tokens = oauth.token(&refresh).unwrap();
        assert_eq!(tokens["expires_in"], 3600);
        assert!(
            oauth.grants.values().next().unwrap().expires
                >= Instant::now() + Duration::from_secs(72 * 3600 - 2)
        );
        assert!(oauth.validate(tokens["access_token"].as_str().unwrap()));
    }
}

#[test]
fn access_and_rejected_refresh_do_not_renew_and_expired_grants_cannot_revive() {
    let mut oauth = runtime();
    let request = request(&mut oauth);
    let fields = approved(&mut oauth, request);
    let tokens = oauth.token(&fields).unwrap();
    let expiry = oauth.grants.values().next().unwrap().expires;
    assert!(oauth.validate(tokens["access_token"].as_str().unwrap()));
    let refresh = refresh_request(&fields, &tokens);
    let mut bad = refresh.clone();
    bad.insert("scope".into(), "admin".into());
    assert!(oauth.token(&bad).is_err());
    assert_eq!(oauth.grants.values().next().unwrap().expires, expiry);
    assert!(!oauth.refresh.values().next().unwrap().used);
    oauth.grants.values_mut().next().unwrap().expires = Instant::now() - Duration::from_secs(1);
    assert!(oauth.token(&refresh).is_err());
    assert!(!oauth.validate(tokens["access_token"].as_str().unwrap()));
    assert!(oauth.grants.is_empty());
}

#[test]
fn sliding_grant_and_client_binding_survive_restart_without_renewing_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oauth.json");
    let mut oauth = persisted_runtime(&path);
    let request = request(&mut oauth);
    let fields = approved(&mut oauth, request);
    let tokens = oauth.token(&fields).unwrap();
    // The unused-registration TTL is shorter than the weekend grant window.
    oauth.clients.values_mut().next().unwrap().expires = Instant::now() - Duration::from_secs(1);
    oauth.grants.values_mut().next().unwrap().expires = Instant::now() + Duration::from_secs(60);
    oauth.persist().unwrap();
    drop(oauth);
    let mut oauth = persisted_runtime(&path);
    let rotated = oauth.token(&refresh_request(&fields, &tokens)).unwrap();
    let saved = std::fs::read(&path).unwrap();
    let grant: Value = serde_json::from_slice(&saved).unwrap();
    let deadline = grant["grants"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()["expires_at_ms"]
        .as_u64()
        .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert!(deadline >= now + (72 * 3600 - 2) * 1000);
    drop(oauth);
    let mut oauth = persisted_runtime(&path);
    assert!(oauth.validate(rotated["access_token"].as_str().unwrap()));
    oauth.persist().unwrap();
    let reloaded: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(grant["grants"], reloaded["grants"]);
    assert!(oauth.token(&refresh_request(&fields, &rotated)).is_ok());
}

#[test]
fn hourly_sliding_refresh_outlives_the_historical_credential_capacity() {
    let mut oauth = runtime();
    let request = request(&mut oauth);
    let fields = approved(&mut oauth, request);
    let mut tokens = oauth.token(&fields).unwrap();
    for _ in 0..MAX_CREDENTIALS + 2 {
        // Advance credential deadlines by an hour without sleeping or renewing them.
        for refresh in oauth.refresh.values_mut() {
            refresh.expires -= Duration::from_secs(3600);
        }
        for access in oauth.access.values_mut() {
            access.expires -= Duration::from_secs(3600);
        }
        oauth.grants.values_mut().next().unwrap().expires -= Duration::from_secs(3600);
        tokens = oauth.token(&refresh_request(&fields, &tokens)).unwrap();
        assert!(oauth.refresh.len() <= 73);
    }
}

#[test]
fn expired_refresh_records_are_pruned_without_revoking_the_renewed_grant() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oauth.json");
    let mut oauth = persisted_runtime(&path);
    let request = request(&mut oauth);
    let fields = approved(&mut oauth, request);
    let old = oauth.token(&fields).unwrap();
    let rotated = oauth.token(&refresh_request(&fields, &old)).unwrap();
    let key = hash(old["refresh_token"].as_str().unwrap());
    oauth.refresh.get_mut(&key).unwrap().expires = Instant::now() - Duration::from_secs(1);
    oauth.persist().unwrap();
    drop(oauth);
    let mut oauth = persisted_runtime(&path);
    assert!(!oauth.refresh.contains_key(&key));
    assert!(oauth.token(&refresh_request(&fields, &old)).is_err());
    assert!(oauth.validate(rotated["access_token"].as_str().unwrap()));
    assert!(oauth.token(&refresh_request(&fields, &rotated)).is_ok());
}

#[test]
fn version_one_refresh_expiry_migrates_without_extending_old_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oauth.json");
    let mut oauth = persisted_runtime(&path);
    let request = request(&mut oauth);
    let fields = approved(&mut oauth, request);
    let tokens = oauth.token(&fields).unwrap();
    let key = hash(tokens["refresh_token"].as_str().unwrap());
    drop(oauth);
    let mut saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    saved["version"] = json!(1);
    for client in saved["clients"].as_object_mut().unwrap().values_mut() {
        client.as_object_mut().unwrap().remove("refresh_allowed");
    }
    saved["refresh"][&key]
        .as_object_mut()
        .unwrap()
        .remove("expires_at_ms");
    std::fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut oauth = persisted_runtime(&path);
    assert_eq!(
        oauth.refresh[&key].expires,
        oauth.grants[&oauth.refresh[&key].family].expires
    );
    assert!(oauth.token(&refresh_request(&fields, &tokens)).is_ok());
    let migrated: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(migrated["version"], 3);
    assert_eq!(
        migrated["refresh"][&key]["expires_at_ms"],
        saved["grants"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()["expires_at_ms"]
    );
}
#[test]
fn code_capacity_retry_preserves_valid_code_but_invalid_bindings_consume() {
    for full_refresh in [false, true] {
        let mut oauth = runtime();
        let request = request(&mut oauth);
        let fields = approved(&mut oauth, request.clone());
        oauth.grants.insert(
            "filled".into(),
            Grant {
                client: request.client_id,
                scope: request.scope,
                expires: Instant::now() + GRANT_TTL,
            },
        );
        for n in 0..MAX_CREDENTIALS {
            if full_refresh {
                oauth.refresh.insert(
                    n.to_string(),
                    Refresh {
                        family: "filled".into(),
                        used: false,
                        expires: Instant::now() + GRANT_TTL,
                    },
                );
            } else {
                oauth.access.insert(
                    n.to_string(),
                    Access {
                        family: "filled".into(),
                        expires: Instant::now() + ACCESS_TTL,
                    },
                );
            }
        }
        let error = oauth.token(&fields).unwrap_err();
        assert_eq!(error.0, "temporarily_unavailable");
        assert_eq!(error.1, "OAUTH_CAPACITY_EXCEEDED");
        assert!(oauth.codes.contains_key(&hash(&fields["code"])));
        assert!(!oauth.used_codes.contains_key(&hash(&fields["code"])));
        oauth.access.clear();
        oauth.refresh.clear();
        let tokens = oauth.token(&fields).unwrap();
        assert!(oauth.validate(tokens["access_token"].as_str().unwrap()));
    }
    for field in ["client_id", "redirect_uri", "code_verifier"] {
        let mut oauth = runtime();
        let request = request(&mut oauth);
        let original = approved(&mut oauth, request);
        let mut bad = original.clone();
        bad.insert(field.into(), "wrong".into());
        assert!(oauth.token(&bad).is_err());
        assert_eq!(oauth.token(&original).unwrap_err().1, "OAUTH_CODE_REPLAY");
    }
}

#[test]
fn pending_deduplication_creation_budget_and_window_wake_are_bounded() {
    let mut oauth = runtime();
    let request = request(&mut oauth);
    let first = oauth.authorize(request.clone()).unwrap();
    assert!(first.created && first.wake_window);
    for _ in 0..100 {
        let duplicate = oauth.authorize(request.clone()).unwrap();
        assert_eq!(duplicate.id, first.id);
        assert!(!duplicate.created && !duplicate.wake_window);
    }
    assert_eq!(oauth.pending().len(), 1);
    for n in 1..4 {
        let mut unique = request.clone();
        unique.state = n.to_string();
        let view = oauth.authorize(unique).unwrap();
        assert!(view.created && !view.wake_window);
    }
    let other = self::request(&mut oauth);
    assert_eq!(
        oauth.authorize(other.clone()).unwrap_err().1,
        "OAUTH_AUTHORIZATION_THROTTLED"
    );
    // Advance only the monotonic budget timestamps; no sleeps or persistent identity.
    for at in &mut oauth.pending_created {
        *at -= Duration::from_secs(11);
    }
    oauth.last_wake = Some(Instant::now() - Duration::from_secs(11));
    assert!(oauth.authorize(other).unwrap().wake_window);
    assert_eq!(oauth.pending().len(), 5);
}

#[test]
fn pending_view_exposes_actual_normalized_scope_before_approval() {
    for scope in ["serena:mcp", "offline_access serena:mcp"] {
        let mut oauth = runtime();
        let mut request = request(&mut oauth);
        request.scope = scope.into();
        let created = oauth.authorize(request).unwrap();
        let pending = oauth.pending();
        let expected = if scope.contains("offline_access") {
            "serena:mcp offline_access"
        } else {
            "serena:mcp"
        };
        assert_eq!(created.scope, expected);
        assert_eq!(pending[0].scope, expected);
        let json = serde_json::to_value(&pending[0]).unwrap();
        assert_eq!(json["scope"], expected);
        assert!(json.get("wakeWindow").is_none() && json.get("created").is_none());
    }
}

#[test]
fn pending_approved_retry_creates_new_code_after_exchange() {
    let mut oauth = runtime();
    let request = request(&mut oauth);
    let first = oauth.authorize(request.clone()).unwrap();
    let a = approved(&mut oauth, request.clone());
    assert!(oauth.token(&a).is_ok());
    let second = oauth.authorize(request.clone()).unwrap();
    assert_ne!(first.id, second.id);
    assert!(second.created);
    assert_eq!(oauth.poll(&second.id)["status"], "pending");
    let b = approved(&mut oauth, request);
    assert_ne!(a["code"], b["code"]);
    assert!(oauth.token(&b).is_ok());
    assert!(oauth.token(&a).is_err());
}

#[test]
fn pending_denied_retry_creates_new_confirmation_and_can_exchange() {
    let mut oauth = runtime();
    let request = request(&mut oauth);
    let first = oauth.authorize(request.clone()).unwrap();
    oauth.decide(&first.id, false).unwrap();
    let second = oauth.authorize(request.clone()).unwrap();
    assert_ne!(first.id, second.id);
    assert!(second.created);
    assert_eq!(oauth.poll(&second.id)["status"], "pending");
    let fields = approved(&mut oauth, request);
    assert!(oauth.token(&fields).is_ok());
}
