use std::path::PathBuf;

use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, FilePath};

pub fn selected_directory_path(selection: Option<FilePath>) -> Result<Option<PathBuf>, String> {
    selection
        .map(|path| {
            path.into_path()
                .map_err(|error| format!("directory picker path conversion failed: {error}"))
        })
        .transpose()
}

#[tauri::command]
pub async fn workspace_pick_directory(app: AppHandle) -> Result<Option<PathBuf>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        selected_directory_path(app.dialog().file().blocking_pick_folder())
    })
    .await
    .map_err(|error| format!("directory picker task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    #[test]
    fn selected_directory_returns_that_single_path() {
        let selected = PathBuf::from("C:/workspace/picked-directory");

        assert_eq!(
            selected_directory_path(Some(selected.clone().into())),
            Ok(Some(selected))
        );
    }

    #[test]
    fn cancellation_is_a_successful_no_op() {
        assert_eq!(selected_directory_path(None), Ok(None));
    }

    #[test]
    fn unsupported_picker_path_is_an_error() {
        let unsupported = FilePath::Url(Url::parse("https://example.test/not-a-path").unwrap());

        assert!(selected_directory_path(Some(unsupported)).is_err());
    }
}
