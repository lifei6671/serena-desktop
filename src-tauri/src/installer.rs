use crate::{
    config::AppPaths,
    discovery::{self, InstallationSource, InstallationState, SERENA_PACKAGE, SERENA_PYTHON},
    logs, serena,
};
use std::{env, path::PathBuf, time::Duration};

pub fn install_serena(paths: &AppPaths) -> Result<(), String> {
    let app_log = &paths.app_log;
    logs::append(app_log, "installer", "installer started");
    let git = discovery::detect_git();
    if !git.available {
        return Err(git.error.unwrap_or_else(|| "Git 不可用。".into()));
    }
    logs::append(app_log, "installer", "Git detected");
    let uv = match find_uv() {
        Some(path) => path,
        None => {
            logs::append(app_log, "installer", "未发现 uv，准备通过 winget 安装");
            install_uv(app_log)?;
            find_uv().ok_or_else(|| {
                "winget 已完成，但仍未找到 uv。请重新登录 Windows 或按官方说明手工安装 uv。"
                    .to_string()
            })?
        }
    };

    logs::append(app_log, "installer", "uv detected");
    install_managed(&uv, paths, serena::run_with_timeout)?;
    let installation =
        discovery::inspect_candidate(&paths.managed_serena(), InstallationSource::Managed);
    if installation.state != InstallationState::Standard {
        return Err(format!(
            "安装后的 官方 Serena 验证失败：{}",
            installation.error.unwrap_or_default()
        ));
    }
    logs::append(app_log, "installer", "Serena capability verified");
    Ok(())
}

fn install_managed(
    uv: &std::path::Path,
    paths: &AppPaths,
    run: impl FnOnce(std::process::Command, Duration, &str) -> Result<serena::CapturedOutput, String>,
) -> Result<(), String> {
    let tool_dir = paths.runtime_directory.join("uv-tools");
    let bin_dir = paths.runtime_directory.join("bin");
    std::fs::create_dir_all(&tool_dir)
        .and_then(|_| std::fs::create_dir_all(&bin_dir))
        .map_err(|error| format!("无法创建 Managed runtime：{error}"))?;
    let mut command = serena::hidden_command(uv);
    command
        .args([
            "tool",
            "install",
            "-p",
            SERENA_PYTHON,
            "--force",
            SERENA_PACKAGE,
        ])
        .env("UV_TOOL_DIR", tool_dir)
        .env("UV_TOOL_BIN_DIR", bin_dir);
    logs::append(&paths.app_log, "installer", "Serena installation started");
    let output = run(command, Duration::from_secs(15 * 60), "安装 官方 Serena")
        .map_err(|_| "官方 Serena 安装进程失败或超时，请检查 uv 与网络后重试。".to_string())?;
    ensure_success("安装 官方 Serena", output, &paths.app_log)
}

fn install_uv(app_log: &std::path::Path) -> Result<(), String> {
    let winget = serena::find_executable("winget").ok_or_else(|| {
        "未发现 uv，也未发现 winget。请从 https://docs.astral.sh/uv/getting-started/installation/ 安装 uv。".to_string()
    })?;
    let mut command = serena::hidden_command(winget);
    command.args([
        "install",
        "--id",
        "astral-sh.uv",
        "-e",
        "--accept-package-agreements",
        "--accept-source-agreements",
        "--disable-interactivity",
    ]);
    let output = serena::run_with_timeout(command, Duration::from_secs(10 * 60), "安装 uv")?;
    ensure_success("安装 uv", output, app_log)
}

fn find_uv() -> Option<PathBuf> {
    serena::find_executable("uv")
        .or_else(|| serena::user_local_candidate("uv"))
        .or_else(|| {
            env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .map(|root| {
                    root.join("Microsoft")
                        .join("WinGet")
                        .join("Links")
                        .join("uv.exe")
                })
                .filter(|path| path.is_file())
        })
}

fn ensure_success(
    action: &str,
    output: serena::CapturedOutput,
    app_log: &std::path::Path,
) -> Result<(), String> {
    if output.status.success() {
        logs::append(app_log, "installer", &format!("{action}完成"));
        return Ok(());
    }

    // Installer output may contain credential-bearing Git URLs. Keep only the exit status.
    let message = format!(
        "{action}失败（{}）。请检查 Git、uv、网络及固定发行标签后重试。",
        output.status
    );
    logs::append(app_log, "installer", &message);
    Err(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolated_install_passes_fixed_argv_and_environment_to_runner() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            runtime_directory: dir.path().join("runtime"),
            config_file: dir.path().join("config.json"),
            log_directory: dir.path().join("logs"),
            app_log: dir.path().join("app.log"),
            serena_log: dir.path().join("serena.log"),
        };
        let uv = dir.path().join("uv.exe");
        let error = install_managed(&uv, &paths, |command, timeout, _| {
            assert_eq!(command.get_program(), uv.as_os_str());
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                [
                    "tool",
                    "install",
                    "-p",
                    "3.13",
                    "--force",
                    "serena-agent==1.7.0"
                ]
            );
            let env: std::collections::HashMap<_, _> = command.get_envs().collect();
            assert_eq!(env.len(), 2);
            assert_eq!(
                env[std::ffi::OsStr::new("UV_TOOL_DIR")],
                Some(paths.runtime_directory.join("uv-tools").as_os_str())
            );
            assert_eq!(
                env[std::ffi::OsStr::new("UV_TOOL_BIN_DIR")],
                Some(paths.runtime_directory.join("bin").as_os_str())
            );
            assert_eq!(timeout, Duration::from_secs(900));
            Err("https://user:secret@example.com/token".into())
        })
        .unwrap_err();
        assert!(!error.contains("secret"));
    }
}
