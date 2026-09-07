// secm-core::netif — 网络适配器查询（薄封装 secm-datasource::netif）
// 供 NetConfig 页展示适配器列表与当前配置；供 Dashboard 网络流量卡高频采样。

use std::collections::HashMap;

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

/// 各接口累计收发字节数（别名 → (下行 InOctets, 上行 OutOctets)）。
/// GetIfTable2 累计八位组，两次快照差分可算任意间隔速率（支持 0.5s 高频轮询）。
pub fn if_bytes_map() -> HashMap<String, (u64, u64)> {
    netif::if_bytes_map()
}

/// 各已连接网卡链路协商速度（别名 → "1 Gbps"）
pub fn link_speeds() -> HashMap<String, String> {
    netif::link_speeds().unwrap_or_default()
}

/// 当前活跃（ESTABLISHED）TCP 连接数（IPv4 全表统计；失败 0）
pub fn tcp_connection_count() -> u32 {
    secm_datasource::net_io::tcp_connection_count()
}
