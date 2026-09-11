//! Versioned local OAuth state. Bearer credentials and authorization flows stay
//! out of this file; only credential digests and their durable bindings are saved.
use super::*;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::SystemTime,
};

const STORE_VERSION: u32 = 3;

pub(super) struct Store {
    path: PathBuf,
    clock: Clock,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRuntime {
    version: u32,
    issuer: String,
    resource: String,
    clients: HashMap<String, StoredClient>,
    grants: HashMap<String, StoredGrant>,
    access: HashMap<String, StoredAccess>,
    refresh: HashMap<String, StoredRefresh>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredClient {
    name: String,
    redirects: Vec<String>,
    #[serde(default)]
    refresh_allowed: Option<bool>,
    expires_at_ms: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredGrant {
    client: String,
    scope: String,
    expires_at_ms: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredAccess {
    family: String,
    expires_at_ms: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRefresh {
    family: String,
    used: bool,
    #[serde(default)]
    expires_at_ms: Option<u64>,
}

fn storage_error(code: &'static str) -> OAuthError {
    OAuthError("server_error", code)
}

// Map monotonic deadlines to absolute wall time, never to a fresh TTL on load.
struct Clock {
    instant: Instant,
    unix_ms: u64,
}
impl Clock {
    fn now() -> Result<Self> {
        let instant = Instant::now();
        let unix_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .and_then(|d| u64::try_from(d.as_millis()).ok())
            .ok_or(storage_error("OAUTH_STORAGE_CLOCK_INVALID"))?;
        Ok(Self { instant, unix_ms })
    }
    fn save_deadline(&self, deadline: Instant) -> Result<u64> {
        let remaining_ms =
            u64::try_from(deadline.saturating_duration_since(self.instant).as_millis())
                .map_err(|_| storage_error("OAUTH_STORAGE_CLOCK_INVALID"))?;
        self.unix_ms
            .checked_add(remaining_ms)
            .ok_or(storage_error("OAUTH_STORAGE_CLOCK_INVALID"))
    }
    fn load_deadline(&self, expires_at_ms: u64) -> Result<Instant> {
        self.instant
            .checked_add(Duration::from_millis(
                expires_at_ms.saturating_sub(self.unix_ms),
            ))
            .ok_or(storage_error("OAUTH_STORAGE_INVALID"))
    }
}

impl Runtime {
    pub fn open(context: RemotePublicContext, path: PathBuf) -> Result<Self> {
        let mut runtime = Self::new(context);
        let clock = Clock::now()?;
        let bytes = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(storage_error("OAUTH_STORAGE_READ_FAILED")),
        };
        if let Some(bytes) = bytes {
            let stored: StoredRuntime = serde_json::from_slice(&bytes)
                .map_err(|_| storage_error("OAUTH_STORAGE_INVALID"))?;
            if !(1..=STORE_VERSION).contains(&stored.version) {
                return Err(storage_error("OAUTH_STORAGE_VERSION_UNSUPPORTED"));
            }
            if stored.issuer != runtime.context.public_origin
                || stored.resource != runtime.context.mcp_resource
            {
                return Err(storage_error("OAUTH_STORAGE_ORIGIN_MISMATCH"));
            }
            if stored.clients.len() > MAX_CLIENTS
                || stored.grants.len() > MAX_CLIENTS
                || stored.access.len() > MAX_CREDENTIALS
                || stored.refresh.len() > MAX_CREDENTIALS
                || stored.clients.values().any(|client| {
                    (stored.version >= 3 && client.refresh_allowed.is_none())
                        || client.name.len() > 200
                        || client.name.chars().any(char::is_control)
                        || client.redirects.is_empty()
                        || client.redirects.len() > 8
                        || client
                            .redirects
                            .iter()
                            .any(|redirect| validate_redirect(redirect).is_err())
                })
                || stored.grants.values().any(|grant| {
                    !stored.clients.contains_key(&grant.client)
                        || !matches!(
                            grant.scope.as_str(),
                            "serena:mcp" | "serena:mcp offline_access"
                        )
                })
                || stored.access.iter().any(|(digest, token)| {
                    !valid_digest(digest) || !stored.grants.contains_key(&token.family)
                })
                || stored.refresh.iter().any(|(digest, token)| {
                    !valid_digest(digest)
                        || (stored.version >= 2 && token.expires_at_ms.is_none())
                        || !stored.grants.get(&token.family).is_some_and(|g| {
                            (stored.version >= 3 || g.scope == "serena:mcp offline_access")
                                && stored
                                    .clients
                                    .get(&g.client)
                                    .is_some_and(|c| c.refresh_allowed.unwrap_or(true))
                        })
                })
            {
                return Err(storage_error("OAUTH_STORAGE_INVALID"));
            }
            for (id, client) in stored.clients {
                runtime.clients.insert(
                    id,
                    Client {
                        name: client.name,
                        redirects: client.redirects,
                        // v1/v2 registration responses always permitted both flows.
                        refresh_allowed: client.refresh_allowed.unwrap_or(true),
                        expires: clock.load_deadline(client.expires_at_ms)?,
                    },
                );
            }
            for (id, grant) in stored.grants {
                runtime.grants.insert(
                    id,
                    Grant {
                        client: grant.client,
                        scope: grant.scope,
                        expires: clock.load_deadline(grant.expires_at_ms)?,
                    },
                );
            }
            for (digest, access) in stored.access {
                runtime.access.insert(
                    digest,
                    Access {
                        family: access.family,
                        expires: clock.load_deadline(access.expires_at_ms)?,
                    },
                );
            }
            for (digest, refresh) in stored.refresh {
                // Version 1 used the fixed Grant deadline for every Refresh Token.
                let expires = match refresh.expires_at_ms {
                    Some(value) => clock.load_deadline(value)?,
                    None => runtime.grants[&refresh.family].expires,
                };
                runtime.refresh.insert(
                    digest,
                    Refresh {
                        family: refresh.family,
                        used: refresh.used,
                        expires,
                    },
                );
            }
            runtime.prune();
        }
        runtime.storage = Some(Store { path, clock });
        Ok(runtime)
    }

    pub fn clear_store(path: &Path) -> Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(storage_error("OAUTH_STORAGE_REMOVE_FAILED")),
        }
    }

    pub fn revoke_persisted(&mut self) -> Result<()> {
        self.fail_closed();
        if let Some(store) = &self.storage {
            Self::clear_store(&store.path)?;
        }
        Ok(())
    }

    pub(super) fn ensure_storage(&self) -> Result<()> {
        if self.storage_failed {
            Err(storage_error("OAUTH_STORAGE_UNAVAILABLE"))
        } else {
            Ok(())
        }
    }

    fn fail_closed(&mut self) {
        self.storage_failed = true;
        self.probe = None;
        self.clients.clear();
        self.pending.clear();
        self.codes.clear();
        self.used_codes.clear();
        self.access.clear();
        self.refresh.clear();
        self.grants.clear();
    }

    pub(super) fn persist(&mut self) -> Result<()> {
        self.ensure_storage()?;
        let Some(store) = self.storage.as_ref() else {
            return Ok(());
        };
        let result = self.write_snapshot(store);
        if result.is_err() {
            self.fail_closed();
        }
        result
    }

    fn write_snapshot(&self, store: &Store) -> Result<()> {
        // Keep this anchor for the Runtime's lifetime, so saving does not shift
        // absolute deadlines after wall-clock adjustments or repeated reloads.
        let clock = &store.clock;
        let path = &store.path;
        let stored = StoredRuntime {
            version: STORE_VERSION,
            issuer: self.context.public_origin.clone(),
            resource: self.context.mcp_resource.clone(),
            clients: self
                .clients
                .iter()
                .map(|(id, client)| {
                    Ok((
                        id.clone(),
                        StoredClient {
                            name: client.name.clone(),
                            redirects: client.redirects.clone(),
                            refresh_allowed: Some(client.refresh_allowed),
                            expires_at_ms: clock.save_deadline(client.expires)?,
                        },
                    ))
                })
                .collect::<Result<_>>()?,
            grants: self
                .grants
                .iter()
                .map(|(id, grant)| {
                    Ok((
                        id.clone(),
                        StoredGrant {
                            client: grant.client.clone(),
                            scope: grant.scope.clone(),
                            expires_at_ms: clock.save_deadline(grant.expires)?,
                        },
                    ))
                })
                .collect::<Result<_>>()?,
            access: self
                .access
                .iter()
                .map(|(digest, access)| {
                    Ok((
                        digest.clone(),
                        StoredAccess {
                            family: access.family.clone(),
                            expires_at_ms: clock.save_deadline(access.expires)?,
                        },
                    ))
                })
                .collect::<Result<_>>()?,
            refresh: self
                .refresh
                .iter()
                .map(|(digest, refresh)| {
                    Ok((
                        digest.clone(),
                        StoredRefresh {
                            family: refresh.family.clone(),
                            used: refresh.used,
                            expires_at_ms: Some(clock.save_deadline(refresh.expires)?),
                        },
                    ))
                })
                .collect::<Result<_>>()?,
        };
        let bytes =
            serde_json::to_vec(&stored).map_err(|_| storage_error("OAUTH_STORAGE_WRITE_FAILED"))?;
        let parent = path
            .parent()
            .ok_or(storage_error("OAUTH_STORAGE_WRITE_FAILED"))?;
        fs::create_dir_all(parent).map_err(|_| storage_error("OAUTH_STORAGE_WRITE_FAILED"))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|_| storage_error("OAUTH_STORAGE_WRITE_FAILED"))?;
        temporary
            .write_all(&bytes)
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|_| storage_error("OAUTH_STORAGE_WRITE_FAILED"))?;
        temporary
            .persist(path)
            .map_err(|_| storage_error("OAUTH_STORAGE_WRITE_FAILED"))?;
        Ok(())
    }
}

fn valid_digest(value: &str) -> bool {
    URL_SAFE_NO_PAD
        .decode(value)
        .is_ok_and(|bytes| bytes.len() == 32)
}
