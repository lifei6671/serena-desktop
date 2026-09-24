//! 从当前 macOS 账户的交互式登录 Shell 获取受限 PATH，供本地子进程共享。

use std::{
    ffi::{OsStr, OsString},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::OnceLock,
    thread,
    time::{Duration, Instant},
};

const BEGIN: &[u8] = b"__SERENA_DESKTOP_PATH_BEGIN__";
const END: &[u8] = b"__SERENA_DESKTOP_PATH_END__";
const DONE: &[u8] = b"__SERENA_DESKTOP_PATH_DONE__";
const SHELL_TIMEOUT: Duration = Duration::from_millis(2500);
static USER_PATH: OnceLock<Vec<PathBuf>> = OnceLock::new();

/// 从 passwd 读取当前账户登录 Shell 与 Home，不信任 GUI 的 SHELL/HOME。
pub(crate) fn account() -> Option<(PathBuf, PathBuf)> {
    // SAFETY: getuid 与 sysconf 无外部指针。
    let uid = unsafe { libc::getuid() };
    let suggested = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let mut buffer = vec![
        0_u8;
        if suggested > 0 {
            suggested as usize
        } else {
            16 * 1024
        }
    ];
    // SAFETY: passwd、buffer 独占可写，成功后指针仅在 buffer 存活时读取。
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            &mut entry,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() || entry.pw_shell.is_null() || entry.pw_dir.is_null() {
        return None;
    }
    // SAFETY: getpwuid_r 成功时两个字段均指向仍有效的 buffer 内 NUL 字符串。
    let shell = unsafe { std::ffi::CStr::from_ptr(entry.pw_shell) };
    let home = unsafe { std::ffi::CStr::from_ptr(entry.pw_dir) };
    Some((
        PathBuf::from(OsStr::from_bytes(shell.to_bytes())),
        PathBuf::from(OsStr::from_bytes(home.to_bytes())),
    ))
}

/// 返回全进程缓存的绝对路径目录，用户 Shell 顺序优先。
pub(crate) fn directories() -> &'static [PathBuf] {
    USER_PATH.get_or_init(|| {
        let account = account();
        let shell = account.as_ref().map(|(shell, _)| shell.as_path());
        let home = account.as_ref().map(|(_, home)| home.as_path());
        let shell_path = shell
            .zip(home)
            .and_then(|(shell, home)| read_shell_path(shell, home, SHELL_TIMEOUT));
        merge_paths(
            shell_path.as_deref(),
            std::env::var_os("PATH").as_deref(),
            home,
        )
    })
}

/// 将同一份缓存目录合成为 child PATH。
pub(crate) fn value() -> OsString {
    std::env::join_paths(directories()).expect("validated PATH entries")
}

/// 只提取最后一个完整 marker 对，不读取 profile 噪声或 stderr。
fn marker_path(output: &[u8]) -> Option<OsString> {
    let start = output
        .windows(BEGIN.len())
        .rposition(|part| part == BEGIN)?
        + BEGIN.len();
    let end = output[start..]
        .windows(END.len())
        .position(|part| part == END)?
        + start;
    let bytes = &output[start..end];
    (!bytes.is_empty() && !bytes.contains(&b'\n') && !bytes.contains(&b'\0'))
        .then(|| OsStr::from_bytes(bytes).to_os_string())
}

/// 读取临时 stdout 尾部；仅在 supervisor 完成标记出现时提取 Shell PATH。
fn completed_path(output: &mut std::fs::File) -> Option<Option<OsString>> {
    let size = output.seek(SeekFrom::End(0)).ok()?;
    output
        .seek(SeekFrom::Start(size.saturating_sub(64 * 1024)))
        .ok()?;
    let mut bytes = Vec::new();
    output.take(64 * 1024).read_to_end(&mut bytes).ok()?;
    let done = bytes.windows(DONE.len()).rposition(|part| part == DONE)?;
    let status = &bytes[done + DONE.len()..];
    let end = status.windows(3).position(|part| part == b"__\n")?;
    Some(
        (status[..end] == *b"0")
            .then(|| marker_path(&bytes[..done]))
            .flatten(),
    )
}

/// 有界收口刚创建的私有组；失败时仅补救直接 child 和已验证身份的组。
fn cleanup_probe(child: &mut Child, identity: &crate::macos_process::Identity) -> bool {
    if crate::macos_process::terminate_sync(
        child,
        identity,
        Duration::from_millis(100),
        Duration::from_millis(200),
    )
    .is_ok()
    {
        return true;
    }
    let _ = identity.signal(libc::SIGKILL);
    let deadline = Instant::now() + Duration::from_millis(200);
    loop {
        let reaped = matches!(child.try_wait(), Ok(Some(_)));
        let empty = matches!(
            crate::macos_process::group_is_empty(identity.pgid()),
            Ok(true)
        );
        if reaped && empty {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// 有界执行账户 Shell；私有 supervisor 在进程组完全收口前保留 leader 身份。
fn read_shell_path(shell: &Path, home: &Path, timeout: Duration) -> Option<OsString> {
    if !shell.is_absolute() || !shell.is_file() {
        return None;
    }
    let mut output = tempfile::tempfile().ok()?;
    let mut command = Command::new("/bin/sh");
    command
        .args([
            OsStr::new("-c"),
            OsStr::new("trap ':' TERM; IFS= read -r start || exit 1; \"$1\" -ilc \"$2\"; status=$?; printf '\\n__SERENA_DESKTOP_PATH_DONE__%s__\\n' \"$status\"; i=0; while [ \"$i\" -lt 5 ]; do /bin/sleep 1; i=$((i+1)); done"),
            OsStr::new("serena-path-probe"),
            shell.as_os_str(),
            OsStr::new("printf '\\n__SERENA_DESKTOP_PATH_BEGIN__%s__SERENA_DESKTOP_PATH_END__\\n' \"$PATH\""),
        ])
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::from(output.try_clone().ok()?))
        .stderr(Stdio::null());
    crate::macos_process::configure_std_command(&mut command);
    let mut child = command.spawn().ok()?;
    // supervisor 在收到启动字节前不会创建账户 Shell；身份失败时直接回收无后代 child。
    let identity = match crate::macos_process::Identity::capture(child.id()) {
        Ok(identity) => identity,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };
    let mut control = child.stdin.take().expect("piped supervisor stdin");
    if control.write_all(b"start\n").is_err() {
        let _ = cleanup_probe(&mut child, &identity);
        return None;
    }
    drop(control);
    let started = Instant::now();
    let mut path = None;
    loop {
        if let Some(completed) = completed_path(&mut output) {
            path = completed;
            break;
        }
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => break,
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(20)),
            Ok(None) => break,
        }
    }
    // 仅向刚捕获的私有 leader 所在组发信号；清空证据不足时放弃 Shell PATH。
    let cleaned = cleanup_probe(&mut child, &identity);
    cleaned.then_some(path).flatten()
}

/// 保留首现的绝对目录；用户 PATH 成功时先于 GUI PATH 和固定回退。
fn merge_paths(
    shell: Option<&OsStr>,
    process: Option<&OsStr>,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    for source in [shell, process].into_iter().flatten() {
        directories.extend(std::env::split_paths(source));
    }
    directories.extend([
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/opt/homebrew/sbin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    if let Some(home) = home {
        directories.extend([
            home.join(".local/bin"),
            home.join(".cargo/bin"),
            home.join("go/bin"),
            home.join(".npm-global/bin"),
            home.join(".volta/bin"),
            home.join(".bun/bin"),
            home.join(".asdf/shims"),
        ]);
    }
    let mut unique = Vec::new();
    for directory in directories {
        if directory.is_absolute()
            && !directory.as_os_str().as_bytes().contains(&b':')
            && !unique.contains(&directory)
        {
            unique.push(directory);
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    /// profile 输出噪声不影响 marker 内的 PATH。
    #[test]
    fn marker_ignores_startup_noise() {
        assert_eq!(
            marker_path(b"startup noise\n__SERENA_DESKTOP_PATH_BEGIN__/nvm/bin:/usr/bin__SERENA_DESKTOP_PATH_END__\n"),
            Some(OsString::from("/nvm/bin:/usr/bin"))
        );
        assert_eq!(marker_path(b"noise only"), None);
    }

    /// Shell 优先、去重、拒绝相对目录，并保留必需回退目录。
    #[test]
    fn merge_preserves_user_priority_and_rejects_relative_entries() {
        let paths = merge_paths(
            Some(OsStr::new("/nvm/bin:relative:/usr/bin:/nvm/bin")),
            Some(OsStr::new("/opt/homebrew/bin:/nvm/bin")),
            Some(Path::new("/Users/test")),
        );
        assert_eq!(paths[0], PathBuf::from("/nvm/bin"));
        assert_eq!(paths[1], PathBuf::from("/usr/bin"));
        assert_eq!(
            paths
                .iter()
                .filter(|path| *path == Path::new("/nvm/bin"))
                .count(),
            1
        );
        assert!(!paths.contains(&PathBuf::from("relative")));
        assert!(paths.contains(&PathBuf::from("/Users/test/.cargo/bin")));
    }

    /// Shell 失败和超时都回退到进程与常见开发目录。
    #[test]
    fn failed_or_timed_out_shell_uses_fallback() {
        assert_eq!(
            read_shell_path(
                Path::new("/missing/shell"),
                Path::new("/Users/test"),
                Duration::from_millis(1)
            ),
            None
        );
        let paths = merge_paths(
            None,
            Some(OsStr::new("/gui/bin")),
            Some(Path::new("/Users/test")),
        );
        assert_eq!(paths[0], PathBuf::from("/gui/bin"));
        assert!(paths.contains(&PathBuf::from("/opt/homebrew/bin")));
    }

    /// 会卡住的 profile 在时限内退出，随后仍能构造回退路径。
    #[test]
    fn hanging_shell_is_bounded() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let shell = directory.path().join("slow-shell");
        std::fs::write(&shell, "#!/bin/sh\nsleep 5\n").unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o700)).unwrap();
        let started = Instant::now();
        assert_eq!(
            read_shell_path(&shell, directory.path(), Duration::from_millis(100)),
            None
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!merge_paths(None, None, None).is_empty());
    }

    /// 探测 Shell 读取 passwd Home 覆盖值，而非父进程的 HOME。
    #[test]
    fn shell_probe_receives_account_home() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let shell = directory.path().join("home-shell");
        let home = directory.path().join("account-home");
        std::fs::create_dir(&home).unwrap();
        std::fs::write(
            &shell,
            "#!/bin/sh\nprintf '\\n__SERENA_DESKTOP_PATH_BEGIN__%s/probe-bin:/usr/bin__SERENA_DESKTOP_PATH_END__\\n' \"$HOME\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o700)).unwrap();
        let expected = OsString::from(format!("{}/probe-bin:/usr/bin", home.display()));
        assert_eq!(
            read_shell_path(&shell, &home, Duration::from_secs(2)),
            Some(expected)
        );
    }

    /// Shell 成功后遗留的后台 child 仍属于本次私有组，返回前必须清空。
    #[test]
    fn successful_shell_cleans_background_child() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let shell = directory.path().join("background-shell");
        let group_file = directory.path().join("group-id");
        std::fs::write(
            &shell,
            format!(
                "#!/bin/sh\n/bin/sleep 30 &\nprintf '%s' \"$PPID\" > '{}'\nprintf '\\n__SERENA_DESKTOP_PATH_BEGIN__/background/bin:/usr/bin__SERENA_DESKTOP_PATH_END__\\n'\n",
                group_file.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            read_shell_path(&shell, directory.path(), Duration::from_secs(2)),
            Some(OsString::from("/background/bin:/usr/bin"))
        );
        let pgid: libc::pid_t = std::fs::read_to_string(&group_file)
            .unwrap()
            .parse()
            .unwrap();
        assert!(crate::macos_process::group_is_empty(pgid).unwrap());
    }

    /// 仅在配置好 node 与 rust-analyzer 的真实 Mac 上运行等价 Slot 子进程探针。
    #[test]
    #[ignore = "requires local node and rust-analyzer installation"]
    fn live_slot_path_resolves_node_and_rust_analyzer() {
        let (shell, home) = account().unwrap();
        let shell_path =
            read_shell_path(&shell, &home, SHELL_TIMEOUT).expect("interactive login shell PATH");
        let finder_path = OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin");
        let simulated_gui_path = std::env::join_paths(merge_paths(
            Some(&shell_path),
            Some(finder_path),
            Some(&home),
        ))
        .unwrap();
        let output = Command::new("/bin/sh")
            .args(["-c", "command -v node && node --version && command -v rust-analyzer && rust-analyzer --version"])
            .env_clear()
            .env("PATH", simulated_gui_path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("/node\n"), "{stdout}");
        assert!(stdout.contains("/rust-analyzer\n"), "{stdout}");
    }
}
