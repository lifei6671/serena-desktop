#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
use tauri::AppHandle;
#[cfg(windows)]
use tauri_plugin_autostart::ManagerExt;
#[cfg(windows)]
use winreg::{
    RegKey,
    enums::{HKEY_CURRENT_USER, KEY_READ},
};

#[cfg(windows)]
const RUN_KEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run";
#[cfg(windows)]
const AUTOSTART_ARGUMENT: &str = "--autostart";

#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefreshDecision {
    Disabled,
    Current,
    Refresh,
}

/// 仅在用户已启用登录自启且 Run target 过期时，使用现有插件覆盖同一个条目。
#[cfg(windows)]
pub fn refresh_enabled_registration(app: &AppHandle) -> Result<(), String> {
    let manager = app.autolaunch();
    let enabled = manager
        .is_enabled()
        .map_err(|error| format!("无法读取 Windows 登录自启状态：{error}"))?;
    let executable = std::env::current_exe()
        .map_err(|error| format!("无法解析当前 Serena Desktop 可执行文件：{error}"))?;
    let expected = expected_command(&executable)?;
    let actual = read_run_value(&app.package_info().name)?;

    refresh_if_stale(enabled, actual.as_deref(), &expected, || {
        manager
            .enable()
            .map_err(|error| format!("无法刷新 Windows 登录自启路径：{error}"))
    })?;
    Ok(())
}

#[cfg(windows)]
fn refresh_if_stale(
    enabled: bool,
    actual: Option<&str>,
    expected: &str,
    refresh: impl FnOnce() -> Result<(), String>,
) -> Result<RefreshDecision, String> {
    let decision = refresh_decision(enabled, actual, expected);
    if decision == RefreshDecision::Refresh {
        refresh()?;
    }
    Ok(decision)
}

#[cfg(windows)]
fn refresh_decision(enabled: bool, actual: Option<&str>, expected: &str) -> RefreshDecision {
    if !enabled {
        RefreshDecision::Disabled
    } else if actual == Some(expected) {
        RefreshDecision::Current
    } else {
        RefreshDecision::Refresh
    }
}

#[cfg(windows)]
fn expected_command(executable: &Path) -> Result<String, String> {
    if !executable.is_absolute() {
        return Err("当前 Serena Desktop 可执行文件不是绝对路径。".into());
    }
    let executable = executable.display().to_string();
    if executable.contains('"') {
        return Err("当前 Serena Desktop 可执行文件路径包含 Windows Run 不支持的引号。".into());
    }
    Ok(format!("\"{executable}\" {AUTOSTART_ARGUMENT}"))
}

#[cfg(windows)]
fn read_run_value(app_name: &str) -> Result<Option<String>, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = hkcu
        .open_subkey_with_flags(RUN_KEY, KEY_READ)
        .map_err(|error| format!("无法读取 Windows Run 项：{error}"))?;
    match run.get_value(app_name) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("无法读取 Serena Desktop 登录自启路径：{error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    use winreg::{
        RegValue,
        enums::{KEY_SET_VALUE, RegType},
    };

    const INSTALLED: &str =
        "\"C:\\Users\\lifei\\AppData\\Local\\Serena Desktop\\serena-desktop.exe\" --autostart";

    #[cfg(windows)]
    const STARTUP_APPROVED_RUN_KEY: &str =
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";

    #[cfg(windows)]
    struct RegistrationRestore {
        app_name: String,
        run_value: Option<RegValue>,
        startup_approved_value: Option<RegValue>,
        restored: bool,
    }

    #[cfg(windows)]
    impl RegistrationRestore {
        /// 保存测试前 SerenaDesktop 自己的 Run 值，并在异常路径也恢复它。
        fn capture(app_name: String) -> Self {
            Self {
                run_value: read_raw_value(RUN_KEY, &app_name).unwrap(),
                startup_approved_value: read_raw_value(STARTUP_APPROVED_RUN_KEY, &app_name)
                    .unwrap(),
                app_name,
                restored: false,
            }
        }

        fn restore(&mut self) -> std::io::Result<()> {
            self.restore_with(restore_raw_value)
        }

        /// 仅在两个原始快照都成功写回后标记完成，失败时保留快照给 Drop 重试。
        fn restore_with(
            &mut self,
            mut restore_value: impl FnMut(&str, &str, Option<&RegValue>) -> std::io::Result<()>,
        ) -> std::io::Result<()> {
            if self.restored {
                return Ok(());
            }
            restore_value(RUN_KEY, &self.app_name, self.run_value.as_ref())?;
            restore_value(
                STARTUP_APPROVED_RUN_KEY,
                &self.app_name,
                self.startup_approved_value.as_ref(),
            )?;
            self.restored = true;
            Ok(())
        }
    }

    #[cfg(windows)]
    impl Drop for RegistrationRestore {
        fn drop(&mut self) {
            let _ = self.restore();
        }
    }

    #[cfg(windows)]
    fn read_raw_value(key_path: &str, app_name: &str) -> std::io::Result<Option<RegValue>> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hkcu.open_subkey_with_flags(key_path, KEY_READ) {
            Ok(key) => key,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        match key.get_raw_value(app_name) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    #[cfg(windows)]
    fn restore_raw_value(
        key_path: &str,
        app_name: &str,
        value: Option<&RegValue>,
    ) -> std::io::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hkcu.open_subkey_with_flags(key_path, KEY_SET_VALUE) {
            Ok(key) => key,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && value.is_none() => {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        match value {
            Some(value) => key.set_raw_value(app_name, value),
            None => match key.delete_value(app_name) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            },
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "mutates only SerenaDesktop current-user autostart and restores its initial values"]
    fn windows_plugin_enable_disable_updates_real_registration_and_restores_state() {
        use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

        let mut context = tauri::generate_context!();
        context.config_mut().app.windows.clear();
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_autostart::init(
                MacosLauncher::LaunchAgent,
                Some(vec![AUTOSTART_ARGUMENT]),
            ))
            .build(context)
            .expect("测试 Tauri App 必须能初始化 autostart 插件");
        let app_name = app.package_info().name.clone();
        let mut restore = RegistrationRestore::capture(app_name.clone());
        let manager = app.autolaunch();

        manager.enable().expect("真实插件 enable 必须成功");
        assert!(manager.is_enabled().expect("真实插件 is_enabled 必须成功"));
        let raw = read_run_value(&app_name)
            .expect("真实 Run 值必须可读取")
            .expect("enable 后必须存在 SerenaDesktop Run 值");
        let expected = expected_command(&std::env::current_exe().unwrap()).unwrap();
        assert_eq!(raw, expected);
        println!("P6-004 real plugin enable: app_name={app_name}; raw_target={raw}; enabled=true");

        manager.disable().expect("真实插件 disable 必须成功");
        assert!(
            !manager
                .is_enabled()
                .expect("disable 后真实插件 is_enabled 必须成功")
        );
        assert_eq!(
            read_run_value(&app_name).expect("disable 后 Run 值必须可读取"),
            None
        );
        println!(
            "P6-004 real plugin disable: app_name={app_name}; registration=absent; enabled=false"
        );

        restore.restore().expect("测试结束必须恢复原始登录自启状态");
    }

    #[cfg(windows)]
    #[test]
    fn restore_retry_keeps_original_snapshots_after_partial_failure() {
        let original = RegValue {
            bytes: vec![1, 2, 3],
            vtype: RegType::REG_BINARY,
        };
        let mut restore = RegistrationRestore {
            app_name: "Serena Desktop".into(),
            run_value: Some(original),
            startup_approved_value: None,
            restored: false,
        };
        let mut first_attempts = 0;

        let error = restore.restore_with(|_, _, _| {
            first_attempts += 1;
            Err(std::io::Error::other("模拟首次恢复失败"))
        });

        assert!(error.is_err());
        assert_eq!(first_attempts, 1);
        assert!(!restore.restored);

        let mut retry = Vec::new();
        restore
            .restore_with(|key, _, value| {
                retry.push((key.to_owned(), value.map(|raw| raw.bytes.clone())));
                Ok(())
            })
            .expect("重试必须仍能取得完整原始快照");

        assert_eq!(
            retry,
            vec![
                (RUN_KEY.to_owned(), Some(vec![1, 2, 3])),
                (STARTUP_APPROVED_RUN_KEY.to_owned(), None),
            ]
        );
        assert!(restore.restored);
    }

    #[test]
    fn disabled_entry_is_not_refreshed() {
        let mut calls = 0;
        let result = refresh_if_stale(
            false,
            Some("D:\\serena-desktop.exe --autostart"),
            INSTALLED,
            || {
                calls += 1;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(result, RefreshDecision::Disabled);
        assert_eq!(calls, 0);
    }

    #[test]
    fn current_enabled_entry_is_not_rewritten() {
        let mut calls = 0;
        let result = refresh_if_stale(true, Some(INSTALLED), INSTALLED, || {
            calls += 1;
            Ok(())
        })
        .unwrap();

        assert_eq!(result, RefreshDecision::Current);
        assert_eq!(calls, 0);
    }

    #[test]
    fn stale_portable_entry_is_replaced_once_even_if_old_file_is_missing() {
        let mut calls = 0;
        let result = refresh_if_stale(
            true,
            Some("D:\\serena-desktop.exe --autostart"),
            INSTALLED,
            || {
                calls += 1;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(result, RefreshDecision::Refresh);
        assert_eq!(calls, 1);
    }

    #[test]
    fn installed_command_quotes_space_containing_executable_path() {
        let command = expected_command(Path::new(
            "C:\\Users\\lifei\\AppData\\Local\\Serena Desktop\\serena-desktop.exe",
        ))
        .unwrap();

        assert_eq!(command, INSTALLED);
    }

    #[test]
    fn refresh_failure_is_returned_after_exactly_one_overwrite_attempt() {
        let mut calls = 0;
        let error = refresh_if_stale(
            true,
            Some("D:\\serena-desktop.exe --autostart"),
            INSTALLED,
            || {
                calls += 1;
                Err("registry denied".into())
            },
        )
        .unwrap_err();

        assert_eq!(calls, 1);
        assert_eq!(error, "registry denied");
    }
}
