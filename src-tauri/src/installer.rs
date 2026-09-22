use crate::{
    config::AppPaths,
    discovery::{self, InstallationSource, InstallationState, SERENA_PACKAGE, SERENA_PYTHON},
    logs, serena,
};
use std::{env, path::PathBuf, time::Duration};

#[cfg(target_os = "macos")]
const UV_MAX_DOWNLOAD: usize = 100 * 1024 * 1024;

/// macOS 首版冻结的官方 uv 发布资产。
#[cfg(target_os = "macos")]
struct MacosUvArtifact {
    version: &'static str,
    name: &'static str,
    sha256: &'static str,
}

pub fn install_serena(paths: &AppPaths) -> Result<(), String> {
    let app_log = &paths.app_log;
    logs::append(app_log, "installer", "installer started");
    let git = discovery::detect_git();
    if !git.available {
        return Err(git.error.unwrap_or_else(|| "Git 不可用。".into()));
    }
    logs::append(app_log, "installer", "Git detected");
    let uv = match find_uv(paths) {
        Some(path) => path,
        None => {
            logs::append(app_log, "installer", "未发现 uv，准备安装固定受管版本");
            install_uv(paths, app_log)?
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

/// Windows 保留既有 winget 安装路径，并在完成后重新发现 uv。
#[cfg(windows)]
fn install_uv(paths: &AppPaths, app_log: &std::path::Path) -> Result<PathBuf, String> {
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
    ensure_success("安装 uv", output, app_log)?;
    find_uv(paths).ok_or_else(|| {
        "winget 已完成，但仍未找到 uv。请重新登录 Windows 或按官方说明手工安装 uv。".to_string()
    })
}

/// Windows 按现有 PATH、用户目录与 WinGet Links 顺序发现 uv。
#[cfg(windows)]
fn find_uv(_paths: &AppPaths) -> Option<PathBuf> {
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

/// 其他 Unix 本阶段不新增自动下载能力，只复用已有显式安装。
#[cfg(all(not(windows), not(target_os = "macos")))]
fn install_uv(_paths: &AppPaths, _app_log: &std::path::Path) -> Result<PathBuf, String> {
    Err("未发现 uv。请按官方说明安装 uv 后重试。".into())
}

/// 其他 Unix 继续按 PATH 与用户目录发现 uv。
#[cfg(all(not(windows), not(target_os = "macos")))]
fn find_uv(_paths: &AppPaths) -> Option<PathBuf> {
    serena::find_executable("uv").or_else(|| serena::user_local_candidate("uv"))
}

/// 返回首版唯一允许的 macOS arm64 uv 资产。
#[cfg(target_os = "macos")]
fn macos_uv_artifact(os: &str, arch: &str) -> Result<MacosUvArtifact, String> {
    if (os, arch) != ("macos", "aarch64") {
        return Err("当前 macOS 架构不在首版 uv 支持范围内。".into());
    }
    Ok(MacosUvArtifact {
        version: "0.12.17",
        name: "uv-aarch64-apple-darwin.tar.gz",
        sha256: "85f00cbdc6dd3e97eba4c31b4d014375a9fdfe8f570023b84e5102fc3456896b",
    })
}

/// 返回应用 runtime 内固定版本 uv 的最终路径。
#[cfg(target_os = "macos")]
fn managed_uv_path(paths: &AppPaths) -> PathBuf {
    paths
        .runtime_directory
        .join("uv")
        .join("0.12.17")
        .join("uv")
}

/// 构造 Finder/LaunchAgent 环境下不依赖 shell profile 的固定候选顺序。
#[cfg(target_os = "macos")]
fn macos_uv_candidates(
    paths: &AppPaths,
    path: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> Vec<PathBuf> {
    let mut candidates = vec![managed_uv_path(paths)];
    candidates.extend(
        path.into_iter()
            .flat_map(env::split_paths)
            .map(|directory| directory.join("uv")),
    );
    if let Some(home) = home {
        candidates.push(PathBuf::from(home).join(".local/bin/uv"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/uv"));
    candidates.push(PathBuf::from("/usr/local/bin/uv"));
    candidates
}

/// 只接受 regular file 且带任一 execute bit 的 macOS uv 候选。
#[cfg(target_os = "macos")]
fn is_executable_file(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// 按固定候选发现 macOS uv，不读取 LOCALAPPDATA 或 shell dotfiles。
#[cfg(target_os = "macos")]
fn find_uv(paths: &AppPaths) -> Option<PathBuf> {
    macos_uv_candidates(
        paths,
        env::var_os("PATH").as_deref(),
        env::var_os("HOME").as_deref(),
    )
    .into_iter()
    .find(|candidate| is_executable_file(candidate))
}

/// 对完整 uv 发布归档计算小写 SHA-256。
#[cfg(target_os = "macos")]
fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 从已验证归档中只提取精确 regular-file uv 成员。
#[cfg(target_os = "macos")]
fn extract_macos_uv(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let gzip = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gzip);
    for entry in archive.entries().map_err(|_| "uv 归档无法读取。")? {
        let entry = entry.map_err(|_| "uv 归档成员无法读取。")?;
        let path = entry.path().map_err(|_| "uv 归档路径无效。")?;
        if path.as_ref() != std::path::Path::new("uv-aarch64-apple-darwin/uv") {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err("uv 归档中的目标成员不是 regular file。".into());
        }
        let mut executable = Vec::new();
        entry
            .take(UV_MAX_DOWNLOAD as u64 + 1)
            .read_to_end(&mut executable)
            .map_err(|_| "uv 可执行文件提取失败。")?;
        if executable.len() > UV_MAX_DOWNLOAD {
            return Err("uv 可执行文件超过大小限制。".into());
        }
        return Ok(executable);
    }
    Err("uv 归档缺少固定目标成员。".into())
}

/// 下载、校验并原子安装固定 macOS arm64 uv，然后验证其版本命令可执行。
#[cfg(target_os = "macos")]
fn install_uv(paths: &AppPaths, app_log: &std::path::Path) -> Result<PathBuf, String> {
    use std::{io::Read, io::Write, os::unix::fs::PermissionsExt};
    let artifact = macos_uv_artifact(std::env::consts::OS, std::env::consts::ARCH)?;
    let target = managed_uv_path(paths);
    let directory = target
        .parent()
        .ok_or_else(|| "受管 uv 安装路径无效。".to_string())?;
    std::fs::create_dir_all(directory).map_err(|_| "无法创建受管 uv 安装目录。")?;
    let url = format!(
        "https://releases.astral.sh/github/uv/releases/download/{}/{}",
        artifact.version, artifact.name
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|_| "无法初始化 uv 下载客户端。")?;
    let response = client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|_| "uv 固定发布资产下载失败。")?;
    if response
        .content_length()
        .is_some_and(|length| length > UV_MAX_DOWNLOAD as u64)
    {
        return Err("uv 发布资产超过大小限制。".into());
    }
    let mut archive = Vec::new();
    response
        .take(UV_MAX_DOWNLOAD as u64 + 1)
        .read_to_end(&mut archive)
        .map_err(|_| "uv 固定发布资产下载失败。")?;
    if archive.len() > UV_MAX_DOWNLOAD {
        return Err("uv 发布资产超过大小限制。".into());
    }
    if sha256(&archive) != artifact.sha256 {
        return Err("uv 固定发布资产 SHA-256 校验失败。".into());
    }
    let executable = extract_macos_uv(&archive)?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(directory).map_err(|_| "无法创建 uv 临时安装文件。")?;
    temporary
        .write_all(&executable)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|_| "无法写入 uv 临时安装文件。")?;
    temporary
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o755))
        .map_err(|_| "无法设置 uv 执行权限。")?;
    temporary
        .persist(&target)
        .map_err(|_| "无法原子安装 uv。")?;

    let mut version = serena::hidden_command(&target);
    version.arg("--version");
    let output = serena::run_with_timeout(version, Duration::from_secs(10), "验证 uv")?;
    ensure_success("验证 uv", output, app_log)?;
    logs::append(
        app_log,
        "installer",
        &format!("受管 uv {} 安装完成", artifact.version),
    );
    Ok(target)
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

    /// 构造只含指定成员的 gzip tar，验证提取路径与 regular-file 门。
    #[cfg(target_os = "macos")]
    fn uv_archive(path: &str, bytes: &[u8], regular: bool) -> Vec<u8> {
        let mut compressed = Vec::new();
        {
            let gzip =
                flate2::write::GzEncoder::new(&mut compressed, flate2::Compression::default());
            let mut archive = tar::Builder::new(gzip);
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            header.set_entry_type(if regular {
                tar::EntryType::Regular
            } else {
                tar::EntryType::Symlink
            });
            header.set_cksum();
            archive.append_data(&mut header, path, bytes).unwrap();
            archive.into_inner().unwrap().finish().unwrap();
        }
        compressed
    }

    /// 首版 macOS 只允许固定版本、固定 arm64 资产与发布摘要。
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uv_asset_is_exactly_pinned() {
        let artifact = macos_uv_artifact("macos", "aarch64").unwrap();
        assert_eq!(artifact.version, "0.12.17");
        assert_eq!(artifact.name, "uv-aarch64-apple-darwin.tar.gz");
        assert_eq!(
            artifact.sha256,
            "85f00cbdc6dd3e97eba4c31b4d014375a9fdfe8f570023b84e5102fc3456896b"
        );
        assert!(macos_uv_artifact("macos", "x86_64").is_err());
        assert!(macos_uv_artifact("linux", "aarch64").is_err());
    }

    /// 只提取官方归档中的精确 regular-file uv 成员，不接受同名旁路或 symlink。
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uv_archive_requires_exact_regular_member() {
        let valid = uv_archive("uv-aarch64-apple-darwin/uv", b"uv-binary", true);
        assert_eq!(extract_macos_uv(&valid).unwrap(), b"uv-binary");

        let wrong = uv_archive("other/uv", b"wrong", true);
        assert!(extract_macos_uv(&wrong).is_err());
        let symlink = uv_archive("uv-aarch64-apple-darwin/uv", b"target", false);
        assert!(extract_macos_uv(&symlink).is_err());
    }

    /// Finder 精简 PATH 下仍按托管路径、PATH、用户目录与固定 Homebrew 位置发现 uv。
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uv_candidates_do_not_depend_on_shell_profiles() {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file: directory.path().join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("app.log"),
            serena_log: directory.path().join("serena.log"),
        };
        let candidates = macos_uv_candidates(
            &paths,
            Some(std::ffi::OsStr::new("/usr/bin:/bin")),
            Some(directory.path().as_os_str()),
        );
        assert_eq!(candidates[0], managed_uv_path(&paths));
        assert!(candidates.contains(&PathBuf::from("/usr/bin/uv")));
        assert!(candidates.contains(&directory.path().join(".local/bin/uv")));
        assert!(candidates.contains(&PathBuf::from("/opt/homebrew/bin/uv")));
        assert!(candidates.contains(&PathBuf::from("/usr/local/bin/uv")));
        assert!(
            candidates
                .iter()
                .all(|path| !path.to_string_lossy().contains("LOCALAPPDATA"))
        );
    }

    /// 显式网络 Gate 下载官方固定资产，并验证安装后的真实 uv 版本。
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "downloads the pinned official uv archive; run explicitly for managed installer verification"]
    fn official_macos_uv_install_matches_pinned_version() {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file: directory.path().join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("app.log"),
            serena_log: directory.path().join("serena.log"),
        };
        let installed = install_uv(&paths, &paths.app_log).unwrap();
        let output = std::process::Command::new(&installed)
            .arg("--version")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).starts_with("uv 0.12.17"));
        assert!(is_executable_file(&installed));
    }
}
