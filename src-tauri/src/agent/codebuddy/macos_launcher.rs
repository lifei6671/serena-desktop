//! CodeBuddy 原生 macOS 启动请求；执行时复用 Codex 的 setsid containment。
use super::discovery::ResolvedLaunchSpec;
use crate::{
    agent::codex::macos_launcher::MacosLaunchRequest,
    config::{canonicalize_workspace_root, same_workspace_root_identity},
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// 兼容平台共用调用点；macOS 不存在 Windows UNC 投影。
pub(crate) enum UncCurrentDirectoryPolicy {
    Unsupported,
}

/// frozen Workspace 的外部路径，ACP 与进程 cwd 共用。
pub(crate) struct ExternalProcessPath(PathBuf);
impl ExternalProcessPath {
    /// 返回已验证的绝对目录。
    pub(crate) fn as_path(&self) -> &Path {
        &self.0
    }
}

/// 固定 argv 和私有 PATH 投影，不经过 shell。
pub(crate) struct LaunchRequest {
    pub(super) inner: MacosLaunchRequest,
    pub(super) path: OsString,
    cwd: ExternalProcessPath,
}
impl LaunchRequest {
    /// 验证 frozen Workspace 与 ACP 参数后建立平台请求。
    pub(crate) fn from_resolved(
        resolved: &ResolvedLaunchSpec,
        root: &Path,
        _unc: UncCurrentDirectoryPolicy,
        id: String,
    ) -> Result<Self, &'static str> {
        let cwd = canonicalize_workspace_root(root)
            .map_err(|_| "CODEBUDDY_EXTERNAL_WORKSPACE_PATH_INVALID")?;
        if !same_workspace_root_identity(root, &cwd) || resolved.args != [OsString::from("--acp")] {
            return Err("CODEBUDDY_LAUNCH_INPUT_INVALID");
        }
        let path = std::env::join_paths(&resolved.path_projection)
            .map_err(|_| "CODEBUDDY_LAUNCH_INPUT_INVALID")?;
        let inner = MacosLaunchRequest {
            executable: resolved.executable.clone(),
            args: resolved.args.clone(),
            current_dir: cwd.clone(),
            runtime_instance_id: id,
        };
        crate::agent::codex::macos_launcher::validate(&inner)
            .map_err(|_| "CODEBUDDY_LAUNCH_INPUT_INVALID")?;
        Ok(Self {
            inner,
            path,
            cwd: ExternalProcessPath(cwd),
        })
    }
    /// 供 ACP session/new 使用同一个路径对象。
    pub(crate) fn projected_cwd(&self) -> &ExternalProcessPath {
        &self.cwd
    }
    /// 当前 containment 与 durable 行共用唯一 Runtime ID。
    pub(crate) fn runtime_instance_id(&self) -> &str {
        &self.inner.runtime_instance_id
    }
}
