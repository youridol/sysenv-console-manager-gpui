// net_diag::traceroute — 流式 Traceroute（TTL 递增，每跳 3 探针，最多 64 跳）
//
// 行为对齐原版 traceroute_streaming（ADR-0001 §3/§4）：
// - max_hops 钳制 1..=64；每跳 3 探针；跳超时 1200ms；命中目标即止
// - 每跳事件 kind="trace-hop"（ip 无响应="*"）；结束 kind="summary"（TraceResult）
// - 取消 → kind="error" data="用户取消" 后 break（仍发 summary）
// - 单跳出错 → kind="info" data="跳点 N: <错误>"，空跳行继续
// - 指定 DNS 服务器：UDP DNS 查询（自研 RFC 1035 客户端；2s 超时 1 次尝试）
// 实现差异（ADR-0002 §1）：探针改用系统态 ICMP（IcmpSendEcho/Icmp6SendEcho2 + TTL 选项），
// Time Exceeded 的路由器地址由系统给出；**非管理员可用**（原版 raw socket 需管理员）。
// 3 探针并发执行（std::thread::scope），单跳墙钟与原版一致（≤1200ms+调度余量）。

use std::net::IpAddr;

use super::cancel::is_cancelled;
use super::dns_client;
use super::icmp::{self, IcmpReply};
use super::ping::resolve_target;
use super::StreamEvent;

/// 每跳探针数（与原版一致）
const PROBE_COUNT: usize = 3;
/// 单跳超时（与原版一致）
const HOP_TIMEOUT_MS: u32 = 1200;
/// 探针载荷（与原版一致：32 字节）
const PROBE_PAYLOAD: usize = 32;

/// 单跳结果（契约与原版一致）
#[derive(Debug, Clone, serde::Serialize)]
pub struct TraceHop {
    pub hop: u8,
    pub ip: String,
    pub rtt_avg_ms: f64,
    pub rtt_min_ms: f64,
    pub rtt_max_ms: f64,
    pub probes_answered: u8,
    pub probes_sent: u8,
}

/// 汇总（契约与原版一致）
#[derive(Debug, Clone, serde::Serialize)]
pub struct TraceResult {
    pub target: String,
    pub max_hops: u8,
    pub hops: Vec<TraceHop>,
}

/// 单次 TTL 探针（独立线程执行；返回该探针回复）
fn probe_with_ttl(addr: IpAddr, ttl: u8) -> IcmpReply {
    icmp::ping_once(addr, PROBE_PAYLOAD, ttl, HOP_TIMEOUT_MS)
}

/// 执行一跳：3 探针并发，返回 (成功探针 RTT 列表, 首个响应者, 是否到达目标)
fn probe_hop(addr: IpAddr, ttl: u8) -> (Vec<f64>, Option<IpAddr>, bool) {
    let target_str = addr.to_string();
    let rtts: Vec<IcmpReply> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..PROBE_COUNT)
            .map(|_| s.spawn(move || probe_with_ttl(addr, ttl)))
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    let mut rtts_ok: Vec<f64> = Vec::new();
    let mut first_responder: Option<IpAddr> = None;
    let mut got_target = false;
    for r in &rtts {
        // Echo Reply（到达目标）与 Time Exceeded（中间跳）均计入 RTT（原版语义一致）
        if (r.is_success() || r.is_ttl_expired()) && r.responder.is_some() {
            let resp = r.responder.expect("is_some 已判定");
            if first_responder.is_none() {
                first_responder = Some(resp);
            }
            if resp.to_string() == target_str {
                got_target = true;
            }
            rtts_ok.push(if r.rtt_ms > 0 { r.rtt_ms as f64 } else { 0.0 });
        }
    }
    (rtts_ok, first_responder, got_target)
}

/// 流式 Traceroute 主流程（阻塞；在后台线程执行）
pub fn trace_streaming(
    target: &str,
    max_hops: u8,
    cmd_id: &str,
    ip_version: &str,
    dns_server: Option<&str>,
    emit: &dyn Fn(StreamEvent),
) -> Result<TraceResult, String> {
    // 解析目标：指定 DNS 服务器时走自研 UDP DNS；否则系统解析
    let addr: IpAddr = match dns_server {
        None | Some("") | Some("system") => resolve_target(target, ip_version)?,
        Some(srv) => {
            let server: IpAddr = srv
                .parse()
                .map_err(|_| format!("DNS 服务器地址无效: {}", srv))?;
            let mut v4_err = None;
            let mut v6_err = None;
            let resolved: Option<IpAddr> = if ip_version != "v6" {
                match dns_client::query(server, target, dns_client::QTYPE_A) {
                    Ok(ip) => Some(ip),
                    Err(e) => {
                        v4_err = Some(e);
                        None
                    }
                }
            } else {
                None
            };
            let resolved = resolved.or(if ip_version != "v4" {
                match dns_client::query(server, target, dns_client::QTYPE_AAAA) {
                    Ok(ip) => Some(ip),
                    Err(e) => {
                        v6_err = Some(e);
                        None
                    }
                }
            } else {
                None
            });
            match (resolved, ip_version) {
                (Some(ip), _) => ip,
                (None, "v4") => {
                    return Err(format!(
                        "通过 DNS {} 查询 {} 的 IPv4 记录失败: {}",
                        srv,
                        target,
                        v4_err.unwrap_or_else(|| "无记录".to_string())
                    ))
                }
                (None, "v6") => {
                    return Err(format!(
                        "通过 DNS {} 查询 {} 的 IPv6 记录失败: {}",
                        srv,
                        target,
                        v6_err.unwrap_or_else(|| "无记录".to_string())
                    ))
                }
                (None, _) => {
                    return Err(format!(
                        "通过 DNS {} 查询 {} 失败: {}",
                        srv,
                        target,
                        v4_err.or(v6_err).unwrap_or_else(|| "无记录".to_string())
                    ))
                }
            }
        }
    };
    let max_hops = max_hops.clamp(1, 64);
    let mut hops: Vec<TraceHop> = Vec::new();

    for ttl in 1..=max_hops {
        if is_cancelled(cmd_id) {
            emit(StreamEvent::text("error", "用户取消"));
            break;
        }

        let (rtts, responder, got_target) = probe_hop(addr, ttl);
        let hop_ip = responder
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "*".to_string());
        let probes_answered = rtts.len() as u8;
        let avg = if rtts.is_empty() {
            0.0
        } else {
            rtts.iter().sum::<f64>() / rtts.len() as f64
        };
        let min = rtts.iter().cloned().fold(f64::MAX, f64::min);
        let max = rtts.iter().cloned().fold(0.0, f64::max);

        let hop = TraceHop {
            hop: ttl,
            ip: hop_ip,
            rtt_avg_ms: avg,
            rtt_min_ms: if min == f64::MAX { 0.0 } else { min },
            rtt_max_ms: max,
            probes_answered,
            probes_sent: PROBE_COUNT as u8,
        };
        emit(StreamEvent::json("trace-hop", &hop));
        hops.push(hop);

        if got_target {
            break;
        }
    }

    let result = TraceResult {
        target: target.to_string(),
        max_hops,
        hops,
    };
    emit(StreamEvent::json("summary", &result));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 实机：网关 traceroute（1 跳确定可达，验证 TTL/Time Exceeded 链路）
    #[test]
    #[ignore = "实机网络用例：cargo test -- --ignored 运行"]
    fn real_traceroute_gateway() {
        let gw = crate::net_diag::dhcp_probe::read_network_config()
            .gateway
            .expect("本机应有默认网关");
        let gw_str = gw.to_string();
        let r = trace_streaming(&gw_str, 4, "test-real-trace-gw", "auto", None, &|_| {})
            .expect("traceroute 应成功");
        assert!(!r.hops.is_empty(), "应至少有一跳");
        let last = r.hops.last().expect("非空");
        assert_eq!(
            last.ip, gw_str,
            "网关应在第一跳直接命中（实际 {}）",
            last.ip
        );
        assert!(last.probes_answered > 0, "网关跳应有探针响应: {:?}", last);
    }

    /// 实机：公网 traceroute 完成性（跳数随拓扑浮动，只断言链路可用）
    #[test]
    #[ignore = "实机网络用例：cargo test -- --ignored 运行"]
    fn real_traceroute_public() {
        let r = trace_streaming("223.5.5.5", 30, "test-real-trace", "auto", None, &|_| {})
            .expect("traceroute 应成功");
        assert!(!r.hops.is_empty(), "应至少有一跳");
        let responded = r.hops.iter().filter(|h| h.ip != "*").count();
        assert!(responded > 0, "至少一跳应响应");
        assert!(
            r.hops.len() <= 30,
            "跳数不应超过上限（实际 {}）",
            r.hops.len()
        );
    }

    /// 实机：指定 DNS 服务器解析目标后 traceroute
    #[test]
    #[ignore = "实机网络用例：cargo test -- --ignored 运行"]
    fn real_traceroute_with_dns() {
        let r = trace_streaming(
            "www.baidu.com",
            6,
            "test-real-trace-dns",
            "auto",
            Some("119.29.29.29"),
            &|_| {},
        )
        .expect("指定 DNS 的 traceroute 应成功");
        assert!(!r.hops.is_empty(), "应至少有一跳");
    }
}
