// secm-core::network — 网络诊断同步工具（网站测试/端口探测/DNS 解析）
// Phase 4 首批：纯 std/ureq 实现，无 tokio 运行时依赖（GPUI 异步面由 UI 层调度）。

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// DNS 解析（A/AAAA 记录，std ToSocketAddrs）
/// 返回 (IPv4 列表, IPv6 列表)；失败返回错误消息
pub fn resolve_host(host: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let addrs = (host, 0u16)
        .to_socket_addrs()
        .map_err(|e| format!("域名解析失败 '{}': {}", host, e))?
        .collect::<Vec<_>>();
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for a in addrs {
        match a.ip() {
            std::net::IpAddr::V4(ip) => {
                if !v4.contains(&ip.to_string()) {
                    v4.push(ip.to_string());
                }
            }
            std::net::IpAddr::V6(ip) => {
                if !v6.contains(&ip.to_string()) {
                    v6.push(ip.to_string());
                }
            }
        }
    }
    if v4.is_empty() && v6.is_empty() {
        return Err(format!("'{}' 无解析结果", host));
    }
    Ok((v4, v6))
}

/// TCP 端口连通性探测（connect_timeout）
/// 返回 Ok(true)=通；Ok(false)=不通（被拒/超时/不可达）
pub fn probe_tcp(host: &str, port: u16, timeout_ms: u64) -> Result<bool, String> {
    if port == 0 {
        return Err("端口必须 > 0".to_string());
    }
    let timeout = Duration::from_millis(timeout_ms.max(200));
    // 解析首个可用地址（优先 IPv4）
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("解析 '{}' 失败: {}", host, e))?
        .collect::<Vec<_>>();
    let addr = addrs
        .iter()
        .find(|a| a.is_ipv4())
        .or_else(|| addrs.first())
        .ok_or_else(|| format!("'{}' 无可用地址", host))?;
    match TcpStream::connect_timeout(addr, timeout) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false), // 被拒/超时/不可达统一为"不通"
    }
}

/// 网站可达性探测（HTTP GET 状态码）
/// 成功返回 Some(状态码)；网络层失败返回 None
pub fn http_probe_status(url: &str, timeout_ms: u64) -> Option<u16> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(timeout_ms.max(500)))
        .build();
    let resp = agent.get(url).call();
    match resp {
        Ok(r) => Some(r.status()),
        Err(ureq::Error::Status(code, _)) => Some(code), // 4xx/5xx 也算可达
        Err(_) => None,
    }
}

/// 常见网络诊断目标（对齐源网站测试默认站点）
pub fn default_probe_sites() -> Vec<&'static str> {
    vec![
        "https://www.baidu.com",
        "https://www.qq.com",
        "https://www.microsoft.com",
        "https://github.com",
    ]
}

/// 端口探测服务默认目标
pub fn default_port_target() -> &'static str {
    "www.baidu.com"
}
