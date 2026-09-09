//! Standalone test Child; no Codex, shell, or dependency on the host library.
use std::{
    ffi::c_void,
    io::{self, BufRead},
    os::windows::ffi::OsStrExt,
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
fn main() {
    // First application operation, before input/argv/output/business work.
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
    for arg in std::env::args_os() {
        let encoded = arg
            .encode_wide()
            .map(|unit| format!("{unit:04x}"))
            .collect::<String>();
        println!("ARG:{encoded}");
    }
    eprintln!("STDERR:separate");
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        if let Some(probes) = line.strip_prefix("PROBE ") {
            let handles: Vec<usize> = probes
                .split_whitespace()
                .map(|s| s.parse().unwrap())
                .collect();
            // JobObjectBasicUIRestrictions is one DWORD. First calibrate the
            // same probe against our current Job (NULL means the calling Job),
            // without opening or retaining any Job handle in this Child.
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
        } else {
            println!("INPUT:{line}");
        }
    }
    println!("EOF");
}
