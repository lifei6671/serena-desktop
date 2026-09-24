//! macOS CommandRun 的进程创建与 Process Group 所有权适配。

use crate::{command::CommandSpec, macos_process, macos_user_path};
use std::{
    collections::BTreeMap,
    ffi::{CString, OsString},
    fs::File,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::{ffi::OsStrExt, process::ExitStatusExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

pub(crate) const RUNTIME_PLATFORM: &str = "macos";
pub(crate) const CONTAINMENT_TYPE: &str = "process_group";
pub(crate) const CLEANUP_FAILURE_REASON: &str = "process_group_cleanup_failed";

/// 每个 CommandRun 唯一的 PATH authority，发现可执行文件与 child 环境共享此快照。
pub(crate) struct CommandPath {
    directories: Vec<PathBuf>,
    value: OsString,
}

/// 显式 PATH 完全覆盖共享用户 PATH；只接受有序的绝对目录项。
pub(crate) fn command_path(explicit: &BTreeMap<String, String>) -> Result<CommandPath, String> {
    if let Some(value) = explicit.get("PATH") {
        if value.is_empty() || value.contains(['\0', '\r', '\n']) {
            return Err("COMMAND_ENV_INVALID".into());
        }
        let directories = std::env::split_paths(value).collect::<Vec<_>>();
        if directories.is_empty() || directories.iter().any(|directory| !directory.is_absolute()) {
            return Err("COMMAND_ENV_INVALID".into());
        }
        return Ok(CommandPath {
            directories,
            value: value.into(),
        });
    }
    Ok(CommandPath {
        directories: macos_user_path::directories().to_vec(),
        value: macos_user_path::value(),
    })
}

/// CommandService 与 Windows launcher 共享的创建输入。
pub(crate) struct LaunchRequest {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: PathBuf,
    pub environment: Vec<(OsString, OsString)>,
    pub command_run_id: String,
}

/// 创建后保留 stdio、直接 child 和进程组控制权。
pub(crate) struct LaunchedProcess {
    pub stdin: File,
    pub stdout: File,
    pub stderr: File,
    pub pid: u32,
    pub control: Arc<ProcessControl>,
}

/// child 仅在短时状态查询或有限终止期间上锁，等待任务可与取消并行。
pub(crate) struct ProcessControl {
    child: Mutex<Child>,
    identity: macos_process::Identity,
}

/// 与 CommandService 的稳定失败码对齐。
#[derive(Debug)]
pub(crate) struct LaunchError {
    pub code: &'static str,
    message: String,
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

/// 将系统调用错误归入 Command Runtime 失败类别。
fn failure(code: &'static str, error: impl std::fmt::Display) -> LaunchError {
    LaunchError {
        code,
        message: error.to_string(),
    }
}

/// 只接受绝对、存在且当前用户可执行的普通文件。
pub(super) fn executable_file(path: &Path) -> bool {
    if !path.is_absolute() || !path.is_file() {
        return false;
    }
    let Ok(raw) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: raw 为以 NUL 结尾且生命周期覆盖调用的路径。
    unsafe { libc::access(raw.as_ptr(), libc::X_OK) == 0 }
}

pub(super) use macos_user_path::account;

/// 选用账户登录 Shell；不可执行时使用系统 /bin/sh。
pub(crate) fn login_shell() -> Result<PathBuf, String> {
    select_shell(account().map(|(shell, _)| shell))
}

/// 将 OS 账户候选与固定回退进行一次确定性选择。
fn select_shell(account_shell: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(shell) = account_shell
        && executable_file(&shell)
    {
        return Ok(shell);
    }
    let fallback = PathBuf::from("/bin/sh");
    executable_file(&fallback)
        .then_some(fallback)
        .ok_or_else(|| "COMMAND_SHELL_NOT_FOUND".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 不存在或非绝对的账户 Shell 必须回退到可执行的 /bin/sh。
    #[test]
    fn unavailable_account_shell_falls_back_to_sh() {
        assert_eq!(select_shell(None).unwrap(), PathBuf::from("/bin/sh"));
        assert_eq!(
            select_shell(Some(PathBuf::from("relative-shell"))).unwrap(),
            PathBuf::from("/bin/sh")
        );
    }

    /// Finder 风格路径不足时，固定系统、Homebrew 和账户开发目录仍在候选表。
    #[test]
    fn executable_search_includes_finder_fallback_directories() {
        let directories = macos_user_path::directories();
        assert!(directories.contains(&PathBuf::from("/usr/bin")));
        assert!(directories.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(directories.contains(&PathBuf::from("/usr/local/bin")));
        if let Some((_, home)) = account() {
            assert!(directories.contains(&home.join(".local/bin")));
            assert!(directories.contains(&home.join(".cargo/bin")));
        }
    }

    /// Process 可执行文件发现与 child 的 PATH 来自同一缓存结果。
    #[test]
    fn discovery_and_child_use_shared_path() {
        let command_path = command_path(&BTreeMap::new()).unwrap();
        let env = environment(&BTreeMap::new(), "workspace", &command_path);
        let path = env.iter().find(|(key, _)| key == "PATH").unwrap().1.clone();
        let child_directories = std::env::split_paths(&path).collect::<Vec<_>>();
        assert_eq!(child_directories, macos_user_path::directories());
        let shell = resolve_executable("sh", &command_path.directories).unwrap();
        assert!(
            child_directories
                .iter()
                .any(|directory| directory.join("sh").is_file())
        );
        assert!(shell.is_absolute());
    }

    /// 显式 PATH 保留顺序，且 Process 发现与 Shell child 均使用同一覆盖值。
    #[test]
    fn explicit_path_controls_discovery_and_child() {
        use std::os::unix::fs::PermissionsExt;

        let mut explicit = BTreeMap::new();
        explicit.insert("PATH".into(), "/custom/bin:/usr/bin".into());
        let accepted = command_path(&explicit).unwrap();
        assert_eq!(
            accepted.directories,
            [PathBuf::from("/custom/bin"), PathBuf::from("/usr/bin")]
        );
        assert_eq!(accepted.value, OsString::from("/custom/bin:/usr/bin"));

        let directory = tempfile::tempdir().unwrap();
        let allowed = directory.path().join("allowed");
        let excluded = directory.path().join("excluded");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&excluded).unwrap();
        let name = "serena-explicit-path-probe";
        std::fs::write(excluded.join(name), "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(excluded.join(name), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        explicit.insert("PATH".into(), format!("{}:/usr/bin", allowed.display()));
        let command_path = command_path(&explicit).unwrap();
        let process = CommandSpec::Process {
            executable: name.into(),
            args: vec![],
        };
        assert_eq!(
            invocation(&process, &command_path).unwrap_err(),
            "COMMAND_EXECUTABLE_NOT_FOUND"
        );
        std::fs::write(allowed.join(name), "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(allowed.join(name), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        assert_eq!(
            invocation(&process, &command_path).unwrap().0,
            allowed.join(name).canonicalize().unwrap()
        );

        let child_env = environment(&explicit, "workspace", &command_path);
        let path = child_env
            .iter()
            .find(|(key, _)| key == "PATH")
            .unwrap()
            .1
            .clone();
        assert_eq!(path, command_path.value);
        assert_eq!(
            std::env::split_paths(&path).collect::<Vec<_>>(),
            command_path.directories
        );
        let (shell, args) = invocation(
            &CommandSpec::Shell {
                command: "printf '%s' \"$PATH\"".into(),
            },
            &command_path,
        )
        .unwrap();
        let mut shell_child = Command::new(shell);
        shell_child.args(args).env_clear().envs(child_env);
        let child_path = shell_child
            .get_envs()
            .find(|(key, _)| *key == "PATH")
            .unwrap()
            .1
            .unwrap();
        assert_eq!(child_path, path);
    }

    /// 相对、空白项和 NUL 均不能进入显式 PATH authority。
    #[test]
    fn explicit_path_rejects_relative_and_invalid_entries() {
        for invalid in ["relative:/usr/bin", "/usr/bin:", "", "/usr/bin\0/else"] {
            let explicit = BTreeMap::from([("PATH".into(), invalid.into())]);
            assert_eq!(
                command_path(&explicit).err(),
                Some("COMMAND_ENV_INVALID".into())
            );
        }
    }
}

/// 按本次 CommandRun 的 PATH 顺序发现第一个当前用户可执行的真实文件。
fn resolve_executable(name: &str, directories: &[PathBuf]) -> Option<PathBuf> {
    for directory in directories {
        let candidate = directory.join(name);
        if executable_file(&candidate)
            && let Ok(path) = candidate.canonicalize()
        {
            return Some(path);
        }
    }
    None
}

/// Process 保持 argv 边界；Shell 以选定登录 Shell 的 -c 执行，不加载 profile。
pub(crate) fn invocation(
    spec: &CommandSpec,
    command_path: &CommandPath,
) -> Result<(PathBuf, Vec<OsString>), String> {
    match spec {
        CommandSpec::Process { executable, args } => Ok((
            resolve_executable(executable, &command_path.directories)
                .ok_or("COMMAND_EXECUTABLE_NOT_FOUND")?,
            args.iter().map(OsString::from).collect(),
        )),
        CommandSpec::Shell { command } => Ok((login_shell()?, vec!["-c".into(), command.into()])),
    }
}

/// 只继承明确列出的开发环境键，再覆盖调用方提供的非保留键。
pub(crate) fn environment(
    explicit: &BTreeMap<String, String>,
    workspace_id: &str,
    command_path: &CommandPath,
) -> Vec<(OsString, OsString)> {
    let mut values = BTreeMap::<OsString, OsString>::new();
    for key in [
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            values.insert(key.into(), value);
        }
    }
    // Finder 的 PATH 往往缺少开发工具目录；Shell 子命令与 Process 发现使用同一组候选。
    if let Some((_, home)) = account() {
        values.insert("HOME".into(), home.into_os_string());
    }
    for (key, value) in explicit {
        values.insert(key.into(), value.into());
    }
    values.insert("PATH".into(), command_path.value.clone());
    values.insert("SERENA_DESKTOP_COMMAND".into(), "1".into());
    values.insert("SERENA_DESKTOP_WORKSPACE_ID".into(), workspace_id.into());
    values.into_iter().collect()
}

/// exec 前形成私有 Session，父进程随后冻结 Darwin leader identity。
pub(crate) fn launch(request: &LaunchRequest) -> Result<LaunchedProcess, LaunchError> {
    if !executable_file(&request.executable)
        || !request.current_dir.is_absolute()
        || request.command_run_id.is_empty()
    {
        return Err(failure(
            "COMMAND_LAUNCH_INPUT_INVALID",
            "invalid launch input",
        ));
    }
    let mut command = Command::new(&request.executable);
    command
        .args(&request.args)
        .current_dir(&request.current_dir);
    command
        .env_clear()
        .envs(request.environment.iter().cloned());
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    macos_process::configure_std_command(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| failure("COMMAND_PROCESS_CREATE_FAILED", error))?;
    let identity = match macos_process::Identity::capture(child.id()) {
        Ok(identity) => identity,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(failure("COMMAND_PROCESS_IDENTITY_FAILED", error));
        }
    };
    let pid = child.id();
    let stdin: File = OwnedFd::from(child.stdin.take().expect("piped stdin")).into();
    let stdout: File = OwnedFd::from(child.stdout.take().expect("piped stdout")).into();
    let stderr: File = OwnedFd::from(child.stderr.take().expect("piped stderr")).into();
    for output in [&stdout, &stderr] {
        if let Err(error) = nonblocking(output) {
            let _ = macos_process::terminate_sync(
                &mut child,
                &identity,
                Duration::from_millis(500),
                Duration::from_secs(2),
            );
            return Err(failure("COMMAND_PIPE_CREATE_FAILED", error));
        }
    }
    Ok(LaunchedProcess {
        stdin,
        stdout,
        stderr,
        pid,
        control: Arc::new(ProcessControl {
            child: Mutex::new(child),
            identity,
        }),
    })
}

/// 读取端非阻塞，允许证据不足时停止 reader 而不等无界后代关闭管道。
fn nonblocking(file: &File) -> std::io::Result<()> {
    // SAFETY: fd 在本函数调用期间由 file 持有，fcntl 只调整其状态标志。
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

impl ProcessControl {
    /// 同一轮观测要求直接 child 已回收且受管 Process Group 已空。
    pub(crate) fn complete_evidence(&self) -> Result<bool, LaunchError> {
        let child_reaped = self
            .child
            .lock()
            .unwrap()
            .try_wait()
            .map_err(|error| failure("COMMAND_PROCESS_WAIT_FAILED", error))?
            .is_some();
        let group_empty = macos_process::group_is_empty(self.identity.pgid())
            .map_err(|error| failure("COMMAND_PROCESS_GROUP_QUERY_FAILED", error))?;
        Ok(child_reaped && group_empty)
    }

    /// 轮询并回收直接 child；短锁允许取消任务同时获得 child 所有权。
    pub(crate) fn wait_parent(&self) -> Result<i32, LaunchError> {
        loop {
            let status = self
                .child
                .lock()
                .unwrap()
                .try_wait()
                .map_err(|error| failure("COMMAND_PROCESS_WAIT_FAILED", error))?;
            if let Some(status) = status {
                return Ok(status.code().unwrap_or(128 + status.signal().unwrap_or(0)));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// SIGTERM、有限等待、必要时 SIGKILL；只在 child reaped 且 group empty 后成功。
    pub(crate) fn terminate(&self, timeout: Duration) -> Result<(), LaunchError> {
        let mut child = self.child.lock().unwrap();
        macos_process::terminate_sync(
            &mut child,
            &self.identity,
            timeout.min(Duration::from_millis(500)),
            timeout.saturating_sub(Duration::from_millis(500)),
        )
        .map_err(|error| failure("COMMAND_PROCESS_TERMINATE_FAILED", error))
    }
}
