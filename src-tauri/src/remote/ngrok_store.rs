use rusqlite::{Connection, OptionalExtension, params};
use std::{fmt, fs, path::Path};

const PROVIDER: &str = "ngrok";
const DATABASE_FILE: &str = "remote-access.db";
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS remote_access_credentials (
    provider TEXT PRIMARY KEY NOT NULL CHECK (provider = 'ngrok'),
    auth_token TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
";

pub(crate) struct NgrokAuthToken(String);

impl NgrokAuthToken {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for NgrokAuthToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NgrokAuthToken([REDACTED])")
    }
}

pub(crate) struct NgrokStore {
    connection: Connection,
}

impl NgrokStore {
    pub(crate) fn open(config_file: &Path) -> Result<Self, String> {
        let directory = config_file
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(directory)
            .map_err(|_| "REMOTE_ACCESS_DATABASE_DIRECTORY_CREATE_FAILED".to_string())?;
        let connection = Connection::open(directory.join(DATABASE_FILE))
            .map_err(|_| "REMOTE_ACCESS_DATABASE_OPEN_FAILED".to_string())?;
        connection
            .execute_batch(SCHEMA)
            .map_err(|_| "REMOTE_ACCESS_DATABASE_INIT_FAILED".to_string())?;
        Ok(Self { connection })
    }

    pub(crate) fn save(&self, auth_token: &NgrokAuthToken) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp_millis();
        self.connection
            .execute(
                "INSERT INTO remote_access_credentials
                    (provider, auth_token, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?3)
                 ON CONFLICT(provider) DO UPDATE SET
                    auth_token = excluded.auth_token,
                    updated_at = excluded.updated_at",
                params![PROVIDER, auth_token.expose(), now],
            )
            .map_err(|_| "REMOTE_ACCESS_DATABASE_SAVE_FAILED".to_string())?;
        Ok(())
    }

    pub(crate) fn read(&self) -> Result<Option<NgrokAuthToken>, String> {
        self.connection
            .query_row(
                "SELECT auth_token FROM remote_access_credentials WHERE provider = ?1",
                [PROVIDER],
                |row| row.get::<_, String>(0).map(NgrokAuthToken::new),
            )
            .optional()
            .map_err(|_| "REMOTE_ACCESS_DATABASE_READ_FAILED".to_string())
    }

    pub(crate) fn delete(&self) -> Result<(), String> {
        self.connection
            .execute(
                "DELETE FROM remote_access_credentials WHERE provider = ?1",
                [PROVIDER],
            )
            .map_err(|_| "REMOTE_ACCESS_DATABASE_DELETE_FAILED".to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::ErrorCode;

    fn config_file(root: &Path) -> std::path::PathBuf {
        root.join("missing").join("config.json")
    }

    #[test]
    fn open_creates_parent_database_and_schema() {
        let root = tempfile::tempdir().unwrap();
        let config_file = config_file(root.path());

        let store = NgrokStore::open(&config_file).unwrap();

        assert!(config_file.parent().unwrap().is_dir());
        assert!(config_file.parent().unwrap().join(DATABASE_FILE).is_file());
        assert_eq!(
            store
                .connection
                .query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = ?1",
                    ["remote_access_credentials"],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn save_read_overwrite_and_delete_ngrok_token() {
        let root = tempfile::tempdir().unwrap();
        let store = NgrokStore::open(&config_file(root.path())).unwrap();
        assert!(store.read().unwrap().is_none());

        store
            .save(&NgrokAuthToken::new("first-test-token".into()))
            .unwrap();
        assert_eq!(store.read().unwrap().unwrap().expose(), "first-test-token");
        let created_at = store
            .connection
            .query_row(
                "SELECT created_at FROM remote_access_credentials WHERE provider = ?1",
                [PROVIDER],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();

        store
            .save(&NgrokAuthToken::new("replacement-test-token".into()))
            .unwrap();
        assert_eq!(
            store.read().unwrap().unwrap().expose(),
            "replacement-test-token"
        );
        let (row_count, saved_created_at, updated_at) = store
            .connection
            .query_row(
                "SELECT count(*), created_at, updated_at
                 FROM remote_access_credentials WHERE provider = ?1",
                [PROVIDER],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row_count, 1);
        assert_eq!(saved_created_at, created_at);
        assert!(updated_at >= created_at);

        store.delete().unwrap();
        assert!(store.read().unwrap().is_none());
    }

    #[test]
    fn saved_token_survives_reopen() {
        let root = tempfile::tempdir().unwrap();
        let config_file = config_file(root.path());
        {
            let store = NgrokStore::open(&config_file).unwrap();
            store
                .save(&NgrokAuthToken::new("persistent-test-token".into()))
                .unwrap();
        }

        let reopened = NgrokStore::open(&config_file).unwrap();
        assert_eq!(
            reopened.read().unwrap().unwrap().expose(),
            "persistent-test-token"
        );
    }

    #[test]
    fn provider_check_rejects_non_ngrok_rows() {
        let root = tempfile::tempdir().unwrap();
        let store = NgrokStore::open(&config_file(root.path())).unwrap();

        let error = store
            .connection
            .execute(
                "INSERT INTO remote_access_credentials
                    (provider, auth_token, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?3)",
                params!["other", "test-token", 1_i64],
            )
            .unwrap_err();

        assert_eq!(
            error.sqlite_error_code(),
            Some(ErrorCode::ConstraintViolation)
        );
        assert!(store.read().unwrap().is_none());
    }

    #[test]
    fn token_debug_is_redacted() {
        let token = NgrokAuthToken::new("must-not-appear".into());

        let debug = format!("{token:?}");

        assert_eq!(debug, "NgrokAuthToken([REDACTED])");
        assert!(!debug.contains(token.expose()));
    }
}
