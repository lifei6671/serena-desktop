//! 产品版本只作展示；复用 ACP Runtime 的进程组 owner 与有界 cleanup，不参与准入。
use super::{Owner, retain_created};
use crate::agent::{
    codebuddy::discovery::ResolvedLaunchSpec,
    codex::{macos_launcher::MacosLaunchRequest, macos_runtime::MacosRuntime},
};
use std::{
    fs::File,
    io::{self, Read},
    os::fd::AsRawFd,
    sync::Mutex,
    time::{Duration, Instant},
};

/// 产品 CLI 输出上限；stdout/stderr 都独立受限，不能阻塞或无限占用内存。
const MAX_OUTPUT: usize = 16 * 1024;

/// 在独立后台线程执行一次原生 --version；任何失败只降级为 None。
pub(crate) fn probe_product_version(
    resolved: &ResolvedLaunchSpec,
    timeout: Duration,
) -> Option<String> {
    let cwd = tempfile::tempdir().ok()?;
    let path = std::env::join_paths(&resolved.path_projection).ok()?;
    let request = MacosLaunchRequest {
        executable: resolved.executable.clone(),
        args: vec!["--version".into()],
        current_dir: cwd.path().to_owned(),
        runtime_instance_id: "codebuddy-product-version".into(),
    };
    let core = match MacosRuntime::create_external(request, &path) {
        Ok(core) => core,
        Err(error) => {
            if let Some(child) = error.created {
                retain_created(cwd.path().to_owned(), *child);
            }
            return None;
        }
    };
    let owner = Owner {
        core: Mutex::new(Some(core)),
        workspace: cwd.path().to_owned(),
        durable: None,
        outcome: Mutex::new(None),
        cleanup_on_drop: false,
    };
    let output = read_version_output(&owner, timeout);
    // 无论成功、I/O 失败、超时或输出超限，先回收直接 child 并确认原进程组为空。
    owner.cleanup().ok()?;
    parse_product_version(&output.ok()?)
}

/// 将 pipe 改成非阻塞，后台线程可用同一个 deadline 同时约束读取和退出状态。
fn nonblocking(file: &File) -> io::Result<()> {
    // SAFETY: fd 来自当前持有的 File，fcntl 只修改有效描述符的状态位。
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// 每轮有界读取两条 pipe；持续输出也必须回到外层 deadline 检查。
fn drain(file: &mut File, output: &mut Vec<u8>) -> io::Result<bool> {
    let mut buffer = [0; 4096];
    match file.read(&mut buffer) {
        Ok(0) => Ok(true),
        Ok(size) => {
            if output.len() + size > MAX_OUTPUT {
                return Err(io::Error::other("version output exceeded bound"));
            }
            output.extend_from_slice(&buffer[..size]);
            Ok(false)
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(false),
        Err(error) => Err(error),
    }
}

/// 持有唯一 owner 读取真实 stdout 和成功退出码，不把管道关闭视为进程成功。
fn read_version_output(owner: &Owner, timeout: Duration) -> io::Result<Vec<u8>> {
    let mut slot = owner.core.lock().unwrap();
    let core = slot.as_mut().expect("version probe owns Runtime");
    let (stdin, mut stdout, mut stderr) = core.clone_stdio()?;
    drop(stdin);
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let deadline = Instant::now() + timeout;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let (mut out_done, mut err_done) = (false, false);
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "version probe timed out",
            ));
        }
        if !out_done {
            out_done = drain(&mut stdout, &mut out)?;
        }
        if !err_done {
            err_done = drain(&mut stderr, &mut err)?;
        }
        if out_done
            && err_done
            && let Some(status) = core.probe_exit_status()?
        {
            return if status.success() {
                Ok(out)
            } else {
                Err(io::Error::other("version probe failed"))
            };
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 只接受 CodeBuddy --version 的完整产品版本，不提取 build/package 或 ACP 版本。
fn parse_product_version(output: &[u8]) -> Option<String> {
    let version = std::str::from_utf8(output).ok()?.trim();
    let parts: Vec<_> = version.split('.').collect();
    (parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())))
    .then(|| version.to_owned())
}

#[cfg(test)]
#[path = "macos_version_tests.rs"]
mod tests;
