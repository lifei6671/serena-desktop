use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};
use tempfile::NamedTempFile;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ManagerConfig {
    pub remote_access: crate::remote::RemoteAccessConfig,
    pub agent_enabled: bool,
    pub broker: BrokerConfig,
    pub workspaces: Vec<Workspace>,
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
            workspaces: Vec::new(),
            serena_path: None,
            port: 9121,
            dashboard_enabled: true,
            open_dashboard_on_launch: false,
            auto_start_server: true,
            minimize_to_tray: true,
        }
    }
}

impl ManagerConfig {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(origin) = &self.remote_access.self_hosted.public_origin {
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
            return Err("MCP 端口必须在 1024–65535 之间。".into());
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
    let config: ManagerConfig = serde_json::from_str(&content)
        .map_err(|error| format!("配置文件格式无效 {}：{error}", path.display()))?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn old_serena_port_9120_loads_with_disabled_broker() {
        let mut config: ManagerConfig = serde_json::from_str(r#"{"port":9120}"#).unwrap();
        assert!(!config.broker.enabled);
        assert!(config.validate().is_ok());
        config.broker.enabled = true;
        assert!(config.validate().is_err());
    }

    #[test]
    fn default_config_is_valid() {
        assert!(ManagerConfig::default().validate().is_ok());
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct BrokerConfig {
    pub enabled: bool,
    pub port: u16,
    pub allow_lan: bool,
}
impl Default for BrokerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 9120,
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
        let context = serde_json::json!({"description":"Desktop Broker read-only backend", "prompt":"", "fixed_tools":["activate_project","get_current_config","read_file","list_dir","find_file","search_for_pattern","get_symbols_overview","find_symbol","find_referencing_symbols"], "single_project":false});
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
