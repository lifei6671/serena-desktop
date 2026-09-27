#[cfg(windows)]
#[allow(dead_code, reason = "CB7-004 内部 Activity mapper，能力仍冻结")]
mod activity;
#[allow(
    dead_code,
    reason = "CB6-003 建立 transport primitive，CB7 才接产品执行"
)]
pub(crate) mod client;
pub(crate) mod discovery;
#[cfg(windows)]
#[allow(
    dead_code,
    reason = "CB7-002 准备 primitive，CB7-005 才接 execute lifecycle"
)]
pub(crate) mod fresh;
#[cfg(windows)]
#[allow(
    dead_code,
    reason = "CB7-003 内部 prompt primitive，CB7-005 才接生命周期"
)]
pub(crate) mod prompt;
#[allow(dead_code, reason = "CB6-003 建立协议边界，CB7 才接产品执行")]
pub(crate) mod protocol;
pub(crate) mod provider;
pub(crate) mod recovery;
#[cfg(windows)]
#[allow(dead_code, reason = "CB6-003 仅 managed handshake，不接产品 session")]
pub(crate) mod runtime;
#[allow(
    dead_code,
    reason = "CB6-004 提供私有 durable store，后续卡才接 Adapter"
)]
pub(crate) mod store;
#[cfg(windows)]
#[allow(
    dead_code,
    reason = "CB6-002 先冻结 launcher primitive，后续 CB6-003/CB7-002 才接入 Runtime"
)]
pub(crate) mod windows_launcher;

#[cfg(test)]
tokio::task_local! {
    /// 测试只在当前异步作用域替换 discovery 结果，不读取开发机 PATH 或 Registry。
    pub(crate) static TEST_DISCOVERY: Result<discovery::DiscoveryResult, discovery::DiscoveryError>;
}

/// 执行一次无进程 discovery；测试 override 与生产 resolver 共用同一注册边界。
pub(crate) fn discover() -> Result<discovery::DiscoveryResult, discovery::DiscoveryError> {
    #[cfg(test)]
    if let Ok(result) = TEST_DISCOVERY.try_with(Clone::clone) {
        return result;
    }
    discovery::discover(discovery::DiscoveryInput::system())
}
