# Phase 2A macOS Runtime Process Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Follow `.trellis/workflow.md`; do not activate Provider/Store integration in this task.

**Goal:** 在 Apple Silicon macOS 上实现独立、真实可测的 Codex launcher 与 live-host Runtime shutdown 契约，同时保持 Windows Runtime、StateStore、Workspace Claim 和 Startup Recovery 完全不变。

**Architecture:** `macos_launcher.rs` 独占输入校验、`setsid`、stdio、libproc identity 与 Process Group 查询；`macos_runtime.rs` 独占 live-host ownership、`SIGTERM → grace → SIGKILL` 和完整/unknown 终止结果。两个模块仅在 `target_os = "macos"` 编译，产品路径仍使用 Phase 1 unavailable backend，Phase 2B 再做持久化和恢复集成。

**Tech Stack:** Rust 2024、`std::process::Command`、`std::os::unix::process::CommandExt`、`libc 0.2.189`、Darwin libproc、Cargo unit tests。

---

## 文件职责

- Modify: `src-tauri/Cargo.toml` — 仅为 macOS 增加直接 `libc` 依赖。
- Modify: `src-tauri/Cargo.lock` — 只把已锁定的 `libc 0.2.189` 记录为根包的直接依赖，不升级版本。
- Modify: `src-tauri/src/agent/codex/mod.rs` — 仅在 macOS 声明两个新模块。
- Create: `src-tauri/src/agent/codex/macos_launcher.rs` — 输入、spawn、stdio、Session/Process Group、identity adapter、group query。
- Create: `src-tauri/src/agent/codex/macos_launcher/tests.rs` — launcher、stdio、真实 identity 与输入边界测试。
- Create: `src-tauri/src/agent/codex/macos_runtime.rs` — live-host ownership、有界 shutdown、完整 evidence 与 ownership-retaining failure。
- Create: `src-tauri/src/agent/codex/macos_runtime/tests.rs` — grace、kill escalation、unknown 和 evidence 测试。
- Create: `src-tauri/tests/fixtures/macos_runtime_child.rs` — 由测试直接用 `rustc` 编译的固定进程树 fixture；不进入产品 bundle。
- Modify: `.trellis/tasks/09-21-macos-p2a-runtime-contract/prd.md` — 完成后勾选已满足的验收项。

## 实施约束

- 不修改 `src-tauri/src/agent/codex/windows_launcher.rs` 或 `src-tauri/src/agent/codex/runtime.rs`。
- 不修改 `StateStore`、schema、Workspace Claim、Provider、Pool、Discovery 或 Startup Recovery。
- 不引入跨平台 Runtime trait、shell command string、fallback identity 或自动恢复。
- 所有新增函数注释、结构职责和关键分支使用中文注释。
- 业务 cancellation 保持现有 `Client::turn_interrupt()`；本计划不修改该调用链。
- 各 Task 末尾只做工作区检查，不创建中间提交；Phase 3.4 一次确认两个最终提交：源码实现提交与 Trellis 归档/session 提交。

## Task 1：建立 macOS 模块和输入边界

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Modify: `src-tauri/src/agent/codex/mod.rs`
- Create: `src-tauri/src/agent/codex/macos_launcher.rs`
- Create: `src-tauri/src/agent/codex/macos_launcher/tests.rs`

开始 RED 前只建立无行为 scaffold：在 `Cargo.toml` 增加 macOS `libc` 依赖，在 `codex/mod.rs` 增加 `macos_launcher` 声明，并创建内容仅为 `#[cfg(test)] mod tests;` 的 `macos_launcher.rs`。该 scaffold 不包含输入校验或 launcher 行为，使测试能够进入编译并因 API 缺失而失败。

- [x] **Step 1：先写输入边界失败测试**

在 `macos_launcher/tests.rs` 定义 request helper，并先覆盖空 ID、相对 executable/cwd、缺失 cwd、NUL 和超长 argv：

```rust
use super::*;
use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::{Path, PathBuf}};

fn request(executable: &Path, cwd: &Path) -> MacosLaunchRequest {
    MacosLaunchRequest {
        executable: executable.to_owned(),
        args: Vec::new(),
        current_dir: cwd.to_owned(),
        runtime_instance_id: "macos-runtime-fixture".into(),
    }
}

#[test]
fn invalid_launcher_inputs_fail_before_spawn() {
    let directory = tempfile::tempdir().unwrap();
    let executable = std::env::current_exe().unwrap();

    let mut empty_id = request(&executable, directory.path());
    empty_id.runtime_instance_id.clear();
    assert_eq!(validate(&empty_id).unwrap_err().code, "CODEX_LAUNCH_INPUT_INVALID");

    let relative_exe = request(Path::new("relative-bin"), directory.path());
    assert_eq!(validate(&relative_exe).unwrap_err().code, "CODEX_LAUNCH_INPUT_INVALID");

    let relative_cwd = request(&executable, Path::new("relative-cwd"));
    assert_eq!(validate(&relative_cwd).unwrap_err().code, "CODEX_LAUNCH_INPUT_INVALID");

    let missing = request(&executable, &directory.path().join("missing"));
    assert_eq!(validate(&missing).unwrap_err().code, "CODEX_LAUNCH_INPUT_INVALID");

    let mut nul = request(&executable, directory.path());
    nul.args.push(OsString::from_vec(b"bad\0arg".to_vec()));
    assert_eq!(validate(&nul).unwrap_err().code, "CODEX_LAUNCH_INPUT_INVALID");

    let nul_executable_path = PathBuf::from(OsString::from_vec(b"/tmp/bad\0bin".to_vec()));
    let nul_executable = request(&nul_executable_path, directory.path());
    assert_eq!(validate(&nul_executable).unwrap_err().code, "CODEX_LAUNCH_INPUT_INVALID");

    let nul_cwd_path = PathBuf::from(OsString::from_vec(b"/tmp/bad\0cwd".to_vec()));
    let nul_cwd = request(&executable, &nul_cwd_path);
    assert_eq!(validate(&nul_cwd).unwrap_err().code, "CODEX_LAUNCH_INPUT_INVALID");

    let mut oversized_id = request(&executable, directory.path());
    oversized_id.runtime_instance_id = "r".repeat(MAX_RUNTIME_ID_BYTES + 1);
    assert_eq!(validate(&oversized_id).unwrap_err().code, "CODEX_LAUNCH_INPUT_INVALID");

    let mut oversized = request(&executable, directory.path());
    oversized.args.push(OsString::from_vec(vec![b'x'; MAX_COMMAND_BYTES]));
    assert_eq!(validate(&oversized).unwrap_err().code, "CODEX_LAUNCH_INPUT_INVALID");
}
```

- [x] **Step 2：运行测试并确认红灯来自缺失模块/API**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked invalid_launcher_inputs_fail_before_spawn --no-run
```

Expected: FAIL；测试已进入模块树，但 `MacosLaunchRequest`、常量或 `validate` 尚不存在。

- [x] **Step 3：刷新直接 libc 依赖的 lockfile 记录**

scaffold 使用以下依赖与模块声明：

```toml
[target.'cfg(target_os = "macos")'.dependencies]
libc = "0.2.189"
```

在 `codex/mod.rs` 增加，且不得改变现有 Windows/unavailable 选择：

```rust
#[cfg(target_os = "macos")]
// Phase 2A 冻结私有进程契约，Phase 2B 才接入产品路径。
#[allow(dead_code)]
pub(crate) mod macos_launcher;
```

Run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
git diff -- src-tauri/Cargo.lock
```

Expected: `cargo check` PASS；`Cargo.lock` 只在根包依赖列表增加已锁定的 `libc 0.2.189`。

- [x] **Step 4：实现最小输入类型、错误和校验**

在 `macos_launcher.rs` 建立以下完整边界；`bytes()` 使用 `OsStrExt::as_bytes()`：

```rust
use std::{
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStrExt,
    path::PathBuf,
};

pub(super) const MAX_COMMAND_BYTES: usize = 128 * 1024;
pub(super) const MAX_RUNTIME_ID_BYTES: usize = 128;

#[derive(Debug)]
pub(crate) struct MacosLaunchRequest {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: PathBuf,
    pub runtime_instance_id: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MacosLaunchError {
    pub code: &'static str,
    pub message: String,
}

fn invalid(message: impl Into<String>) -> MacosLaunchError {
    MacosLaunchError {
        code: "CODEX_LAUNCH_INPUT_INVALID",
        message: message.into(),
    }
}

fn contains_nul(value: &OsStr) -> bool {
    value.as_bytes().contains(&0)
}

pub(super) fn validate(request: &MacosLaunchRequest) -> Result<(), MacosLaunchError> {
    if request.runtime_instance_id.is_empty()
        || request.runtime_instance_id.len() > MAX_RUNTIME_ID_BYTES
        || request.runtime_instance_id.contains(['/', '\\', '\0'])
        || !request.executable.is_absolute()
        || !request.current_dir.is_absolute()
        || !request.current_dir.is_dir()
        || contains_nul(request.executable.as_os_str())
        || contains_nul(request.current_dir.as_os_str())
    {
        return Err(invalid("Runtime ID、绝对 executable 和现有 cwd 必填"));
    }
    let mut bytes = request.executable.as_os_str().as_bytes().len() + 1;
    for argument in &request.args {
        if contains_nul(argument) {
            return Err(invalid("argv 不能包含 NUL"));
        }
        bytes = bytes
            .checked_add(argument.as_bytes().len() + 1)
            .ok_or_else(|| invalid("argv 长度溢出"))?;
    }
    if bytes > MAX_COMMAND_BYTES {
        return Err(invalid("executable 与 argv 超过 128 KiB"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
```

- [x] **Step 5：验证输入测试转绿并检查依赖锁文件**

用 locked 模式运行输入测试并复核 lockfile：

```bash
git diff -- src-tauri/Cargo.lock
cargo test --manifest-path src-tauri/Cargo.toml --locked invalid_launcher_inputs_fail_before_spawn
```

Expected: 测试 PASS；`Cargo.lock` 只在 `serena-desktop` 根包依赖列表增加既有 `libc 0.2.189`，没有 package 版本或 checksum 漂移。

## Task 2：实现真实 Session/Process Group、stdio 和 identity adapter

**Files:**

- Modify: `src-tauri/src/agent/codex/macos_launcher.rs`
- Modify: `src-tauri/src/agent/codex/macos_launcher/tests.rs`
- Create: `src-tauri/tests/fixtures/macos_runtime_child.rs`

- [x] **Step 1：创建固定 fixture 源码**

`macos_runtime_child.rs` 使用 Rust 标准库和直接 C FFI；支持 `report`、`tree`、`ignore-tree`、`leader-exit`、`leaf` 五种模式。核心结构必须如下，不引入 shell：

```rust
use std::{
    env, fs,
    io::Read,
    process::{exit, Command},
    thread,
    time::Duration,
};

unsafe extern "C" {
    fn getpid() -> i32;
    fn getpgrp() -> i32;
    fn getsid(pid: i32) -> i32;
    fn signal(signal: i32, handler: usize) -> usize;
}

const SIGTERM: i32 = 15;
const SIG_IGN: usize = 1;

fn ignore_term() {
    // SAFETY: 为测试 fixture 将 SIGTERM disposition 设置为 SIG_IGN。
    unsafe { signal(SIGTERM, SIG_IGN); }
}

fn wait_forever() -> ! {
    loop { thread::sleep(Duration::from_secs(60)); }
}

fn main() {
    let mut args = env::args().skip(1);
    let mode = args.next().unwrap_or_default();
    match mode.as_str() {
        "report" => {
            let argument = args.next().unwrap_or_default();
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).unwrap();
            // SAFETY: 无参数的进程身份查询只读取当前进程内核状态。
            let (pid, pgid, sid) = unsafe { (getpid(), getpgrp(), getsid(0)) };
            println!("pid={pid};pgid={pgid};sid={sid};cwd={};arg={};stdin={input}",
                env::current_dir().unwrap().display(), argument);
            eprintln!("stderr-ready");
        }
        "tree" | "ignore-tree" | "leader-term-exit" => {
            if mode == "ignore-tree" { ignore_term(); }
            let marker = args.next().expect("marker path");
            let leaf_mode = if mode == "tree" { "default" } else { "ignore" };
            let leaf = Command::new(env::current_exe().unwrap())
                .args(["leaf", leaf_mode, &marker]).spawn().unwrap();
            fs::write(&marker, leaf.id().to_string()).unwrap();
            wait_forever();
        }
        "leader-exit" => {
            let marker = args.next().expect("marker path");
            let leaf = Command::new(env::current_exe().unwrap())
                .args(["leaf", "ignore", &marker]).spawn().unwrap();
            fs::write(&marker, leaf.id().to_string()).unwrap();
            let mut release = [0_u8; 1];
            std::io::stdin().read_exact(&mut release).unwrap();
            exit(0);
        }
        "leaf" => {
            if args.next().as_deref() == Some("ignore") { ignore_term(); }
            let ready = args.next().expect("marker path");
            fs::write(format!("{ready}.ready"), b"ready").unwrap();
            wait_forever();
        }
        _ => exit(2),
    }
}
```

- [x] **Step 2：先写真实 launcher/identity 测试**

在测试模块增加以下 `fixture()`，直接调用固定 `rustc` executable，不经过 shell：

```rust
fn fixture(directory: &Path) -> PathBuf {
    let executable = directory.join("macos-runtime-child");
    let output = std::process::Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "macos_runtime_child"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/macos_runtime_child.rs"))
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    executable
}
```

新增测试：

```rust
#[test]
fn launch_preserves_argv_cwd_stdio_and_creates_verified_session() {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().join("工作 目录");
    std::fs::create_dir(&cwd).unwrap();
    let executable = fixture(directory.path());
    let mut request = request(&executable, &cwd);
    request.args = vec!["report".into(), "参数 值".into()];

    let launched = launch(&request).unwrap();
    assert_eq!(launched.identity.pid, launched.identity.pgid);
    assert_eq!(launched.identity.pid, launched.identity.sid);
    let repeated = MacosProcessIdentityAdapter::observe(launched.identity.pid).unwrap();
    assert_eq!(repeated, launched.identity);

    let mut child = launched.child;
    child.stdin.write_all("输入".as_bytes()).unwrap();
    drop(child.stdin);
    let mut stdout = String::new();
    let mut stderr = String::new();
    child.stdout.read_to_string(&mut stdout).unwrap();
    child.stderr.read_to_string(&mut stderr).unwrap();
    assert!(child.process.wait().unwrap().success());
    assert!(stdout.contains("arg=参数 值"));
    assert!(stdout.contains(&format!("cwd={}", cwd.display())));
    assert!(stdout.contains("stdin=输入"));
    assert_eq!(stderr.trim(), "stderr-ready");
}

#[test]
fn identity_mismatch_never_matches_created_process() {
    let actual = ProcessIdentity { pid: 10, pgid: 10, sid: 10,
        start_token: ProcessStartToken { seconds: 20, microseconds: 30 } };
    let mut reused = actual.clone();
    reused.start_token.microseconds += 1;
    assert!(!actual.matches(&reused));
    reused = actual.clone();
    reused.pgid += 1;
    assert!(!actual.matches(&reused));
    reused = actual.clone();
    reused.sid += 1;
    assert!(!actual.matches(&reused));
}
```

- [x] **Step 3：运行测试并确认因 launch/adapter 缺失而失败**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked launch_preserves_argv_cwd_stdio_and_creates_verified_session --no-run
```

Expected: FAIL；`launch`、`CreatedChild`、`ProcessIdentity` 或 adapter 尚未实现。

- [x] **Step 4：实现私有 identity adapter 和 group query**

在 `macos_launcher.rs` 增加下列类型与方法；不得把 `libc::proc_bsdinfo` 存入任何字段：

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProcessStartToken {
    pub(super) seconds: u64,
    pub(super) microseconds: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProcessIdentity {
    pub(super) pid: libc::pid_t,
    pub(super) pgid: libc::pid_t,
    pub(super) sid: libc::pid_t,
    pub(super) start_token: ProcessStartToken,
}

impl ProcessIdentity {
    pub(super) fn matches(&self, observed: &Self) -> bool { self == observed }
}

pub(super) struct MacosProcessIdentityAdapter;

impl MacosProcessIdentityAdapter {
    /// 从 Darwin 内核读取 leader 身份，不向上层暴露 libproc 结构。
    pub(super) fn observe(pid: libc::pid_t) -> std::io::Result<ProcessIdentity> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&info) as libc::c_int;
        // SAFETY: macOS 的 errno 是线程局部存储；先清零以区分 libproc 的空结果与失败。
        unsafe { *libc::__error() = 0; }
        let read = unsafe {
            libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0,
                (&mut info as *mut libc::proc_bsdinfo).cast(), size)
        };
        if read != size {
            let error = std::io::Error::last_os_error();
            return Err(if error.raw_os_error() == Some(0) {
                std::io::Error::from_raw_os_error(libc::ESRCH)
            } else { error });
        }
        let pgid = unsafe { libc::getpgid(pid) };
        let sid = unsafe { libc::getsid(pid) };
        if pgid < 0 || sid < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if info.pbi_pid != pid as u32 || info.pbi_pgid != pgid as u32 {
            return Err(std::io::Error::from_raw_os_error(libc::EPROTO));
        }
        Ok(ProcessIdentity {
            pid,
            pgid,
            sid,
            start_token: ProcessStartToken {
                seconds: info.pbi_start_tvsec,
                microseconds: info.pbi_start_tvusec,
            },
        })
    }
}
```

实现 `process_group_members(pgid)` 时，每次调用前通过 `*libc::__error() = 0` 清除线程局部 errno。`proc_listpgrppids` 返回 PID 数量；返回 `0` 且 errno 仍为 `0` 才表示空组，返回 `0` 且 errno 非零表示查询失败。先用 null buffer 获取数量，再分配 `count + 16` 个 `libc::pid_t`，最多重试 3 次处理成员增长；实际数量小于容量时 truncate，连续填满容量时返回 `EOVERFLOW`。空 `Vec` 是唯一 group-empty 观测。

- [x] **Step 5：实现 fixed executable launcher、stdio 和双侧 Session 校验**

实现类型：

```rust
pub(super) struct CreatedChild {
    pub(super) stdin: std::process::ChildStdin,
    pub(super) stdout: std::process::ChildStdout,
    pub(super) stderr: std::process::ChildStderr,
    pub(super) process: std::process::Child,
    pub(super) pid: libc::pid_t,
    pub(super) pgid: libc::pid_t,
}

pub(super) struct LaunchedChild {
    pub(super) child: CreatedChild,
    pub(super) identity: ProcessIdentity,
}

pub(crate) struct MacosLaunchFailure {
    pub code: &'static str,
    pub message: String,
    pub created: Option<Box<CreatedChild>>,
}

impl std::fmt::Debug for MacosLaunchFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("MacosLaunchFailure")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("has_created_ownership", &self.created.is_some())
            .finish()
    }
}
```

`launch()` 必须：

1. 调用 `validate`。
2. `Command::new(&request.executable)`，逐项 `.args(&request.args)`，设置 cwd 和三个 `Stdio::piped()`。
3. 在 `pre_exec` 中调用 `libc::setsid()`，随后验证 `setsid_return == getpid == getpgid(0) == getsid(0)`；失败返回 `io::Error::last_os_error()`，一致性失败返回 `io::Error::from_raw_os_error(libc::EPROTO)`。
4. spawn 后立即取得三个 pipe、PID，并调用 adapter。
5. 父侧只接受 `identity.pid == identity.pgid == identity.sid`。
6. spawn 前失败映射 `CODEX_PROCESS_CREATE_FAILED` 且不携带 ownership；spawn 后身份失败返回 `MacosLaunchFailure { code: "CODEX_PROCESS_IDENTITY_FAILED", created: Some(Box<CreatedChild>) }`。

`pre_exec` 必须放在带中文 SAFETY 注释的 unsafe block 中，闭包不分配应用对象、不加锁，只调用 async-signal-safe 的进程身份系统调用：

```rust
// SAFETY: 闭包只调用 setsid/getpid/getpgid/getsid 并构造固定 errno，不访问其他线程状态。
unsafe {
    command.pre_exec(|| {
        let session = libc::setsid();
        if session < 0 { return Err(std::io::Error::last_os_error()); }
        let pid = libc::getpid();
        let pgid = libc::getpgid(0);
        let sid = libc::getsid(0);
        if session != pid || pgid != pid || sid != pid {
            return Err(std::io::Error::from_raw_os_error(libc::EPROTO));
        }
        Ok(())
    });
}
```

- [x] **Step 6：运行 launcher 与 identity 测试**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked macos_launcher::tests
```

Expected: 全部 PASS；真 child 同时满足 `SID == PGID == PID`，stdio/argv/cwd 原样通过。

## Task 3：实现 live-host 有界 shutdown

**Files:**

- Create: `src-tauri/src/agent/codex/macos_runtime.rs`
- Create: `src-tauri/src/agent/codex/macos_runtime/tests.rs`
- Modify: `src-tauri/src/agent/codex/mod.rs`
- Modify: `src-tauri/src/agent/codex/macos_launcher.rs`
- Modify: `src-tauri/tests/fixtures/macos_runtime_child.rs`

- [x] **Step 1：先写 graceful 与 escalation 真实进程测试**

先在 `codex/mod.rs` 增加 Runtime 模块声明；测试文件与实现文件在本 Task 同步创建：

```rust
#[cfg(target_os = "macos")]
// Phase 2A 冻结私有进程契约，Phase 2B 才接入产品路径。
#[allow(dead_code)]
pub(crate) mod macos_runtime;
```

测试分别启动 `tree` 和 `ignore-tree` fixture。`runtime_fixture()` 返回 `(TempDir, MacosRuntime, PathBuf, PathBuf)`；`TempDir` 必须由测试持有到进程收口完成，后两个路径分别是 marker 和通过 `PathBuf::from(format!("{}.ready", marker.display()))` 创建的 ready marker：

```rust
static RUNTIME_IDS: AtomicU64 = AtomicU64::new(1);

fn fixture(directory: &Path) -> PathBuf {
    let executable = directory.join("macos-runtime-child");
    let output = Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "macos_runtime_child"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/macos_runtime_child.rs"))
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    executable
}

fn runtime_fixture(mode: &str) -> (tempfile::TempDir, MacosRuntime, PathBuf, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let executable = fixture(directory.path());
    let marker = directory.path().join("leaf.pid");
    let ready = PathBuf::from(format!("{}.ready", marker.display()));
    let runtime = MacosRuntime::create(MacosLaunchRequest {
        executable,
        args: vec![mode.into(), marker.as_os_str().to_owned()],
        current_dir: directory.path().to_owned(),
        runtime_instance_id: format!(
            "macos-runtime-{}-{}",
            std::process::id(),
            RUNTIME_IDS.fetch_add(1, Ordering::Relaxed),
        ),
    })
    .unwrap();
    (directory, runtime, marker, ready)
}

fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(Instant::now() < deadline, "等待 fixture 文件超时: {}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}
```

测试模块显式导入 `std::io::Write`、`AtomicU64`、`Ordering`、`Path`、`PathBuf`、`Command`、`Duration` 和 `Instant`。`fixture()` 与 Task 2 使用同一条固定 `rustc` 命令，不能通过 shell 间接执行。

测试等待 ready marker 后 shutdown：

```rust
#[test]
fn shutdown_reaps_term_responsive_child_and_group() {
    let (_directory, runtime, _marker, ready) = runtime_fixture("tree");
    wait_file(&ready);
    let expected_id = runtime.id.clone();
    let expected_pid = runtime.identity.pid;
    let expected_token = runtime.identity.start_token.clone();
    let evidence = runtime.shutdown(Duration::from_secs(2), Duration::from_secs(2)).unwrap();
    assert_eq!(evidence.runtime_id, expected_id);
    assert_eq!(evidence.leader_pid, expected_pid);
    assert_eq!(evidence.pgid, expected_pid);
    assert_eq!(evidence.process_start_token, expected_token);
    assert!(evidence.observed_at > 0);
    assert!(evidence.direct_child_reaped);
    assert!(evidence.host_continuous_ownership);
    assert!(process_group_members(evidence.pgid).unwrap().is_empty());
}

#[test]
fn shutdown_escalates_to_sigkill_for_ignoring_group() {
    let (_directory, runtime, _marker, ready) = runtime_fixture("ignore-tree");
    wait_file(&ready);
    let started = Instant::now();
    let evidence = runtime.shutdown(Duration::from_millis(100), Duration::from_secs(2)).unwrap();
    assert!(started.elapsed() >= Duration::from_millis(100));
    assert!(process_group_members(evidence.pgid).unwrap().is_empty());
}
```

- [x] **Step 2：运行测试并确认因 Runtime API 缺失而失败**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked shutdown_reaps_term_responsive_child_and_group --no-run
```

Expected: FAIL；`MacosRuntime`、`shutdown` 或 evidence 尚不存在。

- [x] **Step 3：实现 ownership、evidence 和 failure 类型**

在 `macos_runtime.rs` 建立：

```rust
use super::macos_launcher::{self, MacosLaunchRequest, MacosProcessIdentityAdapter,
    ProcessIdentity, process_group_members};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_PHASE_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

pub(crate) struct MacosRuntime {
    id: String,
    child: macos_launcher::CreatedChild,
    identity: ProcessIdentity,
}

impl std::fmt::Debug for MacosRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("MacosRuntime")
            .field("id", &self.id)
            .field("leader_pid", &self.identity.pid)
            .field("pgid", &self.identity.pgid)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub(crate) struct MacosTerminationEvidence {
    runtime_id: String,
    leader_pid: libc::pid_t,
    pgid: libc::pid_t,
    process_start_token: macos_launcher::ProcessStartToken,
    observed_at: i64,
    host_continuous_ownership: bool,
    direct_child_reaped: bool,
}

#[derive(Debug)]
pub(crate) struct MacosRuntimeFailure {
    pub code: &'static str,
    pub message: String,
    pub runtime: Box<MacosRuntime>,
}
```

`MacosRuntime::create(request)` 只包装正常 `launch` 结果。launch 已创建 child 后的失败继续以 launcher failure 返回 created ownership，不能转换成无 ownership 的 Runtime 错误。

```rust
impl MacosRuntime {
    /// 从已验证的 launcher ownership 构造 live-host Runtime。
    pub(crate) fn create(request: MacosLaunchRequest) -> Result<Self, macos_launcher::MacosLaunchFailure> {
        let id = request.runtime_instance_id.clone();
        let launched = macos_launcher::launch(&request)?;
        Ok(Self { id, child: launched.child, identity: launched.identity })
    }
}
```

- [x] **Step 4：实现 bounded poll 和 signal 阶段**

`shutdown(mut self, grace, kill_wait)` 将两个 duration 分别 clamp 到 `MAX_PHASE_TIMEOUT`。实现以下私有 helper：

```rust
fn signal_group(pgid: libc::pid_t, signal: libc::c_int) -> std::io::Result<()> {
    if unsafe { libc::killpg(pgid, signal) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

struct ExitObservation {
    direct_child_reaped: bool,
    members: Vec<libc::pid_t>,
}

fn observe_exit(&mut self) -> std::io::Result<ExitObservation> {
    let direct_child_reaped = self.child.process.try_wait()?.is_some();
    let members = process_group_members(self.identity.pgid)?;
    Ok(ExitObservation { direct_child_reaped, members })
}
```

`shutdown` 对 `observe_exit()` 使用显式 `match`；`Err(error)` 分支在可变借用结束后调用 `self.unknown(error.to_string())`，确保 failure 能返回完整 ownership。

任意失败码都通过保留 ownership 的构造器返回；`unknown` 只是其中的稳定证据不足分支：

```rust
fn failure(self, code: &'static str, message: impl Into<String>) -> MacosRuntimeFailure {
    MacosRuntimeFailure { code, message: message.into(), runtime: Box::new(self) }
}
```

完整 evidence 只由以下构造器生成；`observed_at` 使用 Unix epoch milliseconds，token 从已验证的创建身份 clone：

```rust
fn complete_evidence(&self) -> MacosTerminationEvidence {
    MacosTerminationEvidence {
        runtime_id: self.id.clone(),
        leader_pid: self.identity.pid,
        pgid: self.identity.pgid,
        process_start_token: self.identity.start_token.clone(),
        observed_at: SystemTime::now()
            .duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64,
        host_continuous_ownership: true,
        direct_child_reaped: true,
    }
}
```

完整算法：

1. 初次 adapter 观测匹配；若 leader 不可观测，则仅在 direct child 已回收且 group 为空时完成，否则 unknown。
2. `killpg(pgid, SIGTERM)`；`ESRCH` 仍进入证据轮询，不直接完成，其他错误返回 `CODEX_PROCESS_GROUP_SIGNAL_FAILED` 并保留 Runtime ownership。
3. 每 10ms 轮询 child 和 group。两者完成则构造 evidence；查询失败立即 unknown。
4. grace 到期仍非空：leader 存在则重新匹配 token/SID/PGID；leader 已退出则只接受此前每次 group 观测均非空的 live-host 路径。
5. `killpg(pgid, SIGKILL)`；同样不把 `ESRCH` 单独视为证据，其他错误返回 `CODEX_PROCESS_GROUP_SIGNAL_FAILED` 并保留 Runtime ownership。
6. kill deadline 内只在 child 已回收且 group 为空时成功，否则 unknown 并返回 Runtime ownership。

- [x] **Step 5：运行 shutdown 测试**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked shutdown_reaps_term_responsive_child_and_group
cargo test --manifest-path src-tauri/Cargo.toml --locked shutdown_escalates_to_sigkill_for_ignoring_group
```

Expected: 两个测试 PASS；第二个耗时不短于 grace 且短于总 deadline。

## Task 4：冻结 unknown、leader-loss 和证据上限

**Files:**

- Modify: `src-tauri/src/agent/codex/macos_runtime.rs`
- Modify: `src-tauri/src/agent/codex/macos_runtime/tests.rs`
- Modify: `src-tauri/src/agent/codex/macos_launcher.rs`
- Modify: `src-tauri/src/agent/codex/macos_launcher/tests.rs`

- [x] **Step 1：先写 token mismatch 与 leader-loss 测试**

增加以下 test-only 身份替换入口，仅在模块内测试可见：

```rust
#[cfg(test)]
fn replace_identity_for_test(&mut self, identity: ProcessIdentity) {
    self.identity = identity;
}
```

增加以下两个测试：

```rust
#[test]
fn token_mismatch_returns_unknown_and_retains_runtime() {
    let (_directory, mut runtime, _marker, ready) = runtime_fixture("ignore-tree");
    wait_file(&ready);
    let mut mismatched = runtime.identity.clone();
    mismatched.start_token.microseconds += 1;
    runtime.replace_identity_for_test(mismatched);
    let pgid = runtime.identity.pgid;
    let failure = runtime.shutdown(Duration::from_millis(20), Duration::from_millis(20)).unwrap_err();
    assert_eq!(failure.code, "CODEX_RUNTIME_TERMINATION_UNCONFIRMED");
    assert_eq!(failure.runtime.identity.pgid, pgid);
    assert!(!process_group_members(pgid).unwrap().is_empty());
    cleanup_fixture(*failure.runtime);
}

#[test]
fn missing_leader_with_live_group_is_unknown_without_signalling_group() {
    let (_directory, mut runtime, _marker, ready) = runtime_fixture("leader-exit");
    wait_file(&ready);
    runtime.child.stdin.write_all(b"x").unwrap();
    wait_direct_child_exit(&mut runtime);
    let pgid = runtime.identity.pgid;
    let failure = runtime.shutdown(Duration::from_millis(20), Duration::from_millis(20)).unwrap_err();
    assert_eq!(failure.code, "CODEX_RUNTIME_TERMINATION_UNCONFIRMED");
    assert!(!process_group_members(pgid).unwrap().is_empty());
    cleanup_fixture(*failure.runtime);
}
```

直接 child 等待 helper 必须有界并通过 `try_wait()` 回收：

```rust
fn wait_direct_child_exit(runtime: &mut MacosRuntime) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if runtime.child.process.try_wait().unwrap().is_some() { break; }
        assert!(Instant::now() < deadline, "直接 child 未按期退出");
        std::thread::sleep(Duration::from_millis(10));
    }
}
```

测试模块使用以下专用清理，不生成 evidence，也不进入生产代码：

```rust
fn cleanup_fixture(mut runtime: MacosRuntime) {
    let pgid = runtime.identity.pgid;
    let _ = signal_group(pgid, libc::SIGKILL);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let _ = runtime.child.process.try_wait();
        if process_group_members(pgid).unwrap_or_default().is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "测试 fixture process group 未退出");
        std::thread::sleep(Duration::from_millis(10));
    }
}
```

- [x] **Step 2：写“TERM 后 leader 退出、grandchild 留组”升级测试**

fixture 增加 `leader-term-exit`：leader 保持默认 SIGTERM，leaf 忽略 SIGTERM。测试断言 shutdown 在 grace 后成功升级 `SIGKILL`，证明 live-host 连续 ownership 路径与 Startup Recovery 的 fail-closed 边界不同。

```rust
#[test]
fn leader_exits_after_term_but_continuous_group_escalates() {
    let (_directory, runtime, _marker, ready) = runtime_fixture("leader-term-exit");
    wait_file(&ready);
    let evidence = runtime
        .shutdown(Duration::from_millis(100), Duration::from_secs(2))
        .unwrap();
    assert!(evidence.direct_child_reaped);
    assert!(process_group_members(evidence.pgid).unwrap().is_empty());
}
```

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked leader_exits_after_term_but_continuous_group_escalates
```

Expected: 初次 FAIL；fixture mode 或升级分支尚未实现。

- [x] **Step 3：覆盖创建后立即 shutdown**

不等待 fixture 的 ready marker，启动 `tree` 后立即 shutdown；成功后验证 group 为空。该测试证明 caller 在 child 完成自身初始化前收口也不会丢失 launcher ownership：

```rust
#[test]
fn shutdown_immediately_after_launch_leaves_no_process_group() {
    let (_directory, runtime, _marker, _ready) = runtime_fixture("tree");
    let pgid = runtime.identity.pgid;
    let evidence = runtime
        .shutdown(Duration::from_secs(1), Duration::from_secs(2))
        .unwrap();
    assert_eq!(evidence.pgid, pgid);
    assert!(process_group_members(pgid).unwrap().is_empty());
}
```

- [x] **Step 4：实现 unknown 分支且保证不构造 evidence**

unknown 分支统一使用：

```rust
fn unknown(self, message: impl Into<String>) -> MacosRuntimeFailure {
    MacosRuntimeFailure {
        code: "CODEX_RUNTIME_TERMINATION_UNCONFIRMED",
        message: message.into(),
        runtime: Box::new(self),
    }
}
```

所有 identity mismatch、leader 首次不可观测但 group 非空、group query 失败和 deadline 超时都走 `unknown`。`complete_evidence()` 保持私有，只能从 `(direct_child_reaped == true && members.is_empty())` 分支调用。

- [x] **Step 5：验证完整 macOS Runtime 契约测试**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked macos_launcher::tests
cargo test --manifest-path src-tauri/Cargo.toml --locked macos_runtime::tests
```

Expected: 输入、真实 SID/PGID/PID、token、grace、escalation、leader-loss 和 unknown 全部 PASS；测试结束后无 fixture process group 残留。

## Task 5：全量验证、范围审计与 Trellis 收口

**Files:**

- Modify: `.trellis/tasks/09-21-macos-p2a-runtime-contract/prd.md`
- Modify: `.trellis/tasks/09-21-macos-p2a-runtime-contract/implement.md`
- Modify: `.trellis/tasks/09-21-macos-p2a-runtime-contract/implement.jsonl`
- Modify: `.trellis/tasks/09-21-macos-p2a-runtime-contract/check.jsonl`
- Modify: `.trellis/workspace/codex/journal-1.md`

- [x] **Step 1：运行 Rust 定向与全量 Gate**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo check --manifest-path src-tauri/Cargo.toml --locked
cargo test --manifest-path src-tauri/Cargo.toml --locked --no-run
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
uname -m
sw_vers -productVersion
```

Expected: 全部 exit 0；记录总测试数量、ignored 数量、实际架构与实际 macOS 版本。只有输出为 `arm64` 且系统版本处于已批准的 macOS 12+ 范围，才勾选当前主机真机 Gate；不得把较新系统测试写成 macOS 12.0 最低版本实测。

- [x] **Step 2：运行前端回归 Gate**

```bash
npm run lint
npm run build
npm test
```

Expected: 全部 exit 0；Phase 2A 没有前端行为变化。

- [x] **Step 3：审计任务边界和 Windows 不变式**

```bash
git diff --exit-code 9153995 -- src-tauri/src/agent/codex/windows_launcher.rs src-tauri/src/agent/codex/runtime.rs
git diff --name-only 9153995
git diff --check
```

Expected:

- 前两个 Windows Runtime 文件无 diff；
- 没有 StateStore、schema、Claim、Provider、Pool、Discovery 或 Startup Recovery 文件；
- 工作区没有 whitespace error。

- [x] **Step 4：更新验收记录**

仅在对应自动化和真机测试确实通过后勾选 `prd.md`。把实际命令、结果、目标 macOS/架构和已知边界写入当前 Trellis journal；不要把 arm64/macOS 12 真机 Gate 在未经验证时标记为通过。

- [x] **Step 5：运行 Trellis check 与 spec 判断**

按 `.trellis/workflow.md` 执行 `trellis-check`。本任务若只增加 macOS 私有实现且没有项目 backend spec layer，则在 journal 记录“不更新 `.trellis/spec`”及原因；不得创建与现有 spec 结构不一致的新层。

- [x] **Step 6：提交前向用户展示单次提交计划并获得确认**

提交计划：

```text
feat(macos): add live runtime process contract
chore(trellis): complete macos phase 2a
```

第一个提交包括两个 macOS-only 模块、fixture、测试和 Cargo/module wiring；第二个提交包括 P2a 验收记录、归档和 session。两者均不包含 Phase 2B。

- [x] **Step 7：确认后提交、归档 P2a 并记录 session**

按 Trellis Phase 3.4 顺序执行：先暂存源码并运行 `git diff --cached --check`，创建 `feat(macos)` 提交；再归档 P2a、用源码提交 hash 记录 session、暂存 Trellis 文件并创建 `chore(trellis)` 提交。若 session 工具先自动提交 journal，则 amend 到已确认的 Trellis 收口提交；不推送远端。
