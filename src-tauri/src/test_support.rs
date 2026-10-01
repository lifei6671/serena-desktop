use std::{
    collections::HashSet,
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    sync::{Mutex, OnceLock},
};

const BROKER_TEST_PORT_START: u16 = 20_000;
const BROKER_TEST_PORT_END: u16 = 39_999;

/// Broker/Remote tests restart listeners on the same configured port. Do not allocate those
/// ports from the OS ephemeral pool: after a listener is released, a parallel outbound TCP
/// connection may legitimately reuse that source port before the Broker rebinds it.
///
/// Keep every selected test port leased for this process and probe a dedicated non-ephemeral
/// range so parallel tests remain enabled without racing the runner's outbound socket allocator.
fn broker_test_ports() -> &'static Mutex<HashSet<u16>> {
    static PORTS: OnceLock<Mutex<HashSet<u16>>> = OnceLock::new();
    PORTS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn unique_listener(ip: Ipv4Addr) -> TcpListener {
    let mut leased = broker_test_ports()
        .lock()
        .expect("Broker test port registry poisoned");
    // 测试套件会拉起子进程；进程内 HashSet 无法阻止父子进程从同一个 20000 起点
    // 选中相同端口。按 PID 打散扫描起点，同时保留专用非 ephemeral 端口段。
    let span = u32::from(BROKER_TEST_PORT_END - BROKER_TEST_PORT_START) + 1;
    let start = std::process::id().wrapping_mul(7_919) % span;
    for delta in 0..span {
        let offset = (start + delta) % span;
        let port = BROKER_TEST_PORT_START + u16::try_from(offset).expect("Broker test port offset");
        if leased.contains(&port) {
            continue;
        }
        let address = SocketAddr::new(IpAddr::V4(ip), port);
        match TcpListener::bind(address) {
            Ok(listener) => {
                leased.insert(port);
                return listener;
            }
            Err(error) if error.kind() == ErrorKind::AddrInUse => continue,
            Err(error) => panic!("bind Broker test listener {address}: {error}"),
        }
    }
    panic!("no Broker test port available in {BROKER_TEST_PORT_START}..={BROKER_TEST_PORT_END}");
}

pub(crate) fn broker_loopback_listener() -> TcpListener {
    unique_listener(Ipv4Addr::LOCALHOST)
}

pub(crate) fn broker_wildcard_listener() -> TcpListener {
    unique_listener(Ipv4Addr::UNSPECIFIED)
}

pub(crate) fn broker_loopback_port() -> u16 {
    broker_loopback_listener()
        .local_addr()
        .expect("read Broker loopback test address")
        .port()
}
