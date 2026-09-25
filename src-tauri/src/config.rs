use crate::agent::{execution::AgentTaskRole, provider::ProviderId};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};
use tempfile::NamedTempFile;

#[allow(
    dead_code,
    reason = "P2A1-002 provides the helper before P2A1-003 Registry integration."
)]
pub(crate) const WORKSPACE_ROOT_NOT_FOUND: &str = "WORKSPACE_ROOT_NOT_FOUND";
#[allow(
    dead_code,
    reason = "P2A1-002 provides the helper before P2A1-003 Registry integration."
)]
pub(crate) const WORKSPACE_ROOT_NOT_DIRECTORY: &str = "WORKSPACE_ROOT_NOT_DIRECTORY";

/// Validates a registration candidate and returns the filesystem's canonical root.
///
/// This does not persist or otherwise modify `root`; existing stored roots remain
/// the caller's responsibility until an explicit registration operation uses it.
#[allow(
    dead_code,
    reason = "P2A1-002 provides the helper before P2A1-003 Registry integration."
)]
pub(crate) fn canonicalize_workspace_root(root: &Path) -> Result<PathBuf, &'static str> {
    let metadata = fs::metadata(root).map_err(|_| WORKSPACE_ROOT_NOT_FOUND)?;
    if !metadata.is_dir() {
        return Err(WORKSPACE_ROOT_NOT_DIRECTORY);
    }

    fs::canonicalize(root).map_err(|_| WORKSPACE_ROOT_NOT_FOUND)
}

/// Compares workspace roots as identities after callers canonicalize them.
///
/// Historical roots may not have been produced by `canonicalize_workspace_root`,
/// so Windows does not rely on byte-for-byte `Path` equality.
#[cfg(not(windows))]
#[allow(
    dead_code,
    reason = "P2A1-002 provides the helper before P2A1-003 Registry integration."
)]
pub(crate) fn same_workspace_root_identity(left: &Path, right: &Path) -> bool {
    left == right
}

/// Uses Windows' UTF-16 ordinal comparison instead of converting paths through
/// a potentially lossy string representation.
#[cfg(windows)]
#[allow(
    dead_code,
    reason = "P2A1-002 provides the helper before P2A1-003 Registry integration."
)]
pub(crate) fn same_workspace_root_identity(left: &Path, right: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

    let left = left.as_os_str().encode_wide().collect::<Vec<_>>();
    let right = right.as_os_str().encode_wide().collect::<Vec<_>>();
    let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return false;
    };

    // The explicit lengths permit non-NUL-terminated UTF-16 path buffers.
    unsafe {
        CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) == CSTR_EQUAL
    }
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub runtime_directory: PathBuf,
    pub config_file: PathBuf,
    pub log_directory: PathBuf,
    pub app_log: PathBuf,
    pub serena_log: PathBuf,
}

impl AppPaths {
    pub fn resolve(app: &AppHandle) -> Result<Self, String> {
        let config_directory = app
            .path()
            .app_config_dir()
            .map_err(|error| format!("无法确定应用配置目录：{error}"))?;
        let log_directory = app
            .path()
            .app_log_dir()
            .map_err(|error| format!("无法确定应用日志目录：{error}"))?;
        Ok(Self {
            runtime_directory: app
                .path()
                .app_data_dir()
                .map_err(|error| format!("无法确定应用数据目录：{error}"))?
                .join("runtime"),
            config_file: config_directory.join("config.json"),
            app_log: log_directory.join("app.log"),
            serena_log: log_directory.join("serena.log"),
            log_directory,
        })
    }

    pub fn managed_serena(&self) -> PathBuf {
        self.runtime_directory.join("bin").join(if cfg!(windows) {
            "serena.exe"
        } else {
            "serena"
        })
    }
}
pub(crate) const AGENT_PROVIDER_CONFIG_INVALID: &str = "AGENT_PROVIDER_CONFIG_INVALID";

const AGENT_TASK_ROLES: [AgentTaskRole; 5] = [
    AgentTaskRole::Development,
    AgentTaskRole::Testing,
    AgentTaskRole::Review,
    AgentTaskRole::Analysis,
    AgentTaskRole::General,
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProviderPolicy {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProviderSettings {
    pub providers: BTreeMap<String, AgentProviderPolicy>,
    pub role_routing: BTreeMap<String, Option<ProviderId>>,
}

impl Default for AgentProviderSettings {
    fn default() -> Self {
        let codex = ProviderId::new("codex".into()).expect("codex provider id is fixed and valid");
        let providers = BTreeMap::from([
            ("codebuddy".into(), AgentProviderPolicy { enabled: false }),
            ("codex".into(), AgentProviderPolicy { enabled: true }),
        ]);
        let role_routing = AGENT_TASK_ROLES
            .into_iter()
            .map(|role| (role.as_str().into(), Some(codex.clone())))
            .collect();
        Self {
            providers,
            role_routing,
        }
    }
}

impl AgentProviderSettings {
    fn validate(&self) -> Result<(), String> {
        for provider_id in self.providers.keys() {
            ProviderId::new(provider_id.clone()).map_err(|error| {
                format!("{AGENT_PROVIDER_CONFIG_INVALID}: provider id: {error}")
            })?;
        }

        if self.role_routing.len() != AGENT_TASK_ROLES.len()
            || AGENT_TASK_ROLES
                .iter()
                .any(|role| !self.role_routing.contains_key(role.as_str()))
            || self
                .role_routing
                .keys()
                .any(|role| !AGENT_TASK_ROLES.iter().any(|known| known.as_str() == role))
        {
            return Err(format!(
                "{AGENT_PROVIDER_CONFIG_INVALID}: roleRouting must contain exactly development, testing, review, analysis, general"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ManagerConfig {
    pub remote_access: crate::remote::RemoteAccessConfig,
    pub agent_enabled: bool,
    pub agent_providers: AgentProviderSettings,
    pub remote_source_write_enabled: bool,
    pub remote_command_execution_enabled: bool,
    pub agent_success_notification_enabled: bool,
    pub agent_failure_notification_enabled: bool,
    pub agent_system_notification_enabled: bool,
    pub agent_sound_enabled: bool,
    pub broker: BrokerConfig,
    pub workspaces: Vec<Workspace>,
    pub workspace_registry_revision: u64,
    pub desktop_selected_workspace_id: Option<String>,
    pub serena_path: Option<PathBuf>,
    pub port: u16,
    pub dashboard_enabled: bool,
    pub open_dashboard_on_launch: bool,
    pub auto_start_server: bool,
    pub minimize_to_tray: bool,
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            broker: BrokerConfig::default(),
            remote_access: crate::remote::RemoteAccessConfig::default(),
            agent_enabled: false,
            agent_providers: AgentProviderSettings::default(),
            remote_source_write_enabled: false,
            remote_command_execution_enabled: false,
            agent_success_notification_enabled: true,
            agent_failure_notification_enabled: true,
            agent_system_notification_enabled: true,
            agent_sound_enabled: true,
            workspaces: Vec::new(),
            workspace_registry_revision: 1,
            desktop_selected_workspace_id: None,
            serena_path: None,
            port: 19121,
            dashboard_enabled: true,
            open_dashboard_on_launch: false,
            auto_start_server: true,
            minimize_to_tray: true,
        }
    }
}

impl ManagerConfig {
    pub fn validate(&self) -> Result<(), String> {
        self.agent_providers.validate()?;
        if self.remote_access.self_hosted.provider == crate::remote::SelfHostedProvider::CustomHttps
            && let Some(origin) = &self.remote_access.self_hosted.public_origin
        {
            crate::remote::validate_https_origin(origin)?;
        }
        if let Some(origin) = &self.remote_access.mcp_only.public_origin {
            crate::remote::validate_https_origin(origin)?;
        }
        if self.broker.port < 1024 || (self.broker.enabled && self.broker.port == self.port) {
            return Err("Broker 端口必须 >= 1024 且不同于 Serena 端口。".into());
        }
        let mut ids = std::collections::HashSet::new();
        let mut roots = std::collections::HashSet::new();
        for workspace in &self.workspaces {
            if workspace.id.is_empty()
                || workspace.name.trim().is_empty()
                || !ids.insert(&workspace.id)
                || !roots.insert(&workspace.root)
            {
                return Err("项目名称、ID 或路径重复/无效。".into());
            }
        }
        if self.port < 1024 {
            return Err("Serena 内部服务端口必须在 1024–65535 之间。".into());
        }
        if !self.dashboard_enabled && self.open_dashboard_on_launch {
            return Err("Dashboard 已关闭时不能配置为启动时自动打开。".into());
        }
        Ok(())
    }
}

pub fn load(path: &Path) -> Result<ManagerConfig, String> {
    if !path.exists() {
        return Ok(ManagerConfig::default());
    }
    let content = fs::read_to_string(path)
        .map_err(|error| format!("无法读取配置 {}：{error}", path.display()))?;
    let mut config: ManagerConfig = serde_json::from_str(&content)
        .map_err(|error| format!("配置文件格式无效 {}：{error}", path.display()))?;
    if config
        .desktop_selected_workspace_id
        .as_ref()
        .is_some_and(|id| {
            !config
                .workspaces
                .iter()
                .any(|workspace| workspace.id == *id)
        })
    {
        config.desktop_selected_workspace_id = None;
    }
    config.validate()?;
    Ok(config)
}

pub fn save(path: &Path, config: &ManagerConfig) -> Result<(), String> {
    config.validate()?;
    let content =
        serde_json::to_vec_pretty(config).map_err(|error| format!("无法序列化配置：{error}"))?;
    atomic_write(path, &content)
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("配置路径没有父目录：{}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("无法创建配置目录 {}：{error}", parent.display()))?;
    let mut temporary =
        NamedTempFile::new_in(parent).map_err(|error| format!("无法创建临时配置文件：{error}"))?;
    temporary
        .write_all(content)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| format!("无法写入临时配置文件：{error}"))?;
    temporary
        .persist(path)
        .map_err(|error| format!("无法替换配置文件 {}：{}", path.display(), error.error))?;
    Ok(())
}

/// Workspace Slot 受管 Serena context 的固定 Semantic 工具白名单。
/// 该列表同时用于写入与回读校验，避免两处配置语义漂移。
const WORKSPACE_SERENA_FIXED_TOOLS: &[&str] = &[
    "activate_project",
    "get_current_config",
    "get_symbols_overview",
    "find_symbol",
    "find_referencing_symbols",
];

/// 为 workspace-scoped Serena Slot 写入独立且最小的受管全局配置。
/// Slot Home 不从旧共享 Home 迁移 projects；Workspace authority 始终来自 Lease。
pub(crate) fn prepare_workspace_serena_home(home: &Path, context: &Path) -> Result<(), String> {
    let value = serde_json::json!({
        "language_backend": "LSP", "trusted_project_path_patterns": [],
        "web_dashboard": false, "web_dashboard_open_on_launch": false,
        "gui_log_window": false, "default_modes": [], "projects": [],
        "project_serena_folder_location": "$projectDir/.serena"
    });
    atomic_write(
        &home.join("serena_config.yml"),
        serde_json::to_string_pretty(&value).unwrap().as_bytes(),
    )?;
    let context_value = serde_json::json!({
        "description":"Desktop workspace-scoped semantic source backend", "prompt":"",
        "fixed_tools": WORKSPACE_SERENA_FIXED_TOOLS,
        "single_project":false
    });
    atomic_write(
        context,
        serde_json::to_string_pretty(&context_value)
            .unwrap()
            .as_bytes(),
    )?;
    verify_workspace_serena_home(home, context)
}

/// 验证 Slot 配置仍保持固定安全基线，避免启动时接受被外部修改的 Home。
pub(crate) fn verify_workspace_serena_home(home: &Path, context: &Path) -> Result<(), String> {
    let global: serde_yaml_ng::Value = serde_yaml_ng::from_str(
        &fs::read_to_string(home.join("serena_config.yml")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let context_value: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&fs::read_to_string(context).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let safe_global = global["trusted_project_path_patterns"]
        .as_sequence()
        .is_some_and(|patterns| patterns.is_empty())
        && global["web_dashboard"].as_bool() == Some(false)
        && global["web_dashboard_open_on_launch"].as_bool() == Some(false)
        && global["gui_log_window"].as_bool() == Some(false)
        && global["project_serena_folder_location"].as_str() == Some("$projectDir/.serena")
        && global["projects"]
            .as_sequence()
            .is_some_and(|projects| projects.is_empty());
    let has_fixed_tools = context_value["fixed_tools"]
        .as_sequence()
        .is_some_and(|tools| {
            tools.len() == WORKSPACE_SERENA_FIXED_TOOLS.len()
                && WORKSPACE_SERENA_FIXED_TOOLS.iter().all(|allowed| {
                    tools
                        .iter()
                        .filter(|tool| tool.as_str() == Some(*allowed))
                        .count()
                        == 1
                })
        });
    let safe_context = context_value["single_project"].as_bool() == Some(false) && has_fixed_tools;
    (safe_global && safe_context)
        .then_some(())
        .ok_or_else(|| "managed Serena slot config is invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn app_paths_resolve_from_tauri_identity_and_preserve_unicode_suffixes() {
        use tauri::Manager;

        let mut context = tauri::generate_context!();
        context.config_mut().app.windows.clear();
        let app = tauri::Builder::default()
            .any_thread()
            .build(context)
            .expect("测试应用上下文必须可构建");
        let handle = app.handle();
        let paths = AppPaths::resolve(handle).expect("生产 Path API 必须能解析应用路径");
        let config_dir = handle
            .path()
            .app_config_dir()
            .expect("生产 Path API 必须能解析配置目录");
        let data_dir = handle
            .path()
            .app_data_dir()
            .expect("生产 Path API 必须能解析数据目录");
        let log_dir = handle
            .path()
            .app_log_dir()
            .expect("生产 Path API 必须能解析日志目录");

        assert_eq!(paths.config_file, config_dir.join("config.json"));
        assert_eq!(paths.runtime_directory, data_dir.join("runtime"));
        assert_eq!(paths.log_directory, log_dir);
        assert_eq!(
            paths.runtime_directory.join("oauth-state.json"),
            data_dir.join("runtime").join("oauth-state.json")
        );

        // Unicode fixture verifies the production PathBuf suffixes do not use ANSI conversion.
        let unicode_data =
            PathBuf::from(r"C:\Users\张三\AppData\Roaming\io.github.lifei6671.serena-desktop");
        assert_eq!(
            unicode_data.join("runtime").join("oauth-state.json"),
            PathBuf::from(
                r"C:\Users\张三\AppData\Roaming\io.github.lifei6671.serena-desktop\runtime\oauth-state.json"
            )
        );

        println!(
            "P6-003_PATH_SNAPSHOT identifier={} configDir={} dataDir={} logDir={} managerConfig={} agentStateDb={} oauthState={}",
            handle.config().identifier,
            config_dir.display(),
            data_dir.display(),
            log_dir.display(),
            paths.config_file.display(),
            data_dir.join("agent-state.db").display(),
            paths.runtime_directory.join("oauth-state.json").display(),
        );
    }

    #[test]
    fn workspace_root_missing_returns_stable_error() {
        let directory = tempfile::tempdir().unwrap();

        assert_eq!(
            canonicalize_workspace_root(&directory.path().join("missing")),
            Err(WORKSPACE_ROOT_NOT_FOUND)
        );
    }

    #[test]
    fn workspace_root_file_returns_stable_error() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("not-a-directory");
        fs::write(&file, "file").unwrap();

        assert_eq!(
            canonicalize_workspace_root(&file),
            Err(WORKSPACE_ROOT_NOT_DIRECTORY)
        );
    }

    #[test]
    fn workspace_root_non_git_directory_canonicalizes_without_side_effects() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("ordinary-directory");
        fs::create_dir(&root).unwrap();

        let canonical = canonicalize_workspace_root(&root).unwrap();

        assert!(canonical.is_dir());
        assert!(!root.join(".git").exists());
        assert!(!root.join(".serena").exists());
        assert!(!root.join(".codegraph").exists());
        assert!(fs::read_dir(&root).unwrap().next().is_none());
    }

    #[test]
    fn workspace_root_same_root_alias_has_the_same_identity() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        fs::create_dir(&root).unwrap();
        let alias = root
            .parent()
            .unwrap()
            .join(root.file_name().unwrap())
            .join("..")
            .join(root.file_name().unwrap());

        let canonical = canonicalize_workspace_root(&root).unwrap();
        let alias_canonical = canonicalize_workspace_root(&alias).unwrap();

        assert!(same_workspace_root_identity(&canonical, &alias_canonical));
    }

    #[cfg(windows)]
    fn windows_path_with_forward_separators(path: &Path) -> PathBuf {
        use std::{
            ffi::OsString,
            os::windows::ffi::{OsStrExt, OsStringExt},
        };

        let wide = path
            .as_os_str()
            .encode_wide()
            .map(|unit| {
                if unit == u16::from(b'\\') {
                    u16::from(b'/')
                } else {
                    unit
                }
            })
            .collect::<Vec<_>>();
        PathBuf::from(OsString::from_wide(&wide))
    }

    #[cfg(windows)]
    fn windows_ascii_uppercase_path(path: &Path) -> PathBuf {
        use std::{
            ffi::OsString,
            os::windows::ffi::{OsStrExt, OsStringExt},
        };

        let wide = path
            .as_os_str()
            .encode_wide()
            .map(|unit| {
                if (u16::from(b'a')..=u16::from(b'z')).contains(&unit) {
                    unit - u16::from(b'a') + u16::from(b'A')
                } else {
                    unit
                }
            })
            .collect::<Vec<_>>();
        PathBuf::from(OsString::from_wide(&wide))
    }

    #[cfg(windows)]
    #[test]
    fn windows_workspace_root_separator_alias_has_the_same_identity() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        fs::create_dir(&root).unwrap();
        let separator_alias = windows_path_with_forward_separators(&root);

        let canonical = canonicalize_workspace_root(&root).unwrap();
        let alias_canonical = canonicalize_workspace_root(&separator_alias).unwrap();

        assert!(same_workspace_root_identity(&canonical, &alias_canonical));
    }

    #[cfg(windows)]
    #[test]
    fn windows_workspace_root_casing_alias_has_the_same_identity() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("RootIdentity");
        fs::create_dir(&root).unwrap();

        let canonical = canonicalize_workspace_root(&root).unwrap();
        let casing_alias = windows_ascii_uppercase_path(&canonical);
        let alias_canonical = canonicalize_workspace_root(&casing_alias).unwrap();

        assert!(same_workspace_root_identity(&canonical, &casing_alias));
        assert!(same_workspace_root_identity(&canonical, &alias_canonical));
    }

    #[test]
    fn missing_saved_executable_can_be_loaded_for_repair_in_ui() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let config = ManagerConfig {
            serena_path: Some(dir.path().join("removed.exe")),
            ..ManagerConfig::default()
        };
        save(&path, &config).unwrap();
        assert_eq!(load(&path).unwrap(), config);
    }

    #[test]
    fn old_serena_port_9120_loads_with_new_broker_default() {
        let mut config: ManagerConfig = serde_json::from_str(r#"{"port":9120}"#).unwrap();
        assert!(!config.broker.enabled);
        assert_eq!(config.broker.port, 19120);
        assert!(config.validate().is_ok());
        config.broker.enabled = true;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn default_config_is_valid() {
        let config = ManagerConfig::default();
        assert_eq!(config.port, 19121);
        assert_eq!(config.broker.port, 19120);
        assert_eq!(config.workspace_registry_revision, 1);
        assert_eq!(config.desktop_selected_workspace_id, None);
        assert!(config.agent_success_notification_enabled);
        assert!(config.agent_failure_notification_enabled);
        assert!(config.agent_system_notification_enabled);
        assert!(config.agent_sound_enabled);
        assert!(config.validate().is_ok());
    }

    /// 验证缺失端口采用新默认值，显式旧端口继续按用户配置加载。
    #[test]
    fn serena_port_defaults_only_when_missing_and_preserves_explicit_legacy_port() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        fs::write(&path, "{}").unwrap();
        let missing_port = load(&path).unwrap();
        assert_eq!(missing_port.port, 19121);
        assert_eq!(missing_port.broker.port, 19120);

        fs::write(&path, r#"{"port":9121}"#).unwrap();
        let explicit_legacy_port = load(&path).unwrap();
        assert_eq!(explicit_legacy_port.port, 9121);
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"port":9121}"#);

        let mut invalid_port = missing_port;
        invalid_port.port = 1023;
        assert_eq!(
            invalid_port.validate().unwrap_err(),
            "Serena 内部服务端口必须在 1024–65535 之间。"
        );
    }

    #[test]
    fn missing_broker_uses_new_default_and_explicit_legacy_port_is_preserved() {
        let new_config: ManagerConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(new_config.broker.port, 19120);

        let legacy_config: ManagerConfig =
            serde_json::from_str(r#"{"broker":{"enabled":false,"port":9120}}"#).unwrap();
        assert_eq!(legacy_config.broker.port, 9120);
    }

    #[test]
    fn old_config_uses_agent_notification_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        fs::write(&path, r#"{"agentEnabled":true}"#).unwrap();
        let config = load(&path).unwrap();
        assert!(config.agent_success_notification_enabled);
        assert!(config.agent_failure_notification_enabled);
        assert!(config.agent_system_notification_enabled);
        assert!(config.agent_sound_enabled);
    }

    #[test]
    fn stale_desktop_selected_workspace_loads_as_unselected_without_rewriting_config() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        fs::write(
            &path,
            r#"{"desktopSelectedWorkspaceId":"missing","workspaces":[{"id":"known","name":"Known","root":"C:\\known","generation":4}]}"#,
        )
        .unwrap();
        let bytes = fs::read(&path).unwrap();

        let loaded = load(&path).unwrap();

        assert_eq!(loaded.desktop_selected_workspace_id, None);
        assert_eq!(loaded.workspaces[0].id, "known");
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn legacy_workspace_registry_versions_migrate_and_round_trip_without_identity_changes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        fs::write(
            &path,
            r#"{
                "workspaces": [
                    {"id":"legacy-b","name":"Legacy B","root":"C:\\legacy\\b"},
                    {"id":"legacy-a","name":"Legacy A","root":"C:\\legacy\\a"}
                ]
            }"#,
        )
        .unwrap();
        let before = fs::read(&path).unwrap();

        let expected_workspaces = vec![
            Workspace {
                id: "legacy-b".into(),
                name: "Legacy B".into(),
                root: PathBuf::from(r"C:\legacy\b"),
                generation: 1,
            },
            Workspace {
                id: "legacy-a".into(),
                name: "Legacy A".into(),
                root: PathBuf::from(r"C:\legacy\a"),
                generation: 1,
            },
        ];

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.workspace_registry_revision, 1);
        assert_eq!(loaded.workspaces, expected_workspaces);
        assert_eq!(fs::read(&path).unwrap(), before);

        save(&path, &loaded).unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["workspaceRegistryRevision"], 1);
        assert_eq!(
            saved["workspaces"],
            serde_json::json!([
                {"id":"legacy-b","name":"Legacy B","root":"C:\\legacy\\b","generation":1},
                {"id":"legacy-a","name":"Legacy A","root":"C:\\legacy\\a","generation":1}
            ])
        );
        assert_eq!(load(&path).unwrap(), loaded);
    }

    #[test]
    fn legacy_self_hosted_config_defaults_to_custom_https() {
        let config: ManagerConfig = serde_json::from_str(
            r#"{"remoteAccess":{"mode":"self_hosted_oauth","selfHosted":{"publicOrigin":"https://legacy.example"}}}"#,
        )
        .unwrap();

        assert_eq!(
            config.remote_access.self_hosted.provider,
            crate::remote::SelfHostedProvider::CustomHttps
        );
        assert_eq!(
            config.remote_access.self_hosted.public_origin.as_deref(),
            Some("https://legacy.example")
        );
        assert!(!config.remote_access.quick_tunnel_desired_running);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn custom_https_rejects_invalid_public_origin() {
        let mut config = ManagerConfig::default();
        config.remote_access.self_hosted.provider = crate::remote::SelfHostedProvider::CustomHttps;
        config.remote_access.self_hosted.public_origin = Some("http://example.com".into());

        assert_eq!(config.validate().unwrap_err(), "PUBLIC_ORIGIN_INVALID");
    }

    #[test]
    fn managed_self_hosted_providers_ignore_residual_public_origin() {
        for provider in [
            crate::remote::SelfHostedProvider::Ngrok,
            crate::remote::SelfHostedProvider::TailscaleFunnel,
        ] {
            let mut config = ManagerConfig::default();
            config.remote_access.self_hosted.provider = provider;
            config.remote_access.self_hosted.public_origin = Some("not-an-origin".into());

            assert!(config.validate().is_ok(), "{provider:?}");
        }
    }

    #[test]
    fn old_config_uses_agent_provider_defaults_without_rewriting_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        let legacy = r#"{"agentEnabled":true}"#;
        fs::write(&path, legacy).unwrap();

        let config = load(&path).unwrap();

        assert!(config.agent_enabled);
        assert_eq!(config.agent_providers.providers.len(), 2);
        assert!(config.agent_providers.providers["codex"].enabled);
        assert!(!config.agent_providers.providers["codebuddy"].enabled);
        for role in AGENT_TASK_ROLES {
            assert_eq!(
                config
                    .agent_providers
                    .role_routing
                    .get(role.as_str())
                    .and_then(Option::as_ref)
                    .map(ProviderId::as_str),
                Some("codex"),
                "{}",
                role.as_str()
            );
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), legacy);
    }

    #[test]
    fn agent_provider_settings_round_trip_future_provider_and_optional_route() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        let mut config = ManagerConfig::default();
        config
            .agent_providers
            .providers
            .insert("future-acp".into(), AgentProviderPolicy { enabled: true });
        config.agent_providers.role_routing.insert(
            AgentTaskRole::Testing.as_str().into(),
            Some(ProviderId::new("future-acp".into()).unwrap()),
        );
        config
            .agent_providers
            .role_routing
            .insert(AgentTaskRole::Review.as_str().into(), None);

        assert!(config.validate().is_ok());
        save(&path, &config).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded, config);

        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved["agentProviders"]["providers"]["future-acp"]["enabled"],
            true
        );
        assert_eq!(
            saved["agentProviders"]["roleRouting"]["testing"],
            "future-acp"
        );
        assert!(saved["agentProviders"]["roleRouting"]["review"].is_null());
    }

    #[test]
    fn agent_provider_settings_reject_invalid_role_or_provider_id() {
        let mut invalid_role = ManagerConfig::default();
        invalid_role
            .agent_providers
            .role_routing
            .remove(AgentTaskRole::Testing.as_str());
        invalid_role.agent_providers.role_routing.insert(
            "deploy".into(),
            Some(ProviderId::new("codex".into()).unwrap()),
        );
        assert_eq!(
            invalid_role.validate().unwrap_err(),
            format!(
                "{AGENT_PROVIDER_CONFIG_INVALID}: roleRouting must contain exactly development, testing, review, analysis, general"
            )
        );

        let mut invalid_provider = ManagerConfig::default();
        invalid_provider
            .agent_providers
            .providers
            .insert("bad provider".into(), AgentProviderPolicy { enabled: true });
        assert_eq!(
            invalid_provider.validate().unwrap_err(),
            format!(
                "{AGENT_PROVIDER_CONFIG_INVALID}: provider id: provider id must not contain whitespace or control characters"
            )
        );

        let mut invalid_route_value = serde_json::to_value(ManagerConfig::default()).unwrap();
        invalid_route_value["agentProviders"]["roleRouting"]["testing"] =
            serde_json::json!("bad provider");
        assert!(
            serde_json::from_value::<ManagerConfig>(invalid_route_value)
                .unwrap_err()
                .to_string()
                .contains("provider id must not contain whitespace or control characters")
        );
    }

    #[test]
    fn agent_tools_are_opt_in_and_persisted() {
        let mut config: ManagerConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.agent_enabled);
        config.agent_enabled = true;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        save(&path, &config).unwrap();
        assert!(load(&path).unwrap().agent_enabled);
        assert_eq!(serde_json::to_value(&config).unwrap()["agentEnabled"], true);
    }

    #[test]
    fn remote_command_execution_is_opt_in_and_persisted() {
        let mut config: ManagerConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.remote_command_execution_enabled);
        config.remote_command_execution_enabled = true;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        save(&path, &config).unwrap();
        assert!(load(&path).unwrap().remote_command_execution_enabled);
        assert_eq!(
            serde_json::to_value(&config).unwrap()["remoteCommandExecutionEnabled"],
            true
        );
    }

    #[test]
    fn remote_source_write_is_opt_in_and_persisted() {
        let mut config: ManagerConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.remote_source_write_enabled);
        config.remote_source_write_enabled = true;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        save(&path, &config).unwrap();
        assert!(load(&path).unwrap().remote_source_write_enabled);
        assert_eq!(
            serde_json::to_value(&config).unwrap()["remoteSourceWriteEnabled"],
            true
        );
    }

    #[test]
    fn broker_lan_is_opt_in_and_persisted() {
        let mut config: ManagerConfig =
            serde_json::from_str(r#"{"broker":{"enabled":true,"port":9120}}"#).unwrap();
        assert!(!config.broker.allow_lan);
        assert_eq!(config.broker.bind_address().to_string(), "127.0.0.1:9120");
        config.broker.allow_lan = true;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        save(&path, &config).unwrap();
        let loaded = load(&path).unwrap();
        assert!(loaded.broker.allow_lan);
        assert_eq!(loaded.broker.bind_address().to_string(), "0.0.0.0:9120");
        assert_eq!(
            serde_json::to_value(&loaded).unwrap()["broker"]["allowLan"],
            true
        );
    }

    #[test]
    fn rejects_reserved_port() {
        let config = ManagerConfig {
            port: 80,
            ..ManagerConfig::default()
        };
        assert!(config.validate().unwrap_err().contains("1024"));
    }

    #[test]
    fn save_atomically_replaces_an_existing_config() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        let mut config = ManagerConfig::default();
        save(&path, &config).unwrap();
        config.port = 9122;
        save(&path, &config).unwrap();
        assert_eq!(load(&path).unwrap().port, 9122);
    }

    #[test]
    fn legacy_start_minimized_field_is_ignored() {
        let config: ManagerConfig = serde_json::from_str(
            r#"{"startMinimized":true,"autoStartServer":false,"minimizeToTray":true}"#,
        )
        .unwrap();

        assert!(!config.auto_start_server);
        assert!(
            !serde_json::to_string(&config)
                .unwrap()
                .contains("startMinimized")
        );
    }

    #[test]
    fn workspace_serena_home_accepts_generated_fixed_tools() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("slot-home");
        let context = home.join("slot-context.yml");
        fs::create_dir(&home).unwrap();

        prepare_workspace_serena_home(&home, &context).unwrap();

        assert!(verify_workspace_serena_home(&home, &context).is_ok());
    }

    #[test]
    fn workspace_serena_home_rejects_extra_fixed_tool() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("slot-home");
        let context = home.join("slot-context.yml");
        fs::create_dir(&home).unwrap();
        prepare_workspace_serena_home(&home, &context).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&context).unwrap()).unwrap();
        value["fixed_tools"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!("write_file"));
        fs::write(&context, serde_json::to_string_pretty(&value).unwrap()).unwrap();

        assert!(verify_workspace_serena_home(&home, &context).is_err());
    }

    #[test]
    fn workspace_serena_home_rejects_missing_fixed_tool() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("slot-home");
        let context = home.join("slot-context.yml");
        fs::create_dir(&home).unwrap();
        prepare_workspace_serena_home(&home, &context).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&context).unwrap()).unwrap();
        value["fixed_tools"].as_array_mut().unwrap().pop();
        fs::write(&context, serde_json::to_string_pretty(&value).unwrap()).unwrap();

        assert!(verify_workspace_serena_home(&home, &context).is_err());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct BrokerConfig {
    pub enabled: bool,
    pub port: u16,
    pub allow_lan: bool,
}
impl Default for BrokerConfig {
    /// 为未持久化 Broker 配置提供新的低冲突默认端口。
    fn default() -> Self {
        Self {
            enabled: false,
            port: 19120,
            allow_lan: false,
        }
    }
}
impl BrokerConfig {
    pub fn bind_address(&self) -> std::net::SocketAddr {
        let ip = if self.allow_lan {
            std::net::Ipv4Addr::UNSPECIFIED
        } else {
            std::net::Ipv4Addr::LOCALHOST
        };
        (ip, self.port).into()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    #[serde(default = "default_workspace_generation")]
    pub generation: u64,
}

fn default_workspace_generation() -> u64 {
    1
}
impl AppPaths {
    pub fn serena_home(&self) -> PathBuf {
        self.runtime_directory.join("serena-home")
    }
    pub fn broker_context(&self) -> PathBuf {
        self.serena_home().join("broker.yml")
    }
    pub fn prepare_serena(&self, dashboard: bool) -> Result<(), String> {
        let projects = match fs::read_to_string(self.serena_home().join("serena_config.yml")) {
            Ok(text) => {
                let previous: serde_yaml_ng::Value =
                    serde_yaml_ng::from_str(&text).map_err(|e| e.to_string())?;
                serde_json::to_value(&previous["projects"]).map_err(|e| e.to_string())?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!([]),
            Err(e) => return Err(e.to_string()),
        };
        let value = serde_json::json!({
            "language_backend": "LSP", "trusted_project_path_patterns": [],
            "web_dashboard": dashboard, "web_dashboard_open_on_launch": false,
            "gui_log_window": false, "default_modes": [], "projects": projects,
            "project_serena_folder_location": "$projectDir/.serena"
        });
        atomic_write(
            &self.serena_home().join("serena_config.yml"),
            serde_json::to_string_pretty(&value).unwrap().as_bytes(),
        )?;
        let context = serde_json::json!({"description":"Desktop Broker semantic source backend", "prompt":"", "fixed_tools":["activate_project","get_current_config","get_symbols_overview","find_symbol","find_referencing_symbols"], "single_project":false});
        atomic_write(
            &self.broker_context(),
            serde_json::to_string_pretty(&context).unwrap().as_bytes(),
        )
    }
}
impl AppPaths {
    pub fn verify_serena_config(&self) -> Result<(), String> {
        let text = fs::read_to_string(self.serena_home().join("serena_config.yml"))
            .map_err(|e| e.to_string())?;
        let value: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&text).map_err(|e| e.to_string())?;
        if !value["trusted_project_path_patterns"]
            .as_sequence()
            .is_some_and(|v| v.is_empty())
        {
            return Err(
                "BACKEND_INCOMPATIBLE: 受管 Serena 必须禁止项目信任命令，请重启服务恢复配置。"
                    .into(),
            );
        }
        Ok(())
    }
}
