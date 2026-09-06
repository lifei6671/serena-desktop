use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};
use tempfile::NamedTempFile;

#[derive(Debug, Clone)]
pub struct AppPaths {
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
            config_file: config_directory.join("config.json"),
            app_log: log_directory.join("app.log"),
            serena_log: log_directory.join("serena.log"),
            log_directory,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ManagerConfig {
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
        if self.port < 1024 {
            return Err("MCP 端口必须在 1024–65535 之间。".into());
        }
        if let Some(path) = &self.serena_path
            && !path.is_file()
        {
            return Err(format!("Serena 可执行文件不存在：{}", path.display()));
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

pub fn apply_dashboard_setting(enabled: bool) -> Result<(), String> {
    let config_file = serena_config_path()?;
    if !config_file.is_file() {
        return Err(format!(
            "Serena 全局配置尚未初始化：{}",
            config_file.display()
        ));
    }
    let existing = fs::read_to_string(&config_file)
        .map_err(|error| format!("无法读取 Serena 配置 {}：{error}", config_file.display()))?;
    let updated = patch_dashboard_line(&existing, enabled);
    atomic_write(&config_file, updated.as_bytes())
        .map_err(|error| format!("无法更新 Serena Dashboard 配置：{error}"))
}

pub fn serena_config_path() -> Result<PathBuf, String> {
    let serena_home = env::var_os("SERENA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".serena")))
        .ok_or_else(|| "无法确定 Serena 用户配置目录。".to_string())?;
    Ok(serena_home.join("serena_config.yml"))
}

fn patch_dashboard_line(existing: &str, enabled: bool) -> String {
    let newline = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let replacement = format!("web_dashboard: {}", if enabled { "true" } else { "false" });
    let mut found = false;
    let mut lines = Vec::new();
    for line in existing.lines() {
        if !line.chars().next().is_some_and(char::is_whitespace)
            && line.trim_start().starts_with("web_dashboard:")
        {
            if !found {
                lines.push(replacement.clone());
                found = true;
            }
        } else {
            lines.push(line.to_string());
        }
    }
    if !found {
        lines.push(replacement);
    }
    let mut result = lines.join(newline);
    result.push_str(newline);
    result
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
    fn default_config_is_valid() {
        assert!(ManagerConfig::default().validate().is_ok());
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
    fn dashboard_patch_preserves_other_lines_and_line_endings() {
        let input = "language_backend: LSP\r\nweb_dashboard: true # old\r\nlog_level: 20\r\n";
        let result = patch_dashboard_line(input, false);
        assert_eq!(
            result,
            "language_backend: LSP\r\nweb_dashboard: false\r\nlog_level: 20\r\n"
        );
    }

    #[test]
    fn dashboard_patch_adds_missing_top_level_key() {
        let result = patch_dashboard_line("log_level: 20\n", true);
        assert_eq!(result, "log_level: 20\nweb_dashboard: true\n");
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
