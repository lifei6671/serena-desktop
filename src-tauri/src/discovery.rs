use crate::{
    config::{AppPaths, ManagerConfig},
    serena::{CapturedOutput, find_executable, hidden_command, run_with_timeout},
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub const SERENA_PACKAGE: &str = "serena-agent==1.7.0";
pub const SERENA_CONTEXT: &str = "broker";
pub const SERENA_PYTHON: &str = "3.13";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstallationState {
    Missing,
    Standard,
    Invalid,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstallationSource {
    Managed,
    External,
    Path,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerenaInstallation {
    pub state: InstallationState,
    pub source: InstallationSource,
    pub path: PathBuf,
    pub version: String,
    pub context: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GitStatus {
    Available,
    Missing,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitInstallation {
    pub status: GitStatus,
    pub available: bool,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    pub error: Option<String>,
}

impl Default for GitInstallation {
    fn default() -> Self {
        Self {
            status: GitStatus::Missing,
            available: false,
            path: None,
            version: None,
            error: Some("Git 尚未检测。".into()),
        }
    }
}

pub fn detect(config: &ManagerConfig, paths: &AppPaths) -> SerenaInstallation {
    discover(
        config.serena_path.as_deref(),
        &paths.managed_serena(),
        || find_executable("serena"),
        inspect_candidate,
    )
}

fn discover(
    external: Option<&Path>,
    managed: &Path,
    find_path: impl FnOnce() -> Option<PathBuf>,
    mut inspect: impl FnMut(&Path, InstallationSource) -> SerenaInstallation,
) -> SerenaInstallation {
    if let Some(path) = external {
        return inspect(path, InstallationSource::External);
    }
    if managed.exists() {
        return inspect(managed, InstallationSource::Managed);
    }
    if let Some(path) = find_path() {
        return inspect(&path, InstallationSource::Path);
    }
    inspect(managed, InstallationSource::Managed)
}

pub fn inspect_candidate(path: &Path, source: InstallationSource) -> SerenaInstallation {
    probe(path, source, path.is_file(), run_with_timeout)
}

fn probe(
    path: &Path,
    source: InstallationSource,
    exists: bool,
    mut run: impl FnMut(Command, Duration, &str) -> Result<CapturedOutput, String>,
) -> SerenaInstallation {
    let mut result = SerenaInstallation {
        state: InstallationState::Invalid,
        source,
        path: path.to_path_buf(),
        version: String::new(),
        context: None,
        error: None,
    };
    if !exists {
        result.state = if source == InstallationSource::Managed {
            InstallationState::Missing
        } else {
            InstallationState::Invalid
        };
        result.error = Some("Serena 可执行文件不存在。".into());
        return result;
    }
    let mut version = hidden_command(path);
    version.arg("--version");
    match run(version, Duration::from_secs(10), "读取 Serena 版本")
        .and_then(|out| successful_text(out, "Serena --version"))
    {
        Ok(text) => result.version = text.lines().next().unwrap_or_default().trim().into(),
        Err(error) => {
            result.error = Some(error);
            return result;
        }
    }
    if supported_version(&result.version) {
        result.state = InstallationState::Standard;
        result.context = Some(SERENA_CONTEXT.into());
    } else {
        result.error = Some("需要官方 Serena >= 1.7.0；请安装受支持版本。".into());
    }

    result
}

pub fn supported_version(text: &str) -> bool {
    let Some(version) = text.split_whitespace().last() else {
        return false;
    };
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    let numbers: Option<Vec<u32>> = parts.iter().map(|part| part.parse().ok()).collect();
    numbers.is_some_and(|v| (v[0], v[1], v[2]) >= (1, 7, 0))
}

fn successful_text(output: CapturedOutput, action: &str) -> Result<String, String> {
    if !output.status.success() {
        return Err(format!("{action}失败（{}）。", output.status));
    }
    let text = String::from_utf8_lossy(if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    })
    .trim()
    .to_string();
    if text.is_empty() {
        return Err(format!("{action}未返回有效输出。"));
    }
    Ok(text)
}

pub fn detect_git() -> GitInstallation {
    probe_git(find_executable("git"), run_with_timeout)
}

fn probe_git(
    path: Option<PathBuf>,
    mut run: impl FnMut(Command, Duration, &str) -> Result<CapturedOutput, String>,
) -> GitInstallation {
    let Some(path) = path else {
        return GitInstallation { error: Some("Git 是项目校验与只读 Git 工具的必要依赖。 请从 https://git-scm.com/downloads 安装后重新检测。".into()), ..GitInstallation::default() };
    };
    let mut command = hidden_command(&path);
    command.arg("--version");
    match run(command, Duration::from_secs(5), "检测 Git")
        .and_then(|out| successful_text(out, "git --version"))
    {
        Ok(text) if text.starts_with("git version ") => GitInstallation {
            status: GitStatus::Available,
            available: true,
            path: Some(path),
            version: Some(text.trim_start_matches("git version ").into()),
            error: None,
        },
        result => GitInstallation {
            status: GitStatus::Error,
            available: false,
            path: Some(path),
            version: None,
            error: Some(
                result
                    .err()
                    .unwrap_or_else(|| "git --version 返回了无法识别的版本。".into()),
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(windows))]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;

    fn output(code: u32, text: &str) -> CapturedOutput {
        CapturedOutput {
            status: std::process::ExitStatus::from_raw(code as _),
            stdout: text.as_bytes().to_vec(),
            stderr: vec![],
        }
    }

    #[test]
    fn rejects_old_unknown_and_fork_versions() {
        for (text, expected) in [
            ("serena 1.7.0", InstallationState::Standard),
            ("serena 1.6.1", InstallationState::Invalid),
            ("serena 1.7.1-enhanced.1", InstallationState::Invalid),
            ("unknown", InstallationState::Invalid),
        ] {
            let result = probe(
                Path::new("serena.exe"),
                InstallationSource::Managed,
                true,
                |_, _, _| Ok(output(0, text)),
            );
            assert_eq!(result.state, expected);
        }
        assert_eq!(
            probe(
                Path::new("serena.exe"),
                InstallationSource::Managed,
                true,
                |_, _, _| Ok(output(1, "serena 1.7.0"))
            )
            .state,
            InstallationState::Invalid
        );
    }

    #[test]
    fn external_and_managed_standard_are_incompatible() {
        for source in [InstallationSource::External, InstallationSource::Managed] {
            let result = probe(Path::new("serena.exe"), source, true, |_, _, _| {
                Ok(output(0, "standard"))
            });
            assert_eq!(result.state, InstallationState::Invalid);
        }
    }

    #[test]
    fn missing_and_unexecutable_candidates_have_explicit_states() {
        for (source, state) in [
            (InstallationSource::Managed, InstallationState::Missing),
            (InstallationSource::External, InstallationState::Invalid),
        ] {
            assert_eq!(
                probe(Path::new("missing.exe"), source, false, |_, _, _| panic!(
                    "must not execute missing file"
                ))
                .state,
                state
            );
        }
        assert_eq!(
            probe(
                Path::new("broken.exe"),
                InstallationSource::Managed,
                true,
                |_, _, _| Err("cannot launch".into())
            )
            .state,
            InstallationState::Invalid
        );
    }

    #[test]
    fn discovery_preserves_explicit_path_and_managed_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let managed = dir.path().join("serena.exe");
        std::fs::write(&managed, "fixture").unwrap();
        let inspect = |path: &Path, source| {
            probe(path, source, true, |_, _, _| Ok(output(0, "serena 1.7.0")))
        };
        let result = discover(
            None,
            &managed,
            || panic!("PATH must not override managed"),
            inspect,
        );
        assert_eq!(result.path, managed);
        assert_eq!(result.state, InstallationState::Standard);
        let external = Path::new("external.exe");
        let result = discover(
            Some(external),
            &managed,
            || panic!("explicit path must not fall back"),
            |path, source| probe(path, source, false, |_, _, _| unreachable!()),
        );
        assert_eq!(result.path, external);
        assert_eq!(result.state, InstallationState::Invalid);
        let result = discover(Some(external), &managed, || unreachable!(), inspect);
        assert_eq!(result.state, InstallationState::Standard);
        assert_eq!(result.source, InstallationSource::External);
    }

    #[test]
    fn discovery_reports_path_standard_or_missing() {
        let dir = tempfile::tempdir().unwrap();
        let managed = dir.path().join("missing.exe");
        let result = discover(
            None,
            &managed,
            || Some("path-serena.exe".into()),
            |path, source| probe(path, source, true, |_, _, _| Ok(output(0, "serena 1.7.0"))),
        );
        assert_eq!(result.state, InstallationState::Standard);
        assert_eq!(result.source, InstallationSource::Path);
        assert_eq!(
            discover(None, &managed, || None, inspect_candidate).state,
            InstallationState::Missing
        );
    }

    #[test]
    fn git_available_missing_and_failure() {
        assert_eq!(
            probe_git(None, |_, _, _| panic!("missing Git must not execute")).status,
            GitStatus::Missing
        );
        for (code, text, status) in [
            (0, "git version 2.51.0.windows.1", GitStatus::Available),
            (1, "failure", GitStatus::Error),
            (0, "unexpected", GitStatus::Error),
        ] {
            let git = probe_git(Some("git.exe".into()), |command, _, _| {
                assert_eq!(command.get_args().collect::<Vec<_>>(), ["--version"]);
                Ok(output(code, text))
            });
            assert_eq!(git.status, status);
            assert_eq!(git.available, status == GitStatus::Available);
        }
        assert_eq!(
            probe_git(Some("git.exe".into()), |_, _, _| Err("timeout".into())).status,
            GitStatus::Error
        );
    }
}
