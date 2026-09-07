use crate::{
    config::{self, AppPaths, ManagerConfig},
    logs,
};
use serde::Serialize;
use std::{
    env,
    ffi::OsStr,
    io::{BufRead, BufReader, Read},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const DEFAULT_DASHBOARD_URL: &str = "http://127.0.0.1:24282/dashboard/index.html";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerenaInstallation {
    pub path: PathBuf,
    pub version: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServerStatus {
    Stopped,
    Starting,
    Running,
    Error,
}

struct ManagedProcess {
    child: Child,
    port: u16,
    dashboard_enabled: bool,
    installation: SerenaInstallation,
}

struct Runtime {
    config: ManagerConfig,
    installation: Option<SerenaInstallation>,
    process: Option<ManagedProcess>,
    status: ServerStatus,
    last_error: Option<String>,
}

pub struct SupervisorState {
    runtime: Mutex<Runtime>,
    operation: Mutex<()>,
    dashboard_url: Arc<Mutex<String>>,
    pub paths: AppPaths,
}

#[derive(Debug, Clone)]
pub struct SupervisorSnapshot {
    pub config: ManagerConfig,
    pub installation: Option<SerenaInstallation>,
    pub active_installation: Option<SerenaInstallation>,
    pub server_status: ServerStatus,
    pub managed_process_present: bool,
    pub active_port: u16,
    pub active_dashboard_enabled: bool,
    pub dashboard_url: String,
    pub last_error: Option<String>,
}

#[derive(Debug)]
pub struct CapturedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl SupervisorState {
    pub fn new(paths: AppPaths) -> Result<Self, String> {
        logs::ensure_directory(&paths.log_directory)?;
        let config = config::load(&paths.config_file)?;
        Ok(Self {
            runtime: Mutex::new(Runtime {
                config,
                installation: None,
                process: None,
                status: ServerStatus::Stopped,
                last_error: None,
            }),
            operation: Mutex::new(()),
            dashboard_url: Arc::new(Mutex::new(DEFAULT_DASHBOARD_URL.to_string())),
            paths,
        })
    }

    pub fn snapshot(&self) -> SupervisorSnapshot {
        self.refresh_process_status();
        let runtime = self.runtime.lock().expect("supervisor mutex poisoned");
        let active_port = runtime
            .process
            .as_ref()
            .map(|process| process.port)
            .unwrap_or(runtime.config.port);
        let managed_process_present = runtime.process.is_some();
        let active_dashboard_enabled = runtime
            .process
            .as_ref()
            .map(|process| process.dashboard_enabled)
            .unwrap_or(runtime.config.dashboard_enabled);
        let active_installation = runtime
            .process
            .as_ref()
            .map(|process| process.installation.clone())
            .or_else(|| runtime.installation.clone());
        SupervisorSnapshot {
            config: runtime.config.clone(),
            installation: runtime.installation.clone(),
            active_installation,
            server_status: runtime.status,
            managed_process_present,
            active_port,
            active_dashboard_enabled,
            dashboard_url: self
                .dashboard_url
                .lock()
                .expect("dashboard URL mutex poisoned")
                .clone(),
            last_error: runtime.last_error.clone(),
        }
    }

    pub fn detect_serena(&self) -> Option<SerenaInstallation> {
        let config = self
            .runtime
            .lock()
            .expect("supervisor mutex poisoned")
            .config
            .clone();
        let installation = detect(&config);
        self.commit_detection(&config, installation)
    }

    fn commit_detection(
        &self,
        detected_config: &ManagerConfig,
        installation: Option<SerenaInstallation>,
    ) -> Option<SerenaInstallation> {
        let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
        if runtime.config != *detected_config {
            return runtime.installation.clone();
        }
        runtime.installation = installation.clone();
        if installation.is_some()
            && runtime.status == ServerStatus::Error
            && runtime.process.is_none()
        {
            runtime.status = ServerStatus::Stopped;
            runtime.last_error = None;
        }
        installation
    }

    pub fn replace_config(&self, next: ManagerConfig) -> Result<(), String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        next.validate()?;
        let installation = match &next.serena_path {
            Some(path) => Some(inspect_candidate(path).ok_or_else(|| {
                format!(
                    "指定文件无法作为 Serena 执行 `--version`：{}",
                    path.display()
                )
            })?),
            None => detect(&next),
        };
        config::save(&self.paths.config_file, &next)?;
        let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
        runtime.config = next;
        runtime.installation = installation;
        Ok(())
    }

    pub fn start(&self) -> Result<(), String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        self.start_unlocked()
    }

    pub fn start_automatically_if(&self, allowed: impl FnOnce() -> bool) -> Result<(), String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        let auto_start_enabled = self
            .runtime
            .lock()
            .expect("supervisor mutex poisoned")
            .config
            .auto_start_server;
        if !auto_start_enabled || !allowed() {
            return Ok(());
        }
        self.start_unlocked()
    }

    fn start_unlocked(&self) -> Result<(), String> {
        self.refresh_process_status();
        let (config, installation) = {
            let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
            if runtime.process.is_some() {
                return Err("Serena 已经在运行。".into());
            }
            let installation = runtime
                .installation
                .clone()
                .ok_or_else(|| "未发现 Serena，请先安装或配置可执行文件路径。".to_string())?;
            runtime.status = ServerStatus::Starting;
            runtime.last_error = None;
            (runtime.config.clone(), installation)
        };

        if let Err(error) = ensure_port_available(config.port) {
            self.set_error(&error);
            return Err(error);
        }
        if let Err(error) = ensure_serena_initialized(&installation.path) {
            self.set_error(&error);
            return Err(error);
        }
        if let Err(error) = config::apply_dashboard_setting(config.dashboard_enabled) {
            self.set_error(&error);
            return Err(error);
        }

        let mut command = hidden_command(&installation.path);
        command
            .args([
                "start-mcp-server",
                "--transport",
                "streamable-http",
                "--host",
                "127.0.0.1",
                "--port",
                &config.port.to_string(),
                "--open-web-dashboard",
                if config.open_dashboard_on_launch {
                    "true"
                } else {
                    "false"
                },
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|error| {
            let message = format!("无法启动 Serena：{error}");
            self.set_error(&message);
            message
        })?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        spawn_log_reader(
            stdout,
            "stdout",
            self.paths.serena_log.clone(),
            self.dashboard_url.clone(),
        );
        spawn_log_reader(
            stderr,
            "stderr",
            self.paths.serena_log.clone(),
            self.dashboard_url.clone(),
        );

        {
            let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
            runtime.process = Some(ManagedProcess {
                child,
                port: config.port,
                dashboard_enabled: config.dashboard_enabled,
                installation: installation.clone(),
            });
        }
        logs::append(
            &self.paths.app_log,
            "app",
            &format!("Serena 启动中，端口 {}", config.port),
        );

        let deadline = Instant::now() + Duration::from_secs(12);
        while Instant::now() < deadline {
            thread::sleep(Duration::from_millis(150));
            self.refresh_process_status();
            let status = self
                .runtime
                .lock()
                .expect("supervisor mutex poisoned")
                .status;
            if status == ServerStatus::Error {
                return Err(self
                    .runtime
                    .lock()
                    .expect("supervisor mutex poisoned")
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "Serena 启动失败。".into()));
            }
            if can_connect(config.port) {
                thread::sleep(Duration::from_millis(150));
                self.refresh_process_status();
                let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
                if runtime.process.is_some() {
                    runtime.status = ServerStatus::Running;
                    logs::append(&self.paths.app_log, "app", "Serena MCP 端口已就绪");
                    return Ok(());
                }
            }
        }

        let mut message = format!("Serena 启动超时：12 秒内未监听 127.0.0.1:{}。", config.port);
        if let Err(stop_error) = self.stop_process(false) {
            message.push_str(&format!(" 同时无法终止进程：{stop_error}"));
        }
        self.set_error(&message);
        Err(message)
    }

    pub fn stop(&self) -> Result<(), String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        self.stop_process(true)
    }

    pub fn restart(&self) -> Result<(), String> {
        let _operation = self.operation.lock().expect("operation mutex poisoned");
        self.stop_process(true)?;
        self.start_unlocked()
    }

    fn stop_process(&self, user_requested: bool) -> Result<(), String> {
        let process = self
            .runtime
            .lock()
            .expect("supervisor mutex poisoned")
            .process
            .take();
        let Some(mut process) = process else {
            let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
            if user_requested {
                runtime.status = ServerStatus::Stopped;
                runtime.last_error = None;
            }
            return Ok(());
        };

        if let Err(kill_error) = terminate_managed_process(&mut process.child) {
            match process.child.try_wait() {
                Ok(Some(_)) => {}
                _ => {
                    let message = format!("无法停止 Serena：{kill_error}");
                    let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
                    runtime.process = Some(process);
                    runtime.status = ServerStatus::Error;
                    runtime.last_error = Some(message.clone());
                    logs::append(&self.paths.app_log, "app", &message);
                    return Err(message);
                }
            }
        }
        let _ = process.child.wait();
        let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
        runtime.status = if user_requested {
            ServerStatus::Stopped
        } else {
            ServerStatus::Error
        };
        if user_requested {
            runtime.last_error = None;
            logs::append(&self.paths.app_log, "app", "Serena 已停止");
        }
        Ok(())
    }

    fn refresh_process_status(&self) {
        let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
        let process_status = runtime
            .process
            .as_mut()
            .map(|process| process.child.try_wait());
        match process_status {
            Some(Ok(Some(status))) => {
                runtime.process = None;
                runtime.status = ServerStatus::Error;
                let message = exit_message(status);
                runtime.last_error = Some(message.clone());
                logs::append(&self.paths.app_log, "app", &message);
            }
            Some(Err(error)) => {
                runtime.status = ServerStatus::Error;
                let message = format!("无法读取 Serena 进程状态：{error}");
                runtime.last_error = Some(message.clone());
                logs::append(&self.paths.app_log, "app", &message);
            }
            _ => {}
        }
        if runtime.status == ServerStatus::Running
            && let Some(port) = runtime.process.as_ref().map(|process| process.port)
            && !can_connect(port)
        {
            runtime.status = ServerStatus::Error;
            let message = format!("Serena 进程仍存在，但 127.0.0.1:{port} 已无法连接。");
            runtime.last_error = Some(message.clone());
            logs::append(&self.paths.app_log, "app", &message);
        }
    }

    fn set_error(&self, message: &str) {
        let mut runtime = self.runtime.lock().expect("supervisor mutex poisoned");
        runtime.status = ServerStatus::Error;
        runtime.last_error = Some(message.to_string());
        logs::append(&self.paths.app_log, "app", message);
    }
}

#[cfg(windows)]
fn terminate_managed_process(child: &mut Child) -> Result<(), String> {
    let mut command = hidden_command("taskkill.exe");
    command.args(["/PID", &child.id().to_string(), "/T", "/F"]);
    let output = run_with_timeout(command, Duration::from_secs(10), "终止 Serena 进程树")?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        Err(format!("taskkill 失败（{}）：{detail}", output.status))
    }
}

#[cfg(not(windows))]
fn terminate_managed_process(child: &mut Child) -> Result<(), String> {
    child.kill().map_err(|error| error.to_string())
}

pub fn detect(config: &ManagerConfig) -> Option<SerenaInstallation> {
    let candidates = config
        .serena_path
        .clone()
        .into_iter()
        .chain(find_executable("serena"))
        .chain(user_local_candidate("serena"));
    for candidate in candidates {
        if let Some(installation) = inspect_candidate(&candidate) {
            return Some(installation);
        }
    }
    None
}

pub fn find_executable(name: &str) -> Option<PathBuf> {
    let executable_name = if cfg!(windows) && !name.ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    env::var_os("PATH")
        .into_iter()
        .flat_map(|value| env::split_paths(&value).collect::<Vec<_>>())
        .map(|directory| directory.join(&executable_name))
        .find(|path| path.is_file())
}

pub fn user_local_candidate(name: &str) -> Option<PathBuf> {
    let filename = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .map(|home| home.join(".local").join("bin").join(filename))
        .filter(|path| path.is_file())
}

pub fn hidden_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

fn read_version(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    let mut command = hidden_command(path);
    command.arg("--version");
    let output = run_with_timeout(command, Duration::from_secs(5), "读取 Serena 版本").ok()?;
    if !output.status.success() {
        return None;
    }
    let bytes = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    let line = String::from_utf8_lossy(bytes)
        .lines()
        .next()?
        .trim()
        .to_string();
    (!line.is_empty()).then_some(line)
}

fn inspect_candidate(path: &Path) -> Option<SerenaInstallation> {
    read_version(path).map(|version| SerenaInstallation {
        path: path.to_path_buf(),
        version,
    })
}

fn ensure_serena_initialized(serena_path: &Path) -> Result<(), String> {
    let config_file = config::serena_config_path()?;
    if config_file.is_file() {
        return Ok(());
    }

    let mut command = hidden_command(serena_path);
    command.arg("init");
    let output = run_with_timeout(command, Duration::from_secs(60), "初始化 Serena")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        return Err(format!("初始化 Serena 失败（{}）：{detail}", output.status));
    }
    if !config_file.is_file() {
        return Err(format!(
            "Serena init 已完成，但未生成全局配置：{}",
            config_file.display()
        ));
    }
    Ok(())
}

pub fn run_with_timeout(
    mut command: Command,
    timeout: Duration,
    action: &str,
) -> Result<CapturedOutput, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动{action}进程：{error}"))?;
    let stdout = child.stdout.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });
    let stderr = child.stderr.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(100)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                join_reader(stdout);
                join_reader(stderr);
                return Err(format!("{action}超时，已终止本次进程。"));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                join_reader(stdout);
                join_reader(stderr);
                return Err(format!("无法读取{action}进程状态：{error}"));
            }
        }
    };

    Ok(CapturedOutput {
        status,
        stdout: join_reader(stdout),
        stderr: join_reader(stderr),
    })
}

fn join_reader(reader: Option<thread::JoinHandle<Vec<u8>>>) -> Vec<u8> {
    reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
}

fn ensure_port_available(port: u16) -> Result<(), String> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .map(drop)
        .map_err(|error| format!("端口 {port} 已被占用或无法绑定：{error}"))
}

fn can_connect(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        Duration::from_millis(120),
    )
    .is_ok()
}

fn spawn_log_reader<R: Read + Send + 'static>(
    reader: Option<R>,
    source: &'static str,
    log_path: PathBuf,
    dashboard_url: Arc<Mutex<String>>,
) {
    let Some(reader) = reader else { return };
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            logs::append(&log_path, source, &line);
            if let Some(url) = extract_dashboard_url(&line) {
                *dashboard_url.lock().expect("dashboard URL mutex poisoned") = url;
            }
        }
    });
}

fn extract_dashboard_url(line: &str) -> Option<String> {
    line.split_whitespace()
        .map(|part| part.trim_matches(|character: char| "()[]{}<>,;\"'".contains(character)))
        .find(|part| {
            (part.starts_with("http://127.0.0.1:") || part.starts_with("http://localhost:"))
                && part.contains("/dashboard/index.html")
        })
        .map(ToOwned::to_owned)
}

fn exit_message(status: ExitStatus) -> String {
    status
        .code()
        .map(|code| format!("Serena 意外退出，退出码 {code}。"))
        .unwrap_or_else(|| "Serena 被外部终止。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_loopback_dashboard_url() {
        assert_eq!(
            extract_dashboard_url("INFO Dashboard: http://127.0.0.1:24283/dashboard/index.html"),
            Some("http://127.0.0.1:24283/dashboard/index.html".into())
        );
        assert_eq!(
            extract_dashboard_url("http://example.com/dashboard/index.html"),
            None
        );
    }

    #[test]
    fn default_detection_does_not_accept_missing_custom_path() {
        let config = ManagerConfig {
            serena_path: Some(PathBuf::from("Z:/definitely/missing/serena.exe")),
            ..ManagerConfig::default()
        };
        let result = detect(&config);
        assert!(result.is_none() || result.unwrap().path != config.serena_path.unwrap());
    }

    #[test]
    fn background_start_rechecks_current_conditions() {
        let directory = tempfile::tempdir().unwrap();
        let config_file = directory.path().join("config.json");
        let config = ManagerConfig {
            auto_start_server: false,
            ..ManagerConfig::default()
        };
        config::save(&config_file, &config).unwrap();
        let paths = AppPaths {
            config_file,
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs").join("app.log"),
            serena_log: directory.path().join("logs").join("serena.log"),
        };
        let supervisor = SupervisorState::new(paths).unwrap();

        assert!(supervisor.start_automatically_if(|| true).is_ok());
        assert_eq!(supervisor.snapshot().server_status, ServerStatus::Stopped);
    }

    #[test]
    fn stale_detection_does_not_overwrite_newer_config_state() {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            config_file: directory.path().join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs").join("app.log"),
            serena_log: directory.path().join("logs").join("serena.log"),
        };
        let supervisor = SupervisorState::new(paths).unwrap();
        let stale_config = ManagerConfig::default();
        let current_installation = SerenaInstallation {
            path: PathBuf::from("current-serena.exe"),
            version: "current".into(),
        };
        {
            let mut runtime = supervisor.runtime.lock().unwrap();
            runtime.config.port = 9122;
            runtime.installation = Some(current_installation.clone());
        }

        let result = supervisor.commit_detection(
            &stale_config,
            Some(SerenaInstallation {
                path: PathBuf::from("stale-serena.exe"),
                version: "stale".into(),
            }),
        );

        assert_eq!(result.unwrap().path, current_installation.path);
        assert_eq!(
            supervisor.snapshot().installation.unwrap().version,
            current_installation.version
        );
    }

    #[cfg(windows)]
    #[test]
    fn bounded_command_returns_output() {
        let mut command = hidden_command("cmd.exe");
        command.args(["/D", "/C", "echo Serena"]);
        let output = run_with_timeout(command, Duration::from_secs(3), "测试命令").unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Serena"));
    }

    #[cfg(windows)]
    #[test]
    fn bounded_command_terminates_after_timeout() {
        let mut command = hidden_command("powershell.exe");
        command.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 5"]);
        let error = run_with_timeout(command, Duration::from_millis(100), "测试命令").unwrap_err();
        assert!(error.contains("超时"));
    }
}
