// net_diag::ping — 流式 Ping（ICMP/TCP/UDP，次数/间隔/包大小/TTL/截止/连续/取消）
//
// 行为对齐原版 ping_streaming（ADR-0001 §3/§4）：
// - sequence 从 0 起；每包事件 kind="ping"；结束 kind="summary"（PingSummary JSON）
// - 取消 → kind="error" data="用户取消" 后 break（仍发 summary）
// - 截止时间到 → kind="info" data="截止时间到达，测试结束" 后 break（仍发 summary）
// - 单包超时 2000ms（surge-ping 0.8.4 默认值，实测确认）
// 增强点：
// - TTL/size 真实生效（原版 _ttl/_proto 被静默忽略）
// - proto=tcp/udp 为真实现（TCP connect_timeout / UDP 发包+错误判定，端口取参数 port）

use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

use super::cancel::is_cancelled;
use super::icmp;
use super::StreamEvent;

/// 单包探测超时（ms，与原版 surge-ping 默认值一致）
const PROBE_TIMEOUT_MS: u32 = 2000;

/// Ping 参数（字段与原版命令入参一一对应；port 仅 tcp/udp proto 使用）
#[derive(Debug, Clone)]
pub struct PingParams {
    pub target: String,
    pub count: u32,
    pub interval_ms: u64,
    pub continuous: bool,
    /// "auto" | "v4" | "v6"
    pub ip_version: String,
    /// "icmp" | "tcp" | "udp"
    pub proto: String,
    pub size: u32,
    pub ttl: u32,
    /// 0 = 不限
    pub deadline_secs: u32,
    /// TCP/UDP 探测端口（icmp 忽略）
    pub port: u16,
}

/// 单包结果（契约与原版一致）
#[derive(Debug, Clone, serde::Serialize)]
pub struct PingResult {
    pub sequence: u32,
    pub rtt_ms: f64,
    pub success: bool,
}

/// 汇总（契约与原版一致）
#[derive(Debug, Clone, serde::Serialize)]
pub struct PingSummary {
    pub target: String,
    pub sent: u32,
    pub received: u32,
    pub loss_percent: f64,
    pub avg_rtt_ms: f64,
    pub min_rtt_ms: f64,
    pub max_rtt_ms: f64,
    pub results: Vec<PingResult>,
}

/// 系统解析目标并按版本过滤（自 IP 直校验；域名走系统解析器）
pub fn resolve_target(target: &str, ip_version: &str) -> Result<IpAddr, String> {
    if let Ok(ip) = target.parse::<IpAddr>() {
        return match ip_version {
            "v4" if !ip.is_ipv4() => Err("目标地址不是 IPv4 地址".to_string()),
            "v6" if !ip.is_ipv6() => Err("目标地址不是 IPv6 地址".to_string()),
            _ => Ok(ip),
        };
    }
    let addrs: Vec<SocketAddr> = format!("{}:0", target)
        .to_socket_addrs()
        .map_err(|e| format!("DNS 解析失败 {}: {}", target, e))?
        .collect();
    match ip_version {
        "v4" => addrs
            .iter()
            .find(|a| a.is_ipv4())
            .map(|a| a.ip())
            .ok_or_else(|| format!("未找到 {} 的 IPv4 地址", target)),
        "v6" => addrs
            .iter()
            .find(|a| a.is_ipv6())
            .map(|a| a.ip())
            .ok_or_else(|| format!("未找到 {} 的 IPv6 地址", target)),
        _ => addrs
            .first()
            .map(|a| a.ip())
            .ok_or_else(|| format!("目标地址无法解析: {}", target)),
    }
}

/// ICMP 单包探测（TTL/size 真实生效）
fn probe_icmp(addr: IpAddr, size: usize, ttl: u8) -> (bool, f64) {
    let r = icmp::ping_once(addr, size, ttl, PROBE_TIMEOUT_MS);
    if r.is_success() {
        // API 报告的 RTT 为整数毫秒；为 0 时回退本地计时（亚毫秒精度）
        let rtt = if r.rtt_ms > 0 { r.rtt_ms as f64 } else { 0.0 };
        (true, rtt)
    } else {
        (false, 0.0)
    }
}

/// TCP 单包探测（connect_timeout，成功=端口可建立连接）
fn probe_tcp(addr: IpAddr, port: u16) -> (bool, f64) {
    let sa = SocketAddr::new(addr, port);
    let start = Instant::now();
    match TcpStream::connect_timeout(&sa, Duration::from_millis(PROBE_TIMEOUT_MS as u64)) {
        Ok(stream) => {
            drop(stream);
            (true, start.elapsed().as_secs_f64() * 1000.0)
        }
        Err(_) => (false, 0.0),
    }
}

/// UDP 单包探测：发 1 字节后等响应/端口不可达回执
/// - 收到响应 → 可达（服务应答）
/// - 收到 WSAECONNRESET（端口不可达 ICMP 回执）→ 主机可达（端口未开放）
/// - 超时 → 判定不可达（可能被过滤）
fn probe_udp(addr: IpAddr, port: u16) -> (bool, f64) {
    let bind: &str = if addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let Ok(sock) = UdpSocket::bind(bind) else {
        return (false, 0.0);
    };
    let sa = SocketAddr::new(addr, port);
    if sock.connect(sa).is_err() {
        return (false, 0.0);
    }
    let start = Instant::now();
    if sock.send(&[0u8]).is_err() {
        return (false, 0.0);
    }
    let _ = sock.set_read_timeout(Some(Duration::from_millis(PROBE_TIMEOUT_MS as u64)));
    let mut buf = [0u8; 64];
    match sock.recv_from(&mut buf) {
        Ok(_) => (true, start.elapsed().as_secs_f64() * 1000.0),
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
            // Windows：对端端口不可达（ICMP Port Unreachable）→ 主机在线
            (true, start.elapsed().as_secs_f64() * 1000.0)
        }
        Err(_) => (false, 0.0),
    }
}

/// 流式 Ping 主流程（阻塞；在后台线程执行）
pub fn ping_streaming(
    p: &PingParams,
    cmd_id: &str,
    emit: &dyn Fn(StreamEvent),
) -> Result<PingSummary, String> {
    // 参数钳制（与原版一致）
    let count = if p.continuous {
        u32::MAX
    } else {
        p.count.clamp(1, 65535)
    };
    let interval_ms = p.interval_ms.clamp(10, 60000);
    let payload_size = p.size.clamp(32, 65507) as usize;
    let ttl = p.ttl.clamp(1, 255) as u8;
    let addr = resolve_target(&p.target, &p.ip_version)?;
    let deadline = if p.deadline_secs > 0 {
        Some(Instant::now() + Duration::from_secs(p.deadline_secs as u64))
    } else {
        None
    };

    let proto = p.proto.to_lowercase();
    let mut results: Vec<PingResult> = Vec::new();

    for seq in 0..count {
        if is_cancelled(cmd_id) {
            emit(StreamEvent::text("error", "用户取消"));
            break;
        }
        if let Some(dl) = deadline {
            if Instant::now() >= dl {
                emit(StreamEvent::text("info", "截止时间到达，测试结束"));
                break;
            }
        }

        let start = Instant::now();
        let (success, mut rtt) = match proto.as_str() {
            "tcp" => probe_tcp(addr, p.port),
            "udp" => probe_udp(addr, p.port),
            _ => probe_icmp(addr, payload_size, ttl),
        };
        if success && rtt == 0.0 {
            // ICMP API 亚毫秒 RTT 报 0 时回退本地耗时
            rtt = start.elapsed().as_secs_f64() * 1000.0;
        }
        let r = PingResult {
            sequence: seq,
            rtt_ms: rtt,
            success,
        };

        emit(StreamEvent::json("ping", &r));
        results.push(r);

        if seq < count.saturating_sub(1) && !is_cancelled(cmd_id) {
            std::thread::sleep(Duration::from_millis(interval_ms));
        }
    }

    let sent = results.len() as u32;
    let received = results.iter().filter(|r| r.success).count() as u32;
    let loss_percent = if sent > 0 {
        ((sent - received) as f64 / sent as f64) * 100.0
    } else {
        100.0
    };
    let rtts: Vec<f64> = results
        .iter()
        .filter(|r| r.success)
        .map(|r| r.rtt_ms)
        .collect();
    let avg_rtt = if rtts.is_empty() {
        0.0
    } else {
        rtts.iter().sum::<f64>() / rtts.len() as f64
    };
    let min_rtt = rtts.iter().cloned().fold(f64::MAX, f64::min);
    let max_rtt = rtts.iter().cloned().fold(0.0, f64::max);

    let summary = PingSummary {
        target: p.target.clone(),
        sent,
        received,
        loss_percent,
        avg_rtt_ms: avg_rtt,
        min_rtt_ms: if min_rtt == f64::MAX { 0.0 } else { min_rtt },
        max_rtt_ms: max_rtt,
        results,
    };

    emit(StreamEvent::json("summary", &summary));
    Ok(summary)
}
