//! CodeBuddy launcher 的独立 Windows child-tree fixture，不依赖 host crate。

use std::{
    ffi::c_void,
    io::{self, BufRead},
    os::windows::ffi::OsStrExt,
    process::Command,
};

#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn IsProcessInJob(process: *mut c_void, job: *mut c_void, result: *mut i32) -> i32;
    fn GetProcessTimes(
        process: *mut c_void,
        created: *mut FileTime,
        exited: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn QueryInformationJobObject(
        job: *mut c_void,
        class: i32,
        data: *mut c_void,
        size: u32,
        returned: *mut u32,
    ) -> i32;
    fn SetEvent(event: *mut c_void) -> i32;
}

/// 以 UTF-16 hex 输出测试值，避免非 Unicode OS string 丢失。
fn encoded(value: &std::ffi::OsStr) -> String {
    value
        .encode_wide()
        .map(|unit| format!("{unit:04x}"))
        .collect()
}

fn main() {
    // 第一个应用操作先观察 Job membership，早于 argv/env/stdio/业务处理。
    let mut member = 0;
    let member_ok =
        unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut member) };
    let (mut created, mut exited, mut kernel, mut user) = (
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
    );
    assert_ne!(
        unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        },
        0
    );
    println!(
        "READY:{member_ok}:{member}:{:08x}{:08x}",
        created.high, created.low
    );

    let arguments: Vec<_> = std::env::args_os().collect();
    if arguments
        .get(1)
        .is_some_and(|value| value == "--descendant")
    {
        println!("DESCENDANT:{member_ok}:{member}");
        return;
    }
    if arguments
        .get(1)
        .is_some_and(|value| value == "--spawn-descendant")
    {
        let executable = arguments.get(2).expect("缺少 descendant executable");
        let status = Command::new(executable)
            .arg("--descendant")
            .status()
            .expect("无法启动 descendant fixture");
        assert!(status.success());
    }

    for argument in &arguments {
        println!("ARG:{}", encoded(argument));
    }
    eprintln!("STDERR:separate");
    for line in io::stdin().lock().lines() {
        let line = line.expect("无法读取 stdin");
        if let Some(probes) = line.strip_prefix("PROBE ") {
            let handles: Vec<usize> = probes
                .split_whitespace()
                .map(|value| value.parse().expect("非法 probe handle"))
                .collect();
            let mut restrictions = 0u32;
            assert_ne!(
                unsafe {
                    QueryInformationJobObject(
                        std::ptr::null_mut(),
                        4,
                        (&mut restrictions as *mut u32).cast(),
                        std::mem::size_of_val(&restrictions) as u32,
                        std::ptr::null_mut(),
                    )
                },
                0
            );
            // Job 与额外 inheritable Event 都不在 launcher 的显式白名单中。
            let job_visible = unsafe {
                QueryInformationJobObject(
                    handles[0] as *mut c_void,
                    4,
                    (&mut restrictions as *mut u32).cast(),
                    std::mem::size_of_val(&restrictions) as u32,
                    std::ptr::null_mut(),
                )
            };
            let event_visible = unsafe { SetEvent(handles[1] as *mut c_void) };
            println!("PROBE:{job_visible}:{event_visible}");
        } else if let Some(name) = line.strip_prefix("ENV ") {
            let value = std::env::var_os(name).unwrap_or_default();
            println!("ENV:{name}:{}", encoded(&value));
        } else {
            println!("INPUT:{line}");
        }
    }
    println!("EOF");
}
