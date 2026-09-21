use crate::config::{WORKSPACE_ROOT_NOT_DIRECTORY, WORKSPACE_ROOT_NOT_FOUND};
use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInspection {
    canonical_root: String,
    folder_basename: Option<String>,
}

pub fn inspect_workspace_directory(root: &Path) -> Result<WorkspaceInspection, String> {
    let metadata = fs::metadata(root).map_err(classify_root_io_error)?;
    if !metadata.is_dir() {
        return Err(WORKSPACE_ROOT_NOT_DIRECTORY.into());
    }

    let canonical_root = fs::canonicalize(root).map_err(classify_root_io_error)?;
    let folder_basename = canonical_root
        .file_name()
        .map(|basename| basename.to_string_lossy().into_owned());

    Ok(WorkspaceInspection {
        canonical_root: canonical_root.to_string_lossy().into_owned(),
        folder_basename,
    })
}

fn classify_root_io_error(error: io::Error) -> String {
    if error.kind() == io::ErrorKind::NotFound {
        WORKSPACE_ROOT_NOT_FOUND.into()
    } else {
        format!("workspace inspection failed: {error}")
    }
}

#[tauri::command]
pub fn workspace_inspect_directory(root: PathBuf) -> Result<WorkspaceInspection, String> {
    inspect_workspace_directory(&root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_directory_returns_canonical_root_and_folder_basename() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("inspect-me");
        fs::create_dir(&root).unwrap();

        assert_eq!(
            inspect_workspace_directory(&root).unwrap(),
            WorkspaceInspection {
                canonical_root: fs::canonicalize(&root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                folder_basename: Some("inspect-me".into()),
            }
        );
    }

    #[test]
    fn regular_file_returns_stable_not_directory_error() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("not-a-directory");
        fs::write(&file, "file").unwrap();

        assert_eq!(
            inspect_workspace_directory(&file),
            Err(WORKSPACE_ROOT_NOT_DIRECTORY.into())
        );
    }

    #[test]
    fn missing_path_returns_stable_not_found_error() {
        let directory = tempfile::tempdir().unwrap();

        assert_eq!(
            inspect_workspace_directory(&directory.path().join("missing")),
            Err(WORKSPACE_ROOT_NOT_FOUND.into())
        );
    }

    #[test]
    fn permission_and_other_io_failures_do_not_invent_stable_workspace_codes() {
        let error = classify_root_io_error(io::Error::from(io::ErrorKind::PermissionDenied));

        assert!(error.contains("workspace inspection failed"));
        assert!(!error.starts_with("WORKSPACE_"));
    }

    #[test]
    fn ordinary_non_git_directory_succeeds_without_filesystem_side_effects() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("ordinary-directory");
        fs::create_dir(&root).unwrap();

        let inspection = inspect_workspace_directory(&root).unwrap();
        let canonical_root = fs::canonicalize(&root)
            .unwrap()
            .to_string_lossy()
            .into_owned();

        assert_eq!(inspection.canonical_root, canonical_root);
        assert!(!root.join(".git").exists());
        assert!(!root.join(".serena").exists());
        assert!(!root.join(".codegraph").exists());
        assert!(fs::read_dir(&root).unwrap().next().is_none());
    }

    #[test]
    fn command_serializes_camel_case_metadata_fields() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("serialized-directory");
        fs::create_dir(&root).unwrap();
        let canonical_root = fs::canonicalize(&root)
            .unwrap()
            .to_string_lossy()
            .into_owned();

        let value = serde_json::to_value(workspace_inspect_directory(root).unwrap()).unwrap();

        assert_eq!(
            value["canonicalRoot"].as_str(),
            Some(canonical_root.as_str())
        );
        assert_eq!(value["folderBasename"], "serialized-directory");
        assert!(value.get("canonical_root").is_none());
        assert!(value.get("folder_basename").is_none());
    }
}
