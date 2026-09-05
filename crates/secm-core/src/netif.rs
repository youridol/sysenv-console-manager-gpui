// secm-core::netif — 网络适配器查询（薄封装 secm-datasource::netif）
// 供 NetConfig 页展示适配器列表与当前配置。

/// 适配器完整配置类型（重新导出供 UI 层消费）
pub use secm_datasource::netif::{self, AdapterConfig};

/// 查询所有非回环适配器完整网络配置
pub fn list_adapters() -> Result<Vec<AdapterConfig>, String> {
    netif::adapter_configs().map_err(|e| e.to_string())
}

/// 本机首个非回环 IPv4/IPv6（Dashboard 网络卡预留）
#[allow(dead_code)]
pub fn local_ips() -> Result<(Option<String>, Option<String>), String> {
    netif::local_ips()
        .map(|l| (l.ipv4, l.ipv6))
        .map_err(|e| e.to_string())
}
