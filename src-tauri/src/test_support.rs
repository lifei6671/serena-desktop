use std::{
    collections::HashSet,
    net::TcpListener,
    sync::{Mutex, OnceLock},
};

/// Broker/Remote tests often need an OS-assigned port before the product listener starts.
/// Record every leased port for the lifetime of this test process so parallel tests cannot
/// receive the same ephemeral port during that handoff window.
fn broker_test_ports() -> &'static Mutex<HashSet<u16>> {
    static PORTS: OnceLock<Mutex<HashSet<u16>>> = OnceLock::new();
    PORTS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn unique_listener(bind_address: &str) -> TcpListener {
    loop {
        let listener = TcpListener::bind(bind_address).expect("bind Broker test listener");
        let port = listener
            .local_addr()
            .expect("read Broker test listener address")
            .port();
        if broker_test_ports()
            .lock()
            .expect("Broker test port registry poisoned")
            .insert(port)
        {
            return listener;
        }
    }
}

pub(crate) fn broker_loopback_listener() -> TcpListener {
    unique_listener("127.0.0.1:0")
}

pub(crate) fn broker_wildcard_listener() -> TcpListener {
    unique_listener("0.0.0.0:0")
}

pub(crate) fn broker_loopback_port() -> u16 {
    broker_loopback_listener()
        .local_addr()
        .expect("read Broker loopback test address")
        .port()
}
