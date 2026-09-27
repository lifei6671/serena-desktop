//! CB6-005 原 PID 退出后仍存活的受管 descendant；不启动真实 Provider。
use std::{io::Write, process::Command, time::Duration};
/// parent 创建继承 Job 的 descendant 后退出，leaf 保持运行供 Job recovery 验证。
fn main() {
    if std::env::args().any(|arg| arg == "--leaf") {
        println!("LEAF:{}", std::process::id());
        std::io::stdout().flush().unwrap();
        std::thread::sleep(Duration::from_secs(60));
    } else {
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--leaf")
            .spawn()
            .unwrap();
        println!("PARENT:{}:{}", std::process::id(), child.id());
        std::io::stdout().flush().unwrap();
    }
}
