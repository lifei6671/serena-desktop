use chrono::Local;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

pub fn ensure_directory(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("无法创建日志目录 {}：{error}", path.display()))
}

pub fn append(path: &Path, source: &str, message: &str) {
    let parent = match path.parent() {
        Some(parent) => parent,
        None => return,
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }

    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    for line in message.lines() {
        let _ = writeln!(file, "{timestamp} [{source}] {line}");
    }
}
