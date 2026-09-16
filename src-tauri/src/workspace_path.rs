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
    pub(crate) fn new(lease: &'a WorkspaceLease) -> Self {
        Self { lease }
    }

    pub(crate) fn resolve(&self, relative_path: &str) -> Result<PathBuf, String> {
        let relative = normalize_relative_path(relative_path)?;
        let root = fs::canonicalize(&self.lease.canonical_root)
            .map_err(|_| invalid_path("workspace root is unavailable"))?;
        if !root.is_dir() {
            return Err(invalid_path("workspace root is not a directory"));
        }

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

    let left = left.as_os_str().encode_wide().collect::<Vec<_>>();
    let right = right.as_os_str().encode_wide().collect::<Vec<_>>();
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
