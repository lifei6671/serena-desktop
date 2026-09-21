// Copyright 2019-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT
//
// SerenaDesktop 修改说明：基于 tauri-plugin-single-instance 2.4.4，修复 Windows
// mutex 已存在而 event HWND 尚未就绪时的第二实例漏放行问题。详见 README.serena-patch.md。

#[cfg(feature = "semver")]
use crate::semver_compat::semver_compat_string;

use crate::SingleInstanceCallback;
use std::time::{Duration, Instant};
use tauri::{
    plugin::{self, TauriPlugin},
    AppHandle, Manager, RunEvent, Runtime,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WAIT_ABANDONED,
        WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM,
    },
    System::{
        DataExchange::COPYDATASTRUCT,
        LibraryLoader::GetModuleHandleW,
        Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
    },
    UI::WindowsAndMessaging::{
        self as w32wm, AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow,
        FindWindowW, GetWindowThreadProcessId, RegisterClassExW, SendMessageW, CREATESTRUCTW,
        GWLP_USERDATA, GWL_STYLE, WINDOW_LONG_PTR_INDEX, WM_COPYDATA, WM_CREATE, WM_DESTROY,
        WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
        WS_OVERLAPPED, WS_POPUP, WS_VISIBLE,
    },
};

const WMCOPYDATA_SINGLE_INSTANCE_DATA: usize = 1542;
const IPC_READY_TIMEOUT: Duration = Duration::from_secs(5);
const IPC_READY_POLL_LIMIT_MS: u32 = 50;

struct MutexHandle(isize);

struct TargetWindowHandle(isize);

struct UserData<R: Runtime> {
    app: AppHandle<R>,
    callback: Box<SingleInstanceCallback<R>>,
}

impl<R: Runtime> UserData<R> {
    unsafe fn from_hwnd_raw(hwnd: HWND) -> *mut Self {
        GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Self
    }

    unsafe fn from_hwnd<'a>(hwnd: HWND) -> &'a mut Self {
        &mut *Self::from_hwnd_raw(hwnd)
    }

    fn run_callback(&mut self, args: Vec<String>, cwd: String) {
        (self.callback)(&self.app, args, cwd)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MutexWaitResult {
    Acquired,
    Abandoned,
    Timeout,
    Failed,
}

#[derive(Debug, PartialEq, Eq)]
enum OwnershipDecision<T> {
    Primary,
    Forward(T),
    FailClosed,
}

// 此 helper 将竞态决策与 Win32 调用拆开，测试可确定性覆盖所有 ownership 分支。
fn resolve_secondary_ownership<T>(
    mut find_window: impl FnMut() -> Option<T>,
    mut wait_mutex: impl FnMut(u32) -> MutexWaitResult,
    mut remaining_wait_ms: impl FnMut() -> Option<u32>,
) -> OwnershipDecision<T> {
    loop {
        if let Some(hwnd) = find_window() {
            return OwnershipDecision::Forward(hwnd);
        }

        let Some(wait_ms) = remaining_wait_ms() else {
            return OwnershipDecision::FailClosed;
        };

        match wait_mutex(wait_ms) {
            MutexWaitResult::Acquired | MutexWaitResult::Abandoned => {
                return OwnershipDecision::Primary;
            }
            MutexWaitResult::Timeout => {}
            MutexWaitResult::Failed => return OwnershipDecision::FailClosed,
        }
    }
}

// 计算单次等待时间，确保任何一次 Win32 wait 都不越过总 deadline。
fn remaining_wait_ms(deadline: Instant) -> Option<u32> {
    let remaining = deadline.checked_duration_since(Instant::now())?;
    let millis = remaining.as_millis().max(1).min(IPC_READY_POLL_LIMIT_MS as u128);
    Some(millis as u32)
}

fn wait_result(result: u32) -> MutexWaitResult {
    match result {
        WAIT_OBJECT_0 => MutexWaitResult::Acquired,
        WAIT_ABANDONED => MutexWaitResult::Abandoned,
        WAIT_TIMEOUT => MutexWaitResult::Timeout,
        WAIT_FAILED => MutexWaitResult::Failed,
        _ => MutexWaitResult::Failed,
    }
}

pub fn init<R: Runtime>(callback: Box<SingleInstanceCallback<R>>) -> TauriPlugin<R> {
    plugin::Builder::new("single-instance")
        .setup(|app, _api| {
            #[allow(unused_mut)]
            let mut id = app.config().identifier.clone();
            #[cfg(feature = "semver")]
            {
                id.push('_');
                id.push_str(semver_compat_string(&app.package_info().version).as_str());
            }

            let class_name = encode_wide(format!("{id}-sic"));
            let window_name = encode_wide(format!("{id}-siw"));
            let mutex_name = encode_wide(format!("{id}-sim"));

            let hmutex = unsafe { CreateMutexW(std::ptr::null(), true.into(), mutex_name.as_ptr()) };
            if hmutex.is_null() {
                tracing::error!("single-instance failed to create the ownership mutex");
                return Err(std::io::Error::last_os_error().into());
            }

            let decision: OwnershipDecision<HWND> = if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                let deadline = Instant::now() + IPC_READY_TIMEOUT;
                resolve_secondary_ownership(
                    || {
                        let hwnd = unsafe { FindWindowW(class_name.as_ptr(), window_name.as_ptr()) };
                        (!hwnd.is_null()).then_some(hwnd)
                    },
                    |wait_ms| wait_result(unsafe { WaitForSingleObject(hmutex, wait_ms) }),
                    || remaining_wait_ms(deadline),
                )
            } else {
                OwnershipDecision::Primary
            };

            match decision {
                OwnershipDecision::Forward(hwnd) => {
                    forward_to_primary(hwnd);
                    // Secondary 从未取得 mutex ownership，只关闭自己的观察句柄。
                    unsafe { CloseHandle(hmutex) };
                    app.cleanup_before_exit();
                    std::process::exit(0);
                }
                OwnershipDecision::FailClosed => {
                    // 不能证明 ownership 已移交时，禁止第二 Host 进入应用 setup。
                    unsafe { CloseHandle(hmutex) };
                    tracing::error!("single-instance ownership or activation IPC was not ready");
                    return Err(std::io::Error::other(
                        "single-instance ownership or activation IPC was not ready",
                    )
                    .into());
                }
                OwnershipDecision::Primary => {}
            }

            // 新建或接管 mutex 的唯一 primary 将句柄托管到应用退出生命周期。
            app.manage(MutexHandle(hmutex as _));

            let userdata = UserData {
                app: app.clone(),
                callback,
            };
            let userdata = Box::into_raw(Box::new(userdata));
            let hwnd = create_event_target_window::<R>(&class_name, &window_name, userdata);
            app.manage(TargetWindowHandle(hwnd as _));

            Ok(())
        })
        .on_event(|app, event| {
            if let RunEvent::Exit = event {
                destroy(app);
            }
        })
        .build()
}

fn forward_to_primary(hwnd: HWND) {
    // secondary 将一次性前台权限交给 primary，供回调中的 window.set_focus 使用。
    unsafe {
        let mut primary_pid = 0;
        GetWindowThreadProcessId(hwnd, &mut primary_pid);
        if primary_pid != 0 {
            AllowSetForegroundWindow(primary_pid);
        }
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let cwd = cwd.to_str().unwrap_or_default();
    let args = std::env::args().collect::<Vec<String>>().join("|");
    let data = format!("{cwd}|{args}\0");
    let bytes = data.as_bytes();
    let cds = COPYDATASTRUCT {
        dwData: WMCOPYDATA_SINGLE_INSTANCE_DATA,
        cbData: bytes.len() as _,
        lpData: bytes.as_ptr() as _,
    };
    unsafe { SendMessageW(hwnd, WM_COPYDATA, 0, &cds as *const _ as _) };
}

pub fn destroy<R: Runtime, M: Manager<R>>(manager: &M) {
    if let Some(hmutex) = manager.try_state::<MutexHandle>() {
        unsafe {
            ReleaseMutex(hmutex.0 as _);
            CloseHandle(hmutex.0 as _);
        }
    }
    if let Some(hwnd) = manager.try_state::<TargetWindowHandle>() {
        unsafe { DestroyWindow(hwnd.0 as _) };
    }
}

unsafe extern "system" fn single_instance_window_proc<R: Runtime>(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let create_struct = &*(lparam as *const CREATESTRUCTW);
            let userdata = create_struct.lpCreateParams as *const UserData<R>;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, userdata as _);
            0
        }
        WM_COPYDATA => {
            let cds_ptr = lparam as *const COPYDATASTRUCT;
            if (*cds_ptr).dwData == WMCOPYDATA_SINGLE_INSTANCE_DATA {
                let userdata = UserData::<R>::from_hwnd(hwnd);
                let data = std::ffi::CStr::from_ptr((*cds_ptr).lpData as _).to_string_lossy();
                let mut values = data.split('|');
                let cwd = values.next().unwrap();
                let args = values.map(|value| value.to_string()).collect();
                userdata.run_callback(args, cwd.to_string());
            }
            1
        }
        WM_DESTROY => {
            let userdata = UserData::<R>::from_hwnd_raw(hwnd);
            drop(Box::from_raw(userdata));
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn create_event_target_window<R: Runtime>(
    class_name: &[u16],
    window_name: &[u16],
    userdata: *const UserData<R>,
) -> HWND {
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(single_instance_window_proc::<R>),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(std::ptr::null()),
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };
        RegisterClassExW(&class);
        let hwnd = CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            window_name.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            userdata as _,
        );
        SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_VISIBLE | WS_POPUP) as isize);
        hwnd
    }
}

pub fn encode_wide(string: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    std::os::windows::prelude::OsStrExt::encode_wide(string.as_ref())
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_pointer_width = "32")]
#[allow(non_snake_case)]
unsafe fn SetWindowLongPtrW(hwnd: HWND, index: WINDOW_LONG_PTR_INDEX, value: isize) -> isize {
    w32wm::SetWindowLongW(hwnd, index, value as _) as _
}

#[cfg(target_pointer_width = "64")]
#[allow(non_snake_case)]
unsafe fn SetWindowLongPtrW(hwnd: HWND, index: WINDOW_LONG_PTR_INDEX, value: isize) -> isize {
    w32wm::SetWindowLongPtrW(hwnd, index, value)
}

#[cfg(target_pointer_width = "32")]
#[allow(non_snake_case)]
unsafe fn GetWindowLongPtrW(hwnd: HWND, index: WINDOW_LONG_PTR_INDEX) -> isize {
    w32wm::GetWindowLongW(hwnd, index) as _
}

#[cfg(target_pointer_width = "64")]
#[allow(non_snake_case)]
unsafe fn GetWindowLongPtrW(hwnd: HWND, index: WINDOW_LONG_PTR_INDEX) -> isize {
    w32wm::GetWindowLongPtrW(hwnd, index)
}

#[cfg(test)]
mod tests {
    use super::{resolve_secondary_ownership, MutexWaitResult, OwnershipDecision};
    use std::collections::VecDeque;

    #[test]
    fn new_mutex_owner_is_primary() {
        assert_eq!(OwnershipDecision::<()>::Primary, OwnershipDecision::Primary);
    }

    #[test]
    fn ready_window_forwards_without_waiting() {
        let decision = resolve_secondary_ownership(
            || Some(7_u32),
            |_| panic!("ready IPC must not wait for mutex"),
            || Some(50),
        );
        assert_eq!(decision, OwnershipDecision::Forward(7));
    }

    #[test]
    fn absent_window_then_ready_window_forwards() {
        let mut windows = VecDeque::from([None, Some(9_u32)]);
        let decision = resolve_secondary_ownership(
            || windows.pop_front().flatten(),
            |_| MutexWaitResult::Timeout,
            || Some(50),
        );
        assert_eq!(decision, OwnershipDecision::Forward(9));
    }

    #[test]
    fn released_or_abandoned_owner_allows_takeover() {
        for result in [MutexWaitResult::Acquired, MutexWaitResult::Abandoned] {
            let decision = resolve_secondary_ownership(|| None::<u32>, |_| result, || Some(50));
            assert_eq!(decision, OwnershipDecision::Primary);
        }
    }

    #[test]
    fn deadline_without_window_fails_closed() {
        let decision = resolve_secondary_ownership(
            || None::<u32>,
            |_| panic!("expired deadline must not wait"),
            || None,
        );
        assert_eq!(decision, OwnershipDecision::FailClosed);
    }

    #[test]
    fn wait_failure_fails_closed() {
        let decision = resolve_secondary_ownership(
            || None::<u32>,
            |_| MutexWaitResult::Failed,
            || Some(50),
        );
        assert_eq!(decision, OwnershipDecision::FailClosed);
    }
}
