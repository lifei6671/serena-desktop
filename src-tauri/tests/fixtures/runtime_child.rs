//! Test process tree only. Normal Windows descendant creation inherits Job membership.
use std::{
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    time::Duration,
};
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let directory = std::path::PathBuf::from(&args[1]);
    let mode = args[2].to_str().unwrap();
    if mode == "leaf" {
        std::fs::write(directory.join("leaf.pid"), std::process::id().to_string()).unwrap();
    } else {
        let _child = Command::new(std::env::current_exe().unwrap())
            .arg(&directory)
            .arg("leaf")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap();
        while !directory.join("leaf.pid").exists() {
            std::thread::sleep(Duration::from_millis(1));
        }
        std::fs::write(directory.join("main.pid"), std::process::id().to_string()).unwrap();
        if mode == "main-exit" {
            return;
        }
    }
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
