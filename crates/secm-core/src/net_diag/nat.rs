// net_diag::nat — STUN NAT 类型检测（RFC 3489 经典三阶段，NAT0–NAT4 分类）
//
// 逐行移植原版 detect_nat_streaming 算法（ADR-0001 §3 契约不变）：
// - Phase 1：Binding Request（0x0001，magic 0x2112A442，12B 随机事务 ID）→
//   解析 MAPPED-ADDRESS（0x0001）/ XOR-MAPPED-ADDRESS（0x0020）→ 映射后公网地址
// - Phase 2：新 socket 二次 Binding → 比较映射端口是否变化（对称 NAT 判定）
// - Phase 3：Change-Request（0x0003，值 0x0002|0x0004 = 变更 IP+Port）→
//   无响应 = NAT/防火墙过滤外部回包
// - 分类：映射 IP == 本机 IP → NAT0（公网/无 NAT）；
//   !port_changed && !filtered → NAT1（全锥）；!port_changed && filtered → NAT2（受限锥）；
//   port_changed && filtered → NAT4（对称）；其余 → NAT3（端口受限锥）
// - 每阶段事件 kind="stun-step"（phase/description），结束 kind="summary"
// 增强点：取消检查（读循环 300ms 唤醒）；超时语义保持 3s/阶段。

use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

use rand::Rng;

use super::cancel::is_cancelled;
use super::StreamEvent;

/// STUN magic cookie（RFC 3489/5380）
const MAGIC_COOKIE: u32 = 0x2112A442;
/// 阶段读超时（与原版一致）
const PHASE_TIMEOUT_MS: u64 = 3000;
/// 读循环唤醒间隔（取消检查粒度）
const RECV_SLICE_MS: u64 = 300;

/// NAT 类型枚举（serde 名与原版一致）
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub enum NatType {
    #[serde(rename = "NAT0")]
    Nat0,
    #[serde(rename = "NAT1")]
    Nat1,
    #[serde(rename = "NAT2")]
    Nat2,
    #[serde(rename = "NAT3")]
    Nat3,
    #[serde(rename = "NAT4")]
    Nat4,
}

impl std::fmt::Display for NatType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NatType::Nat0 => write!(f, "NAT0 — 公网 / 无 NAT"),
            NatType::Nat1 => write!(f, "NAT1 — 全锥形 NAT (Full Cone)"),
            NatType::Nat2 => write!(f, "NAT2 — 受限锥形 NAT (Restricted Cone)"),
            NatType::Nat3 => write!(f, "NAT3 — 端口受限锥形 NAT (Port Restricted Cone)"),
            NatType::Nat4 => write!(f, "NAT4 — 对称 NAT (Symmetric)"),
        }
    }
}

/// 单阶段步骤事件负载
#[derive(Debug, Clone, serde::Serialize)]
pub struct StunTestStep {
    pub phase: String,
    pub description: String,
}

/// 检测结果（契约与原版一致）
#[derive(Debug, Clone, serde::Serialize)]
pub struct NatDetectionResult {
    pub nat_type: NatType,
    pub nat_label: String,
    pub public_ip: String,
    pub public_port: u16,
    pub local_ip: String,
    pub steps: Vec<StunTestStep>,
    pub success: bool,
    pub error_message: String,
}

/// 预设 STUN 服务器（15 台，与原版一致：显示名, host, port）
pub const STUN_SERVERS: &[(&str, &str, u16)] = &[
    ("stun.qq.com", "stun.qq.com", 3478),
    ("Google Primary", "stun.l.google.com", 19302),
    ("Google Alt", "stun1.l.google.com", 19302),
    ("Google v6", "stun2.l.google.com", 19302),
    ("Freeswitch", "stun.freeswitch.org", 3478),
    ("Twilio", "global.stun.twilio.com", 3478),
    ("Numb", "stun.counterpath.com", 3478),
    ("Sipgate", "stun.sipgate.net", 3478),
    ("Ekiga", "stun.ekiga.net", 3478),
    ("3CX", "stun.3cx.com", 3478),
    ("Blink", "stun.blink.de", 3478),
    ("Vivox", "stun.vivox.com", 3478),
    ("Nextcloud", "stun.nextcloud.com", 3478),
    ("Linphone", "stun.linphone.org", 3478),
    ("Miwifi", "stun.miwifi.com", 3478),
];

/// 预设服务器 host:port 列表
pub fn get_stun_servers() -> Vec<String> {
    STUN_SERVERS
        .iter()
        .map(|s| format!("{}:{}", s.1, s.2))
        .collect()
}

/// 构造 Binding Request（RFC 3489 §8.1；magic cookie 占位事务 ID 前缀兼容 RFC 5380 服务器）
pub fn build_binding_request(transaction_id: &[u8; 12]) -> Vec<u8> {
    let mut pkt = vec![0u8; 20];
    pkt[0..2].copy_from_slice(&0x0001u16.to_be_bytes()); // 类型：Binding Request
    pkt[2..4].copy_from_slice(&0u16.to_be_bytes()); // 消息长度
    pkt[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
    pkt[8..20].copy_from_slice(transaction_id);
    pkt
}

/// 构造带 CHANGE-REQUEST 属性的 Binding Request
pub fn build_change_request(tid: &[u8; 12], change_ip: bool, change_port: bool) -> Vec<u8> {
    let mut value: u32 = 0;
    if change_ip {
        value |= 0x0002;
    }
    if change_port {
        value |= 0x0004;
    }
    let mut pkt = vec![0u8; 28];
    pkt[0..2].copy_from_slice(&0x0001u16.to_be_bytes());
    pkt[2..4].copy_from_slice(&8u16.to_be_bytes()); // 消息长度（1 个属性）
    pkt[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
    pkt[8..20].copy_from_slice(tid);
    pkt[20..22].copy_from_slice(&0x0003u16.to_be_bytes()); // CHANGE-REQUEST
    pkt[22..24].copy_from_slice(&4u16.to_be_bytes()); // 属性长度
    pkt[24..28].copy_from_slice(&value.to_be_bytes());
    pkt
}

/// 从 STUN 响应解析映射地址（MAPPED-ADDRESS 0x0001 / XOR-MAPPED-ADDRESS 0x0020，IPv4）
pub fn parse_mapped_address(response: &[u8]) -> Option<(IpAddr, u16)> {
    if response.len() < 20 {
        return None;
    }
    let msg_len = u16::from_be_bytes([response[2], response[3]]) as usize;
    if response.len() < 20 + msg_len {
        return None;
    }
    let mut pos = 20;
    while pos + 4 <= 20 + msg_len {
        let attr_type = u16::from_be_bytes([response[pos], response[pos + 1]]);
        let attr_len = u16::from_be_bytes([response[pos + 2], response[pos + 3]]) as usize;
        if attr_type == 0x0001 && pos + 4 + attr_len <= response.len() && attr_len >= 8 {
            let family = response[pos + 5];
            let port = u16::from_be_bytes([response[pos + 6], response[pos + 7]]);
            if family == 0x01 {
                return Some((
                    IpAddr::from([
                        response[pos + 8],
                        response[pos + 9],
                        response[pos + 10],
                        response[pos + 11],
                    ]),
                    port,
                ));
            }
        } else if attr_type == 0x0020 && pos + 4 + attr_len <= response.len() && attr_len >= 8 {
            let family = response[pos + 5];
            let xport = u16::from_be_bytes([response[pos + 6], response[pos + 7]]);
            let port = xport ^ 0x2112;
            if family == 0x01 {
                let magic = &response[4..8];
                return Some((
                    IpAddr::from([
                        response[pos + 8] ^ magic[0],
                        response[pos + 9] ^ magic[1],
                        response[pos + 10] ^ magic[2],
                        response[pos + 11] ^ magic[3],
                    ]),
                    port,
                ));
            }
        }
        pos += 4 + attr_len;
        if attr_len % 4 != 0 {
            pos += 4 - (attr_len % 4);
        }
    }
    None
}

/// 解析 STUN 服务器地址（host:port；缺省端口 3478）
fn resolve_stun(server: &str) -> Result<SocketAddr, String> {
    let (host, port) = if let Some((h, p)) = server.rsplit_once(':') {
        (h, p.parse::<u16>().unwrap_or(3478))
    } else {
        (server, 3478)
    };
    format!("{}:{}", host, port)
        .to_socket_addrs()
        .map_err(|e| format!("STUN DNS: {}", e))?
        .next()
        .ok_or_else(|| format!("No addr for {}", server))
}

/// 本机出口 IP（UDP connect 8.8.8.8:53 由协议栈选路；失败回退 127.0.0.1）
fn get_local_ip() -> IpAddr {
    if let Ok(s) = UdpSocket::bind("0.0.0.0:0") {
        let _ = s.connect("8.8.8.8:53");
        if let Ok(addr) = s.local_addr() {
            return addr.ip();
        }
    }
    IpAddr::from([127, 0, 0, 1])
}

/// 带取消检查的收包（总超时 3s，300ms 切片唤醒）
fn recv_with_cancel(sock: &UdpSocket, cmd_id: &str, buf: &mut [u8]) -> Option<(usize, SocketAddr)> {
    let deadline = Instant::now() + Duration::from_millis(PHASE_TIMEOUT_MS);
    loop {
        if is_cancelled(cmd_id) {
            return None;
        }
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        let _ = sock.set_read_timeout(Some(Duration::from_millis(RECV_SLICE_MS)));
        match sock.recv_from(buf) {
            Ok((len, src)) => return Some((len, src)),
            Err(_) => continue, // 切片超时 → 检查取消/总超时
        }
    }
}

fn stun_step(emit: &dyn Fn(StreamEvent), phase: &str, description: &str) {
    let step = StunTestStep {
        phase: phase.to_string(),
        description: description.to_string(),
    };
    emit(StreamEvent::json("stun-step", &step));
}

/// 流式 NAT 检测主流程（阻塞；在后台线程执行）
pub fn detect_nat_streaming(
    stun_server: &str,
    cmd_id: &str,
    emit: &dyn Fn(StreamEvent),
) -> Result<NatDetectionResult, String> {
    let server_addr = resolve_stun(stun_server)?;
    let local_ip = get_local_ip();
    let mut rng = rand::thread_rng();

    stun_step(
        &emit,
        "初始化",
        &format!(
            "STUN 服务器地址: {}:{} | 本机 IP 地址: {} | 开始 NAT 类型检测 (RFC 3489)",
            server_addr.ip(),
            server_addr.port(),
            local_ip
        ),
    );

    if is_cancelled(cmd_id) {
        emit(StreamEvent::text("error", "用户取消"));
        return Err("Cancelled".into());
    }

    // Phase 1：绑定请求 → 映射地址
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("UDP bind失败: {}", e))?;
    let tid1: [u8; 12] = rng.gen();
    socket
        .send_to(&build_binding_request(&tid1), server_addr)
        .map_err(|e| format!("STUN send失败: {}", e))?;
    let mut buf = [0u8; 2048];
    let (len, _) = recv_with_cancel(&socket, cmd_id, &mut buf).ok_or_else(|| {
        if is_cancelled(cmd_id) {
            "用户取消".to_string()
        } else {
            "阶段1超时".to_string()
        }
    })?;
    let mapped =
        parse_mapped_address(&buf[..len]).ok_or_else(|| "无法解析MAPPED-ADDRESS".to_string())?;
    stun_step(
        &emit,
        "阶段1: 绑定请求",
        &format!(
            "发送 Binding Request → 收到响应 | NAT 映射后的公网地址: {}:{}",
            mapped.0, mapped.1
        ),
    );

    if is_cancelled(cmd_id) {
        emit(StreamEvent::text("error", "用户取消"));
        return Err("Cancelled".into());
    }

    // Phase 2：二次绑定 → 端口变化判定（对称 NAT）
    let socket2 = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("UDP bind2失败: {}", e))?;
    let tid2: [u8; 12] = rng.gen();
    socket2
        .send_to(&build_binding_request(&tid2), server_addr)
        .map_err(|e| format!("STUN send2失败: {}", e))?;
    let mut buf2 = [0u8; 2048];
    let (len2, _) = recv_with_cancel(&socket2, cmd_id, &mut buf2).ok_or_else(|| {
        if is_cancelled(cmd_id) {
            "用户取消".to_string()
        } else {
            "阶段2超时".to_string()
        }
    })?;
    let mapped2 = parse_mapped_address(&buf2[..len2]).ok_or_else(|| "阶段2解析失败".to_string())?;
    let port_changed = mapped.1 != mapped2.1;
    stun_step(
        &emit,
        "阶段2: 映射行为检测",
        &format!(
            "第二次绑定请求 | 映射1: {}:{} | 映射2: {}:{} → 端口是否变化: {} (判断是否为对称NAT)",
            mapped.0,
            mapped.1,
            mapped2.0,
            mapped2.1,
            if port_changed { "是" } else { "否" }
        ),
    );

    if is_cancelled(cmd_id) {
        emit(StreamEvent::text("error", "用户取消"));
        return Err("Cancelled".into());
    }

    // Phase 3：Change-Request → 过滤行为判定
    let socket3 = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("UDP bind3失败: {}", e))?;
    let tid3: [u8; 12] = rng.gen();
    socket3
        .send_to(&build_change_request(&tid3, true, true), server_addr)
        .map_err(|e| format!("STUN send3失败: {}", e))?;
    let mut buf3 = [0u8; 2048];
    let filtered = recv_with_cancel(&socket3, cmd_id, &mut buf3).is_none();
    if is_cancelled(cmd_id) {
        emit(StreamEvent::text("error", "用户取消"));
        return Err("Cancelled".into());
    }
    stun_step(
        &emit,
        "阶段3: 过滤行为检测",
        &format!(
            "发送 Change-Request (变更IP+端口) → {} | 判断是否允许外部主机回包",
            if filtered {
                "无响应 → NAT/防火墙过滤了外部数据包"
            } else {
                "收到响应 → NAT 允许外部数据包通过"
            }
        ),
    );

    // 分类（与原版判定式完全一致）
    let nat_type = if mapped.0 == local_ip {
        NatType::Nat0
    } else if !port_changed && !filtered {
        NatType::Nat1
    } else if !port_changed && filtered {
        NatType::Nat2
    } else if port_changed && filtered {
        NatType::Nat4
    } else {
        NatType::Nat3
    };

    let result = NatDetectionResult {
        nat_type: nat_type.clone(),
        nat_label: nat_type.to_string(),
        public_ip: mapped.0.to_string(),
        public_port: mapped.1,
        local_ip: local_ip.to_string(),
        steps: vec![], // 步骤已逐条 emit
        success: true,
        error_message: String::new(),
    };
    emit(StreamEvent::json("summary", &result));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_binding_request() {
        let tid = [0u8; 12];
        let pkt = build_binding_request(&tid);
        assert_eq!(pkt.len(), 20);
        assert_eq!(&pkt[0..2], &0x0001u16.to_be_bytes());
        assert_eq!(&pkt[4..8], &MAGIC_COOKIE.to_be_bytes());
    }

    #[test]
    fn test_build_change_request_flags() {
        let tid = [7u8; 12];
        let pkt = build_change_request(&tid, true, true);
        assert_eq!(pkt.len(), 28);
        assert_eq!(&pkt[20..22], &0x0003u16.to_be_bytes());
        assert_eq!(&pkt[24..28], &0x0006u32.to_be_bytes());
        let pkt2 = build_change_request(&tid, false, true);
        assert_eq!(&pkt2[24..28], &0x0004u32.to_be_bytes());
    }

    #[test]
    fn test_parse_mapped_address_plain() {
        let mut pkt = vec![0u8; 20];
        pkt[0..2].copy_from_slice(&0x0101u16.to_be_bytes()); // Binding Response
        pkt[2..4].copy_from_slice(&12u16.to_be_bytes()); // msg len
                                                         // 属性：MAPPED-ADDRESS，len 8，family 1，port 3478，ip 1.2.3.4
        pkt.extend_from_slice(&0x0001u16.to_be_bytes());
        pkt.extend_from_slice(&8u16.to_be_bytes());
        pkt.push(0);
        pkt.push(0x01);
        pkt.extend_from_slice(&3478u16.to_be_bytes());
        pkt.extend_from_slice(&[1, 2, 3, 4]);
        let (ip, port) = parse_mapped_address(&pkt).unwrap();
        assert_eq!(ip, IpAddr::from([1, 2, 3, 4]));
        assert_eq!(port, 3478);
    }

    #[test]
    fn test_parse_mapped_address_xor() {
        let mut pkt = vec![0u8; 20];
        pkt[0..2].copy_from_slice(&0x0101u16.to_be_bytes());
        pkt[2..4].copy_from_slice(&12u16.to_be_bytes());
        pkt[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
        pkt.extend_from_slice(&0x0020u16.to_be_bytes()); // XOR-MAPPED
        pkt.extend_from_slice(&8u16.to_be_bytes());
        pkt.push(0);
        pkt.push(0x01);
        // XOR 端口 = 3478 ^ 0x2112
        pkt.extend_from_slice(&(3478u16 ^ 0x2112).to_be_bytes());
        // XOR IP = 1.2.3.4 ^ magic 字节序
        let magic = MAGIC_COOKIE.to_be_bytes();
        pkt.extend_from_slice(&[1 ^ magic[0], 2 ^ magic[1], 3 ^ magic[2], 4 ^ magic[3]]);
        let (ip, port) = parse_mapped_address(&pkt).unwrap();
        assert_eq!(ip, IpAddr::from([1, 2, 3, 4]));
        assert_eq!(port, 3478);
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_mapped_address(&[0u8; 10]).is_none(), "短包");
        let mut bad = vec![0u8; 32];
        bad[2..4].copy_from_slice(&64u16.to_be_bytes()); // 声明长度超过实际
        assert!(parse_mapped_address(&bad).is_none());
    }

    #[test]
    fn test_get_stun_servers() {
        assert!(get_stun_servers().len() >= 15);
    }

    /// 实机：STUN 三阶段检测（stun.l.google.com）
    ///
    /// 注：实证（2026-09-07 本机）stun.qq.com/miwifi 不响应 UDP 3478（原版实现同样
    /// 3s 超时，行为一致）；Google STUN 正常响应，选其验证端到端链路。
    #[test]
    #[ignore = "实机网络用例：cargo test -- --ignored 运行"]
    fn real_nat_detection() {
        let steps = std::sync::Mutex::new(Vec::<String>::new());
        let r = detect_nat_streaming("stun.l.google.com:19302", "test-real-nat", &|ev| {
            steps
                .lock()
                .unwrap()
                .push(format!("{}/{}", ev.kind, ev.data))
        })
        .expect("NAT 检测应成功完成");
        assert!(r.success, "检测应成功");
        assert!(!r.public_ip.is_empty(), "应有公网映射地址");
        assert!(
            r.nat_label.starts_with("NAT"),
            "标签应形如 NAT0-NAT4: {}",
            r.nat_label
        );
        let n = steps.lock().unwrap().len();
        assert!(
            n >= 4,
            "应有 ≥3 个 stun-step + 1 个 summary 事件，实际 {}",
            n
        );
    }
}
