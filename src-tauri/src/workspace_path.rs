#![allow(
    dead_code,
    reason = "P2A2-002 establishes the lease-rooted path boundary before callers migrate."
)]

use crate::workspace_resolver::WorkspaceLease;
use std::{
    ffi::OsString,
    fs,
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};

pub(crate) struct WorkspacePathResolver<'a> {
    lease: &'a WorkspaceLease,
}

impl<'a> WorkspacePathResolver<'a> {
    /// 返回仅由已捕获 Lease 派生的 canonical Workspace root，供无路径参数的 Tool 内部使用。
    pub(crate) fn root(&self) -> Result<PathBuf, String> {
        let root = fs::canonicalize(&self.lease.canonical_root)
            .map_err(|_| invalid_path("workspace root is unavailable"))?;
        if !crate::config::same_workspace_root_identity(&root, &self.lease.canonical_root) {
            return Err(invalid_path("frozen workspace root identity changed"));
        }
        if !root.is_dir() {
            return Err(invalid_path("workspace root is not a directory"));
        }
        Ok(root)
    }

    pub(crate) fn new(lease: &'a WorkspaceLease) -> Self {
        Self { lease }
    }

    /// 将 ACP 绝对路径按平台组件身份映射回冻结根，再复用 canonical 父目录检查。
    pub(crate) fn resolve_absolute(&self, path: &Path) -> Result<PathBuf, String> {
        if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(invalid_path("absolute path required without traversal"));
        }
        // macOS 系统目录别名是平台路径身份的一部分；不接受任意用户 symlink 别名。
        #[cfg(target_os = "macos")]
        let normalized = {
            let mut normalized = path.to_path_buf();
            for alias in ["/var", "/tmp", "/etc"] {
                if let Ok(relative) = path.strip_prefix(alias) {
                    let canonical = fs::canonicalize(alias)
                        .map_err(|_| invalid_path("system alias unavailable"))?;
                    normalized = canonical.join(relative);
                    break;
                }
            }
            normalized
        };
        #[cfg(target_os = "macos")]
        let path = normalized.as_path();
        let root = &self.lease.canonical_root;
        let mut components = path.components();
        for expected in root.components() {
            if !components
                .next()
                .is_some_and(|actual| same_component(expected, actual))
            {
                return Err(invalid_path("path escapes the frozen workspace root"));
            }
        }
        let relative: PathBuf = components.collect();
        if relative.as_os_str().is_empty() {
            return self.root();
        }
        self.resolve(
            relative
                .to_str()
                .ok_or_else(|| invalid_path("path encoding"))?,
        )
    }

    pub(crate) fn resolve(&self, relative_path: &str) -> Result<PathBuf, String> {
        let relative = normalize_relative_path(relative_path)?;
        let root = self.root()?;

        let candidate = root.join(relative);
        let (existing, suffix) = nearest_existing_path(&candidate)?;
        if !is_within_root(&root, &existing) {
            return Err(invalid_path("path escapes the workspace root"));
        }

        Ok(suffix
            .into_iter()
            .fold(existing, |path, component| path.join(component)))
    }
}

fn normalize_relative_path(value: &str) -> Result<PathBuf, String> {
    if value.is_empty()
        || value.starts_with('\\')
        || value.contains(':')
        || Path::new(value).is_absolute()
    {
        return Err(invalid_path("only workspace-relative paths are accepted"));
    }
    #[cfg(not(windows))]
    if value.contains('\\') {
        return Err(invalid_path("only workspace-relative paths are accepted"));
    }

    let mut normalized = PathBuf::new();
    for component in Path::new(value).components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(invalid_path("path traversal is not accepted"));
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(invalid_path("workspace root is not a path target"));
    }
    Ok(normalized)
}

fn nearest_existing_path(path: &Path) -> Result<(PathBuf, Vec<OsString>), String> {
    let mut probe = path.to_path_buf();
    let mut suffix = Vec::new();
    loop {
        match fs::symlink_metadata(&probe) {
            Ok(_) => {
                let existing = fs::canonicalize(&probe)
                    .map_err(|_| invalid_path("path canonicalization failed"))?;
                if !suffix.is_empty() && !existing.is_dir() {
                    return Err(invalid_path("a path parent is not a directory"));
                }
                suffix.reverse();
                return Ok((existing, suffix));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let component = probe
                    .file_name()
                    .ok_or_else(|| invalid_path("path has no existing parent"))?;
                suffix.push(component.to_os_string());
                if !probe.pop() {
                    return Err(invalid_path("path has no existing parent"));
                }
            }
            Err(_) => return Err(invalid_path("path inspection failed")),
        }
    }
}

fn invalid_path(reason: &str) -> String {
    format!("INVALID_PATH: {reason}")
}

fn is_within_root(root: &Path, candidate: &Path) -> bool {
    let mut candidate_components = candidate.components();
    root.components().all(|root_component| {
        candidate_components
            .next()
            .is_some_and(|candidate_component| same_component(root_component, candidate_component))
    })
}

#[cfg(not(windows))]
fn same_component(left: Component<'_>, right: Component<'_>) -> bool {
    left == right
}

#[cfg(windows)]
fn same_component(left: Component<'_>, right: Component<'_>) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

    // canonicalize 的 verbatim prefix 与 ACP 的普通 drive/UNC 表示使用相同身份。
    let normalize = |component: Component<'_>| -> OsString {
        use std::path::Prefix;
        match component {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                    OsString::from(format!("{}:", char::from(drive)))
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                    let mut value = OsString::from("\\\\");
                    value.push(server);
                    value.push("\\");
                    value.push(share);
                    value
                }
                _ => component.as_os_str().to_os_string(),
            },
            _ => component.as_os_str().to_os_string(),
        }
    };
    let left = normalize(left).encode_wide().collect::<Vec<_>>();
    let right = normalize(right).encode_wide().collect::<Vec<_>>();
    let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return false;
    };
    unsafe {
        CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) == CSTR_EQUAL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lease(root: &Path) -> WorkspaceLease {
        WorkspaceLease {
            workspace_id: "workspace".into(),
            canonical_root: fs::canonicalize(root).unwrap(),
            generation: 7,
        }
    }

    /// 绝对路径仍受冻结根和不存在父目录边界约束，macOS 系统别名可映射。
    #[test]
    fn absolute_targets_preserve_frozen_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        fs::create_dir(&root).unwrap();
        let lease = lease(&root);
        let resolver = WorkspacePathResolver::new(&lease);
        assert_eq!(
            resolver.resolve_absolute(&root.join("new/file")).unwrap(),
            lease.canonical_root.join("new/file")
        );
        assert!(
            resolver
                .resolve_absolute(&directory.path().join("outside"))
                .is_err()
        );
        assert!(resolver.resolve_absolute(&root.join("../outside")).is_err());
        assert!(
            resolver
                .resolve_absolute(Path::new("C:\\outside\\file"))
                .is_err()
        );
        assert!(
            resolver
                .resolve_absolute(Path::new("\\\\server\\share\\file"))
                .is_err()
        );
        #[cfg(unix)]
        {
            let outside = directory.path().join("outside");
            fs::create_dir(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
            assert!(
                resolver
                    .resolve_absolute(&root.join("link/missing/file"))
                    .is_err()
            );
            std::os::unix::fs::symlink(&root, directory.path().join("outside-alias")).unwrap();
            assert!(
                resolver
                    .resolve_absolute(&directory.path().join("outside-alias/file"))
                    .is_err()
            );
        }
    }

    /// Windows drive/UNC/verbatim 与大小写身份使用平台 ordinal 比较。
    #[cfg(windows)]
    #[test]
    fn windows_prefix_and_case_identity() {
        for (left, right) in [
            (r"C:\Root\file", r"\\?\c:\root\file"),
            (r"\\Server\Share\Root", r"\\?\UNC\server\share\root"),
        ] {
            let left: Vec<_> = Path::new(left).components().collect();
            let right: Vec<_> = Path::new(right).components().collect();
            assert_eq!(left.len(), right.len());
            assert!(
                left.into_iter()
                    .zip(right)
                    .all(|(a, b)| same_component(a, b))
            );
        }
    }

    #[test]
    fn resolves_existing_and_nonexistent_relative_targets_from_the_lease_root() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        let nested = root.join("nested");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&nested).unwrap();
        let existing = nested.join("file.txt");
        fs::write(&existing, "contents").unwrap();
        let lease = lease(&root);
        let resolver = WorkspacePathResolver::new(&lease);

        assert_eq!(
            resolver.resolve("nested/./file.txt").unwrap(),
            fs::canonicalize(&existing).unwrap()
        );
        assert_eq!(
            resolver.resolve("nested/new/file.txt").unwrap(),
            fs::canonicalize(&nested).unwrap().join("new/file.txt")
        );
        assert!(!nested.join("new").exists());
    }

    #[test]
    fn rejects_absolute_root_and_parent_paths() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        fs::create_dir(&root).unwrap();
        let lease = lease(&root);
        let resolver = WorkspacePathResolver::new(&lease);

        for value in [
            "",
            ".",
            "/tmp/outside",
            "\\windows-root",
            "C:/windows-root",
            "C:\\windows-root",
            "\\\\server\\share",
            "..",
            "../outside",
            "..\\outside",
        ] {
            assert!(
                resolver
                    .resolve(value)
                    .unwrap_err()
                    .starts_with("INVALID_PATH"),
                "{value}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn rejects_junction_escapes_for_existing_and_nonexistent_targets() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        let outside = directory.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        let status = std::process::Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(root.join("link"))
            .arg(&outside)
            .output()
            .unwrap();
        assert!(status.status.success());
        let lease = lease(&root);
        let resolver = WorkspacePathResolver::new(&lease);

        for value in ["link", "link/future.txt"] {
            assert!(resolver.resolve_absolute(&root.join(value)).is_err());
            assert!(
                resolver
                    .resolve(value)
                    .unwrap_err()
                    .starts_with("INVALID_PATH"),
                "{value}"
            );
        }
    }
}
