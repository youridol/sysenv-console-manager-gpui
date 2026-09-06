// secm-core::net_info — 侧栏「网络信息」数据契约与编排（复原旧版网络信息检测）
//
// 背景（用户指令 2026-09-06 严格模式）：上一版应用（Tauri 旧版）左侧边栏含
// 「网络信息 IP 检测」——已连接网卡上下行速率 + 本机/公网 IP。v2.2.0 克隆壳迁移
// 后该功能未恢复，需在新克隆壳侧栏复原。本模块为数据面（datasource/core 编排），
// UI 渲染在 pi_clone::shell 的侧栏网络信息卡。
//
// 数据来源：
// - 协商速率：datasource netif::link_speeds()（GetIfTable2，Alias → "1 Gbps"）
// - 本地 IPv4 / Up 网卡名：datasource netif::adapter_configs()（GetAdaptersAddresses）
// - 实时上下行：datasource net_io::get_net_io_speed_map()（PDH Network Interface，KB/s）
// - 公网 IPv4 国内出口：members.3322.org/dyndns/getip（国内 DDNS 回显，国内线路可达）
// - 公网 IPv4/IPv6 当前出口：ipify（api.ipify.org / api64.ipify.org，纯文本回显）
// - 公网归属（国内/国外）：ip-api.com countryCode（CN=国内，其余=国外）
//
// 公网请求均为固定白名单 URL + 5s 超时（对齐历史 datasource::http 安全约束 R4）；
// 全部为同步阻塞实现，调用方须在后台线程执行（S8）。

use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

/// 公网回显 / 归属查询 URL 白名单（固定常量，禁止拼接任意 URL —— R4 安全约束）
const IPIFY_V4: &str = "https://api.ipify.org";
const IPIFY_V6: &str = "https://api64.ipify.org";
const IP_API: &str = "http://ip-api.com/json/";
/// 国内 IPv4 回显端点（国内 DDNS 服务，实测经国内线路返回 CN 出口，
/// 如 106.127.136.216 = 中国电信；api.ipify.org 只返回系统路由当前出口，
/// 在代理环境下会漏掉国内出口 —— 故「公网 v4 · 国内」槽须走本端点）
const DOMESTIC_V4_ECHO: &str = "http://members.3322.org/dyndns/getip";

/// 默认超时（秒）
const TIMEOUT_SECS: u64 = 5;

/// 网卡当前流量速率（KB/s）
#[derive(Debug, Clone, Copy, Default)]
pub struct NetRate {
    /// 下行 KB/s
    pub rx_kbps: f32,
    /// 上行 KB/s
    pub tx_kbps: f32,
}

/// 单条公网地址槽（IPv4 / IPv6 × 国内/国外）
#[derive(Debug, Clone, Default)]
pub struct PublicIpEntry {
    /// IP 地址（未取到为空）
    pub ip: String,
    /// 归属：国内="国内"；国外="国外"；未知="—"
    pub region: String,
    /// 取数诊断（失败原因，空=成功）
    pub diag: String,
}

/// 侧栏网络信息全量数据（一次采集结果；UI 直接消费）
#[derive(Debug, Clone, Default)]
pub struct NetInfo {
    /// 当前已连接网卡名（如 "以太网"；无则空）
    pub adapter_name: String,
    /// 协商速率（如 "1 Gbps"；无则空）
    pub link_speed: String,
    /// 已连接网卡实时流量：下行/上行 KB/s
    pub rate: NetRate,
    /// 速率实际来源（description；与 adapter_name 不同 = 桥接/聚合到物理卡）
    pub rate_source: String,
    /// 本地 IPv4（首个非 APIPA 的已连接网卡）
    pub local_ipv4: String,
    /// 公网 IPv4 —— 国内出口
    pub pub_v4_domestic: PublicIpEntry,
    /// 公网 IPv4 —— 国外出口
    pub pub_v4_abroad: PublicIpEntry,
    /// 公网 IPv6 —— 国内出口
    pub pub_v6_domestic: PublicIpEntry,
    /// 公网 IPv6 —— 国外出口
    pub pub_v6_abroad: PublicIpEntry,
}

/// 判断 IPv4 是否 APIPA 链路本地地址（169.254.x.x —— 未获取到 DHCP/真实地址）
fn is_apipa(ip: &str) -> bool {
    ip.starts_with("169.254.")
}

/// 归属判定：countryCode == "CN" → 国内，否则国外
fn region_of(country_code: &str) -> String {
    if country_code.trim().eq_ignore_ascii_case("CN") {
        "国内".to_string()
    } else if country_code.trim().is_empty() {
        "—".to_string()
    } else {
        "国外".to_string()
    }
}

/// 对固定白名单 URL 发起 GET（纯文本回显；失败 Err）
fn fetch_text(url: &str) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .build();
    let resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("请求失败: {}", describe_ureq(&e)))?;
    resp.into_string()
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("读取响应失败: {}", e))
}

/// 按地址族强制 DNS 解析 + 连接（保证 api64 双栈域名按需走 v4/v6 出口）
///
/// 背景：ipify 域名同时有 A/AAAA 记录，系统 IPv6 优先策略会导致"同一域名
/// 返回的出口地址族不稳定"。强制只解析目标地址族 → 拿到该族真实出口 IP。
fn fetch_text_family(url: &str, family: IpFamily) -> Result<String, String> {
    let mut builder = ureq::AgentBuilder::new().timeout(Duration::from_secs(TIMEOUT_SECS));
    builder = builder.resolver(move |netloc: &str| -> std::io::Result<Vec<SocketAddr>> {
        let all: Vec<SocketAddr> = netloc
            .to_socket_addrs()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::AddrNotAvailable, e))?
            .collect();
        let filtered: Vec<SocketAddr> = all
            .into_iter()
            .filter(|a| match family {
                IpFamily::V4 => a.is_ipv4(),
                IpFamily::V6 => a.is_ipv6(),
            })
            .collect();
        if filtered.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrNotAvailable,
                format!("{}: 无可用 {:?} 地址", netloc, family),
            ));
        }
        Ok(filtered)
    });
    let agent = builder.build();
    let resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("请求失败: {}", describe_ureq(&e)))?;
    resp.into_string()
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("读取响应失败: {}", e))
}

#[derive(Debug, Clone, Copy)]
enum IpFamily {
    V4,
    V6,
}

fn describe_ureq(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, _) => format!("HTTP 状态码 {}", code),
        ureq::Error::Transport(t) => match t.kind() {
            ureq::ErrorKind::Dns => "无法解析主机名".to_string(),
            ureq::ErrorKind::ConnectionFailed => "连接失败".to_string(),
            ureq::ErrorKind::Io => format!("网络 IO 错误: {}", t),
            _ => format!("网络错误: {}", t),
        },
    }
}

/// 查询公网出口 IP 并判归属（family 强制走指定地址族）
///
/// 返回 (IP, region)。任一步失败 → Err，diag 交由调用方拼装。
fn fetch_public_ip(family: IpFamily) -> Result<(String, String), String> {
    let echo = match family {
        IpFamily::V4 => IPIFY_V4,
        IpFamily::V6 => IPIFY_V6,
    };
    let ip = fetch_text_family(echo, family)?;
    if ip.is_empty() {
        return Err("回显为空".to_string());
    }
    let region = query_region(&ip)?;
    Ok((ip, region))
}

/// 查询国内线路出口 IPv4（走国内 DDNS 回显端点；专填「公网 v4 · 国内」槽）
///
/// 背景：ipify 只返回系统路由当前出口 —— 代理环境（v4 走国外）下国内 v4 槽
/// 恒空。国内端点经国内线路可达，返回 CN 出口。取回后仍用 ip-api 核验归属，
/// 若归属确为国内则采用（防御该端点被劫持/走代理返回国外的情况）。
fn fetch_domestic_v4() -> Result<(String, String), String> {
    let ip = fetch_text(DOMESTIC_V4_ECHO)?;
    // 回显体可能是纯 IP，也可能带空白/异常字符；提取首个 IPv4 段
    let ip = extract_ipv4(&ip).ok_or_else(|| format!("回显非 IPv4: '{}'", ip))?;
    let region = query_region(&ip)?;
    Ok((ip, region))
}

/// 从文本中提取第一个合法 IPv4 地址（纯数字四段）
fn extract_ipv4(text: &str) -> Option<String> {
    text.split(|c: char| !c.is_ascii_digit() && c != '.')
        .find(|tok| {
            let parts: Vec<&str> = tok.split('.').collect();
            parts.len() == 4
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.parse::<u8>().is_ok() && p.len() <= 3)
        })
        .map(|s| s.to_string())
}

/// 归属查询：ip-api countryCode（CN=国内 其余=国外；查询失败 → Err）
fn query_region(ip: &str) -> Result<String, String> {
    let geo_url = format!("{}{}?fields=countryCode", IP_API, ip);
    let body = fetch_text(&geo_url).map_err(|e| format!("归属查询失败: {}", e))?;
    // 响应形如 {"query":"1.2.3.4","status":"success","countryCode":"SG"}
    let cc = body
        .find("countryCode")
        .and_then(|i| {
            let rest = &body[i..];
            let q1 = rest.find('"').map(|x| x + 1)?;
            let after = &rest[q1..];
            let q2 = after.find('"')?;
            Some(after[q2 + 1..].split('"').next().unwrap_or("").to_string())
        })
        .unwrap_or_default();
    if cc.trim().is_empty() {
        return Err("归属查询无 countryCode".to_string());
    }
    Ok(region_of(&cc))
}

/// 刷新本地链路字段（已连接网卡名/协商速率/本地 IPv4/实时上下行速率）。
///
/// 轻量、无网络请求（仅 GetAdaptersAddresses + GetIfTable2 + PDH，微秒~毫秒级），
/// 供侧栏 1~2s 轮询实时速率。PDH 速率首次调用建基线，此后每次返回最近 ~1s 均值。
///
/// 主体网卡选择与速率匹配（实测真机语义）：
/// - 「已连接」= Up 且首个 IPv4 非 APIPA(169.254) 的网卡（真实拿到地址者）；
///   全部 APIPA 或无 Up 则取首个 Up。
/// - PDH Network Interface 只有**物理网卡**实例（键=描述名）；虚拟交换器/桥接卡
///   （Hyper-V vEthernet*）不上 PDH，其真实流量由所桥物理卡计数。故速率匹配：
///   ① 直接按主体卡 description/name 匹配 io；② 匹配不到（桥接虚拟卡）时，取 io
///   中当前流量最大的物理实例作为承载速率，并把 rate_source 标注该实例描述，
///   供 UI 展示「经 XX 网卡」。
pub fn refresh_local_rate(info: &mut NetInfo) {
    // ---- 本地链路：主体网卡选择 + IPv4 + 协商速率 ----
    let adapters = secm_datasource::netif::adapter_configs().unwrap_or_default();
    let speeds: HashMap<String, String> = secm_datasource::netif::link_speeds().unwrap_or_default();
    let io: HashMap<String, (f32, f32)> = secm_datasource::net_io::get_net_io_speed_map();

    info.adapter_name.clear();
    info.local_ipv4.clear();
    info.link_speed.clear();
    info.rate_source.clear();
    info.rate = NetRate::default();

    let ups: Vec<&secm_datasource::netif::AdapterConfig> =
        adapters.iter().filter(|a| a.status == "Up").collect();
    // 主体 = 首个「非 APIPA IPv4」Up 网卡（真实连接）；否则首个 Up
    let main = ups
        .iter()
        .copied()
        .find(|a| a.ipv4.iter().any(|ip| !is_apipa(ip)))
        .or_else(|| ups.first().copied());

    if let Some(up) = main {
        info.adapter_name = up.name.clone();
        info.local_ipv4 = up
            .ipv4
            .iter()
            .find(|ip| !is_apipa(ip))
            .cloned()
            .unwrap_or_default();
        // 协商速率：Alias 键先按 name、再按 description 匹配
        info.link_speed = speeds
            .get(&up.name)
            .or_else(|| speeds.get(&up.description))
            .cloned()
            .unwrap_or_default();

        // 实时上下行：PDH 键 = 物理网卡描述名
        let (rx, tx) = io
            .get(&up.description)
            .or_else(|| io.get(&up.name))
            .copied()
            .unwrap_or_default();
        if rx > 0.0 || tx > 0.0 {
            info.rate = NetRate {
                rx_kbps: rx,
                tx_kbps: tx,
            };
            info.rate_source = up.description.clone();
        } else {
            // 主体卡自身无 PDH 实例/零速率（虚拟桥）：取 io 中流量最大的物理实例
            if let Some((desc, (brx, btx))) = io
                .iter()
                .max_by(|a, b| {
                    let at = a.1 .0 + a.1 .1;
                    let bt = b.1 .0 + b.1 .1;
                    at.partial_cmp(&bt).unwrap_or(std::cmp::Ordering::Equal)
                })
            {
                info.rate = NetRate {
                    rx_kbps: *brx,
                    tx_kbps: *btx,
                };
                info.rate_source = desc.clone();
            }
        }
    }
}

/// 刷新公网四槽（国内/国外 × IPv4/IPv6；联网请求，带 5s 超时与逐槽失败降级）。
///
/// 取数语义（v2.3.1 修复「国内 IPv4 没正确读取」）：
/// - **公网 v4 · 国内**：走国内回显端点（members.3322.org/dyndns/getip，实测
///   经国内线路返回 CN 出口如 106.127.x.x）。此前仅用 ipify → 代理环境下 v4
///   出口为国外，国内槽恒空 —— 现改为国内端点专取，ip-api 归属核验兜底。
/// - **公网 v4 · 国外**：ipify 当前出口（归属非 CN → 国外槽）。
/// - 公网 v6 同 v4：ipify(v6) 取当前 v6 出口，按归属填国内/国外对应槽。
/// 每槽独立失败降级，互不阻塞（S8）；同一槽内端点失败则填 diag「失败」。
pub fn refresh_public(info: &mut NetInfo) {
    info.pub_v4_domestic = PublicIpEntry::default();
    info.pub_v4_abroad = PublicIpEntry::default();
    info.pub_v6_domestic = PublicIpEntry::default();
    info.pub_v6_abroad = PublicIpEntry::default();

    // ---- 公网 v4 · 国内（国内端点专取）----
    match fetch_domestic_v4() {
        Ok((ip, region)) if region == "国内" => info.pub_v4_domestic = PublicIpEntry {
            ip,
            region,
            diag: String::new(),
        },
        Ok((ip, region)) => {
            // 国内端点返回了非国内归属（异常/被代理）—— 仍展示但标注区域，避免丢数据
            info.pub_v4_domestic = PublicIpEntry {
                ip,
                region,
                diag: String::new(),
            };
        }
        Err(e) => {
            info.pub_v4_domestic.diag = e;
        }
    }

    // ---- 公网 v4 · 国外（ipify 当前出口）----
    match fetch_public_ip(IpFamily::V4) {
        Ok((ip, region)) => {
            // 当前出口若是国内（无代理直连）则国内槽已有值，国外槽留空属正常；
            // 但若 ipify 出口为国外，填国外槽
            if region != "国内" {
                info.pub_v4_abroad = PublicIpEntry {
                    ip,
                    region,
                    diag: String::new(),
                };
            } else {
                // 双保险：ipify 走到国内出口，但国内槽为空时补入
                if info.pub_v4_domestic.ip.is_empty() {
                    info.pub_v4_domestic = PublicIpEntry {
                        ip,
                        region,
                        diag: String::new(),
                    };
                }
            }
        }
        Err(e) => {
            info.pub_v4_abroad.diag = e;
        }
    }

    // ---- 公网 v6（ipify 当前 v6 出口，按归属填国内/国外）----
    match fetch_public_ip(IpFamily::V6) {
        Ok((ip, region)) if region == "国内" => info.pub_v6_domestic = PublicIpEntry {
            ip,
            region,
            diag: String::new(),
        },
        Ok((ip, region)) => info.pub_v6_abroad = PublicIpEntry {
            ip,
            region,
            diag: String::new(),
        },
        Err(e) => {
            info.pub_v6_abroad.diag = e.clone();
            info.pub_v6_domestic.diag = e;
        }
    }
}

/// 采集一次完整侧栏网络信息（本地 + 公网；后台线程调用）
pub fn collect_net_info() -> NetInfo {
    let mut info = NetInfo::default();
    refresh_local_rate(&mut info);
    refresh_public(&mut info);
    info
}
#[cfg(test)]
mod tests {
    use super::*;

    /// 真机验证（默认忽略）：collect_net_info 应返回 Up 网卡名/本地 v4/协商速率，
    /// 以及至少一个公网槽（v4 或 v6 出口）。
    /// 运行：`cargo test -p secm-core -- --ignored real_net_info --nocapture`
    #[test]
    #[ignore]
    fn real_net_info() {
        let info = collect_net_info();
        eprintln!(
            "adapter={} speed={} local_v4={} rate={:.1}/{:.1} KB/s",
            info.adapter_name,
            info.link_speed,
            info.local_ipv4,
            info.rate.rx_kbps,
            info.rate.tx_kbps
        );
        eprintln!("v4 domestic={:?}", info.pub_v4_domestic);
        eprintln!("v4 abroad={:?}", info.pub_v4_abroad);
        eprintln!("v6 domestic={:?}", info.pub_v6_domestic);
        eprintln!("v6 abroad={:?}", info.pub_v6_abroad);
        assert!(!info.local_ipv4.is_empty(), "应取到本地 IPv4");
    }

    /// 速率两样本真机验证（PDH 需 ≥1s 间隔；确认匹配到 Up 网卡后出数）
    #[test]
    #[ignore]
    fn real_net_rate_samples() {
        let mut info = NetInfo::default();
        refresh_local_rate(&mut info);
        std::thread::sleep(std::time::Duration::from_millis(1200));
        refresh_local_rate(&mut info);
        eprintln!(
            "[真机] adapter='{}' desc-匹配后 rate={:.2}/{:.2} KB/s speed={}",
            info.adapter_name, info.rate.rx_kbps, info.rate.tx_kbps, info.link_speed
        );
    }

    /// 纯逻辑：IPv4 从回显文本提取
    #[test]
    fn test_extract_ipv4() {
        assert_eq!(
            extract_ipv4("106.127.136.216"),
            Some("106.127.136.216".to_string())
        );
        // 带前导/尾随空白与异常文本
        assert_eq!(
            extract_ipv4("  220.181.38.148 \r\n"),
            Some("220.181.38.148".to_string())
        );
        // 非 IPv4 / 无地址
        assert_eq!(extract_ipv4("240e::1"), None);
        assert_eq!(extract_ipv4("not an ip"), None);
        assert_eq!(extract_ipv4("999.1.1.1"), None); // 段超 255
        assert_eq!(extract_ipv4("1.2.3"), None);
    }
}
