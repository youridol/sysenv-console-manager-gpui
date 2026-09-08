// net_diag::dns_client — 自研 RFC 1035 UDP DNS 客户端（A / AAAA 查询）
//
// 替代原版 trust-dns-resolver（ADR-0002 §1：去 tokio 依赖，纯 std UdpSocket）。
// 支持：
// - 报文构造：标准 12 字节头（RD=1，QDCOUNT=1）+ QNAME 标签 + QTYPE/QCLASS=IN
// - 报文解析：跳过 Question，遍历 Answer（含 0xC0 压缩指针），取 A(1)/AAAA(28) 记录；
//   CNAME(5) 存在时对目标域名递归查询（最多 3 跳，防环）
// - 指定 DNS 服务器（Traceroute 预设/自定义）与系统默认 DNS（GetAdaptersAddresses 同源）
// - 单次查询 2s 超时 × 2 次尝试（对齐原版 traceroute 指定 DNS 的「2s、1 次尝试、不走 TCP」
//   调优语义，并保留系统解析的基础可用性）
// - TC（截断）位：显式报错（UDP DNS 客户端边界，不静默失败）

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

/// QTYPE：A 记录（IPv4）
pub const QTYPE_A: u16 = 1;
/// QTYPE：AAAA 记录（IPv6）
pub const QTYPE_AAAA: u16 = 28;
/// QTYPE：CNAME
const QTYPE_CNAME: u16 = 5;
/// QCLASS：IN
const QCLASS_IN: u16 = 1;
/// 单次查询超时
const QUERY_TIMEOUT_MS: u64 = 2000;
/// 尝试次数
const QUERY_ATTEMPTS: usize = 2;
/// CNAME 跟随上限
const MAX_CNAME_FOLLOWS: usize = 3;
/// 接收缓冲
const RECV_BUF: usize = 4096;

/// 构造 DNS 查询报文
pub fn build_query(id: u16, domain: &str, qtype: u16) -> Result<Vec<u8>, String> {
    let domain = domain.trim().trim_end_matches('.');
    if domain.is_empty() {
        return Err("域名为空".to_string());
    }
    let mut pkt = Vec::with_capacity(64);
    pkt.extend_from_slice(&id.to_be_bytes()); // ID
    pkt.extend_from_slice(&0x0100u16.to_be_bytes()); // Flags: RD=1
    pkt.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT=1
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ANCOUNT
    pkt.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT
                                                // QNAME：按 '.' 分段标签，每段前缀长度字节
    for label in domain.split('.') {
        if label.is_empty() {
            continue; // 容忍连续点/结尾点
        }
        let bytes = label.as_bytes();
        if bytes.len() > 63 {
            return Err(format!("域名标签过长（>63 字节）: {label}"));
        }
        pkt.push(bytes.len() as u8);
        pkt.extend_from_slice(bytes);
    }
    pkt.push(0); // 根标签终止
    pkt.extend_from_slice(&qtype.to_be_bytes()); // QTYPE
    pkt.extend_from_slice(&QCLASS_IN.to_be_bytes()); // QCLASS=IN
    Ok(pkt)
}

/// 域名标签读取（支持 0xC0 压缩指针；返回 (域名, 消费的字节数)）
fn read_name(pkt: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let start_pos = pos;
    let mut labels: Vec<String> = Vec::new();
    let mut jumped = false;
    let mut consumed = 0usize;
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > 64 {
            return None; // 防压缩环
        }
        let len = *pkt.get(pos)?;
        if len & 0xC0 == 0xC0 {
            // 压缩指针：高 2 位 11，低 14 位偏移
            if pos + 1 >= pkt.len() {
                return None;
            }
            let ptr = (((len & 0x3F) as usize) << 8) | pkt[pos + 1] as usize;
            if !jumped {
                consumed = pos + 2 - start_pos;
                jumped = true;
            }
            pos = ptr;
            continue;
        }
        if len == 0 {
            if !jumped {
                consumed = pos + 1 - start_pos;
            }
            break;
        }
        let end = pos + 1 + len as usize;
        if end > pkt.len() {
            return None;
        }
        labels.push(String::from_utf8_lossy(&pkt[pos + 1..end]).to_string());
        pos = end;
    }
    Some((labels.join("."), consumed))
}

/// 单条解析出的记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsRecord {
    pub rtype: u16,
    /// A → 4 字节；AAAA → 16 字节；CNAME → 域名
    pub value: RecordValue,
}

/// 记录值
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordValue {
    V4(Ipv4Addr),
    V6(Ipv6Addr),
    Domain(String),
}

/// 解析 DNS 响应，返回 Answer 区记录
pub fn parse_response(pkt: &[u8]) -> Result<Vec<DnsRecord>, String> {
    if pkt.len() < 12 {
        return Err("响应过短（<12 字节）".to_string());
    }
    let flags = u16::from_be_bytes([pkt[2], pkt[3]]);
    if flags & 0x8000 == 0 {
        return Err("响应不是 QR=1 的应答报文".to_string());
    }
    let rcode = flags & 0x000F;
    if rcode != 0 {
        return Err(format!("DNS 服务器返回错误码 {}", rcode));
    }
    if flags & 0x0200 != 0 {
        return Err("响应被截断（TC=1，超出 UDP 单次容量）".to_string());
    }
    let qdcount = u16::from_be_bytes([pkt[4], pkt[5]]) as usize;
    let ancount = u16::from_be_bytes([pkt[6], pkt[7]]) as usize;
    let mut pos = 12usize;
    // 跳过 Question 区
    for _ in 0..qdcount {
        let (_, used) = read_name(pkt, pos).ok_or_else(|| "Question 区域名解析失败".to_string())?;
        pos += used + 4; // + QTYPE/QCLASS
        if pos > pkt.len() {
            return Err("Question 区越界".to_string());
        }
    }
    // 遍历 Answer 区
    let mut out = Vec::new();
    for _ in 0..ancount {
        let (_, used) = read_name(pkt, pos).ok_or_else(|| "Answer 区域名解析失败".to_string())?;
        pos += used;
        if pos + 10 > pkt.len() {
            break;
        }
        let rtype = u16::from_be_bytes([pkt[pos], pkt[pos + 1]]);
        let rdlength = u16::from_be_bytes([pkt[pos + 8], pkt[pos + 9]]) as usize;
        pos += 10;
        let rdata = pkt
            .get(pos..pos + rdlength)
            .ok_or_else(|| "RDATA 越界".to_string())?;
        match rtype {
            QTYPE_A if rdlength == 4 => out.push(DnsRecord {
                rtype,
                value: RecordValue::V4(Ipv4Addr::new(rdata[0], rdata[1], rdata[2], rdata[3])),
            }),
            QTYPE_AAAA if rdlength == 16 => {
                let mut octets = [0u8; 16];
                octets.copy_from_slice(rdata);
                out.push(DnsRecord {
                    rtype,
                    value: RecordValue::V6(Ipv6Addr::from(octets)),
                })
            }
            QTYPE_CNAME => {
                if let Some((name, _)) = read_name(pkt, pos) {
                    out.push(DnsRecord {
                        rtype,
                        value: RecordValue::Domain(name),
                    })
                }
            }
            _ => {}
        }
        pos += rdlength;
    }
    Ok(out)
}

/// 通过指定 DNS 服务器查询（UDP；2s × 2 次；CNAME 最多跟随 3 跳防环）
pub fn query(server: IpAddr, domain: &str, qtype: u16) -> Result<IpAddr, String> {
    query_all(server, domain, qtype)?
        .into_iter()
        .next()
        .ok_or_else(|| "无匹配记录".to_string())
}

/// 通过指定 DNS 服务器查询全部目标类型记录（Nslookup 用；对齐原版全量列表语义）
pub fn query_all(server: IpAddr, domain: &str, qtype: u16) -> Result<Vec<IpAddr>, String> {
    query_all_depth(server, domain, qtype, 0)
}

/// 带深度限制的内部查询实现
fn query_all_depth(
    server: IpAddr,
    domain: &str,
    qtype: u16,
    depth: usize,
) -> Result<Vec<IpAddr>, String> {
    if depth > MAX_CNAME_FOLLOWS {
        return Err("CNAME 跟随超过 3 跳（可能存在环）".to_string());
    }
    let id: u16 = rand::random();
    let pkt = build_query(id, domain, qtype)?;
    let bind = if server.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let sock = UdpSocket::bind(bind).map_err(|e| format!("UDP bind 失败: {e}"))?;
    let dst = SocketAddr::new(server, 53);
    let mut last_err = String::from("查询未执行");
    for _ in 0..QUERY_ATTEMPTS {
        let start = Instant::now();
        if sock.send_to(&pkt, dst).is_err() {
            last_err = "发送失败".to_string();
            continue;
        }
        let _ = sock.set_read_timeout(Some(Duration::from_millis(QUERY_TIMEOUT_MS)));
        let mut buf = vec![0u8; RECV_BUF];
        // 循环 recv 直到拿到匹配 id 的响应或超时（丢弃迟到的前次尝试残余）
        loop {
            let now = Instant::now();
            if now.duration_since(start) >= Duration::from_millis(QUERY_TIMEOUT_MS) {
                last_err = "查询超时（2s）".to_string();
                break;
            }
            match sock.recv_from(&mut buf) {
                Ok((len, src)) if src == dst && len >= 12 => {
                    if u16::from_be_bytes([buf[0], buf[1]]) != id {
                        continue; // 事务 ID 不匹配，继续等
                    }
                    let records = parse_response(&buf[..len])?;
                    // 收集全部目标类型记录
                    let ips: Vec<IpAddr> = records
                        .iter()
                        .filter_map(|r| match &r.value {
                            RecordValue::V4(v) if qtype == QTYPE_A => Some(IpAddr::V4(*v)),
                            RecordValue::V6(v) if qtype == QTYPE_AAAA => Some(IpAddr::V6(*v)),
                            _ => None,
                        })
                        .collect();
                    if !ips.is_empty() {
                        return Ok(ips);
                    }
                    // CNAME 跟随（限次防环）
                    if let Some(RecordValue::Domain(cname)) =
                        records.iter().map(|r| &r.value).next()
                    {
                        return query_all_depth(server, cname, qtype, depth + 1);
                    }
                    last_err = "无匹配记录".to_string();
                    break;
                }
                Ok(_) => continue, // 非目标源/短包，继续收
                Err(e) => {
                    last_err = format!("接收失败: {e}");
                    break;
                }
            }
        }
    }
    Err(last_err)
}

/// 本机系统 DNS 服务器（GetAdaptersAddresses 同源，取首个 Up 且有 DNS 的适配器）
pub fn system_dns_servers() -> Vec<IpAddr> {
    let Ok(adapters) = secm_datasource::netif::adapter_configs() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for a in adapters {
        if a.status != "Up" {
            continue;
        }
        for s in a.ipv4_dns.iter().chain(a.ipv6_dns.iter()) {
            if let Ok(ip) = s.parse::<IpAddr>() {
                if !out.contains(&ip) {
                    out.push(ip);
                }
            }
        }
        if !out.is_empty() {
            break; // 首个 Up 且有 DNS 的适配器即可
        }
    }
    out
}

/// 解析域名（系统默认 DNS 链路）：先尝试本机 DNS 服务器逐台查询，
/// 全部失败时回退系统解析器（ToSocketAddrs），保证基础可用性。
pub fn resolve_system(domain: &str, qtype: u16) -> Result<IpAddr, String> {
    resolve_system_all(domain, qtype)?
        .into_iter()
        .next()
        .ok_or_else(|| "无匹配记录".to_string())
}

/// 解析域名全部记录（系统默认 DNS 链路；Nslookup 用，对齐原版全量列表语义）
pub fn resolve_system_all(domain: &str, qtype: u16) -> Result<Vec<IpAddr>, String> {
    let servers = system_dns_servers();
    let mut last_err = String::from("无可用本机 DNS 服务器");
    for srv in &servers {
        match query_all(*srv, domain, qtype) {
            Ok(ips) if !ips.is_empty() => return Ok(ips),
            Ok(_) => last_err = "无匹配记录".to_string(),
            Err(e) => last_err = e,
        }
    }
    // 回退：系统解析器（语义 = 原版 trust-dns 系统默认配置）
    let want_v6 = qtype == QTYPE_AAAA;
    let addrs: Vec<SocketAddr> = format!("{}:0", domain)
        .to_socket_addrs()
        .map_err(|e| {
            format!(
                "系统 DNS 解析失败 {}: {}（本机 DNS 尝试: {}）",
                domain, e, last_err
            )
        })?
        .collect();
    let matched: Vec<IpAddr> = addrs
        .iter()
        .filter(|a| a.is_ipv6() == want_v6)
        .map(|a| a.ip())
        .collect();
    if !matched.is_empty() {
        return Ok(matched);
    }
    Err(last_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_query_shape() {
        let pkt = build_query(0x1234, "example.com", QTYPE_A).unwrap();
        // 头：id + flags(RD) + qdcount=1 + 三个 0
        assert_eq!(&pkt[0..2], &[0x12, 0x34]);
        assert_eq!(&pkt[2..4], &[0x01, 0x00]);
        assert_eq!(&pkt[4..6], &[0, 1]);
        // QNAME：7 e x a m p l e 3 c o m 0
        assert_eq!(pkt[12], 7);
        assert_eq!(&pkt[13..20], b"example");
        assert_eq!(pkt[20], 3);
        assert_eq!(&pkt[21..24], b"com");
        assert_eq!(pkt[24], 0);
        // QTYPE=A / QCLASS=IN
        assert_eq!(&pkt[25..27], &1u16.to_be_bytes());
        assert_eq!(&pkt[27..29], &1u16.to_be_bytes());
        // 尾部点容忍
        assert!(build_query(1, "example.com.", QTYPE_A).is_ok());
        assert!(build_query(1, "", QTYPE_A).is_err());
    }

    /// 构造最小合法响应（1 条 A 记录，无压缩）
    fn build_a_response(domain: &str, ip: [u8; 4]) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&0x1234u16.to_be_bytes());
        p.extend_from_slice(&0x8180u16.to_be_bytes()); // QR=1, RD=1, RA=1, RCODE=0
        p.extend_from_slice(&1u16.to_be_bytes());
        p.extend_from_slice(&1u16.to_be_bytes());
        p.extend_from_slice(&0u16.to_be_bytes());
        p.extend_from_slice(&0u16.to_be_bytes());
        for label in domain.split('.') {
            p.push(label.len() as u8);
            p.extend_from_slice(label.as_bytes());
        }
        p.push(0);
        p.extend_from_slice(&QTYPE_A.to_be_bytes());
        p.extend_from_slice(&QCLASS_IN.to_be_bytes());
        // Answer：name 指针指向 12（Question 头）
        p.extend_from_slice(&0xC00Cu16.to_be_bytes());
        p.extend_from_slice(&QTYPE_A.to_be_bytes());
        p.extend_from_slice(&QCLASS_IN.to_be_bytes());
        p.extend_from_slice(&300u32.to_be_bytes()); // TTL
        p.extend_from_slice(&4u16.to_be_bytes()); // RDLENGTH
        p.extend_from_slice(&ip);
        p
    }

    #[test]
    fn test_parse_response_a_with_compression() {
        let pkt = build_a_response("example.com", [93, 184, 216, 34]);
        let records = parse_response(&pkt).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].value,
            RecordValue::V4(Ipv4Addr::new(93, 184, 216, 34))
        );
    }

    #[test]
    fn test_parse_response_errors() {
        assert!(parse_response(&[0u8; 8]).is_err(), "短包应报错");
        // QR=0（查询而非应答）
        let mut bad = build_a_response("example.com", [1, 2, 3, 4]);
        bad[2] = 0x00;
        assert!(parse_response(&bad).is_err());
        // RCODE=2（服务器失败）
        let mut nx = build_a_response("example.com", [1, 2, 3, 4]);
        nx[3] |= 0x0002;
        assert!(parse_response(&nx).is_err());
    }

    #[test]
    fn test_cname_follow_parse() {
        // Answer: CNAME + A（指针指向 CNAME 的 RDATA）
        let mut p = Vec::new();
        p.extend_from_slice(&0x0001u16.to_be_bytes());
        p.extend_from_slice(&0x8180u16.to_be_bytes());
        p.extend_from_slice(&1u16.to_be_bytes()); // qd
        p.extend_from_slice(&2u16.to_be_bytes()); // an
        p.extend_from_slice(&0u16.to_be_bytes());
        p.extend_from_slice(&0u16.to_be_bytes());
        // Question: www.example.com A IN
        for label in ["www", "example", "com"] {
            p.push(label.len() as u8);
            p.extend_from_slice(label.as_bytes());
        }
        p.push(0);
        p.extend_from_slice(&QTYPE_A.to_be_bytes());
        p.extend_from_slice(&QCLASS_IN.to_be_bytes());
        // Answer1: CNAME example.com
        p.extend_from_slice(&0xC00Cu16.to_be_bytes());
        p.extend_from_slice(&QTYPE_CNAME.to_be_bytes());
        p.extend_from_slice(&QCLASS_IN.to_be_bytes());
        p.extend_from_slice(&60u32.to_be_bytes());
        let rdlen_pos = p.len();
        p.extend_from_slice(&0u16.to_be_bytes()); // RDLENGTH 占位后修
        let rdata_start = p.len();
        p.push(7);
        p.extend_from_slice(b"example");
        p.push(3);
        p.extend_from_slice(b"com");
        p.push(0);
        let rdlen = (p.len() - rdata_start) as u16;
        p[rdlen_pos..rdlen_pos + 2].copy_from_slice(&rdlen.to_be_bytes());
        // Answer2: A 1.2.3.4（名字 = 压缩指针指向 CNAME RDATA）
        p.extend_from_slice(&(0xC000 | rdata_start as u16).to_be_bytes());
        p.extend_from_slice(&QTYPE_A.to_be_bytes());
        p.extend_from_slice(&QCLASS_IN.to_be_bytes());
        p.extend_from_slice(&60u32.to_be_bytes());
        p.extend_from_slice(&4u16.to_be_bytes());
        p.extend_from_slice(&[1, 2, 3, 4]);
        let records = parse_response(&p).unwrap();
        assert_eq!(records.len(), 2, "应解析出 CNAME + A: {:?}", records);
        assert_eq!(records[1].value, RecordValue::V4(Ipv4Addr::new(1, 2, 3, 4)));
    }

    /// 实机：指定 DNS 服务器 A 查询
    #[test]
    #[ignore = "实机网络用例：cargo test -- --ignored 运行"]
    fn real_dns_query_a() {
        let ip = query(
            IpAddr::V4(Ipv4Addr::new(223, 5, 5, 5)),
            "www.baidu.com",
            QTYPE_A,
        )
        .expect("阿里 DNS A 查询应成功");
        assert!(matches!(ip, IpAddr::V4(_)), "应返回 IPv4: {}", ip);
    }

    /// 实机：指定 DNS 服务器 AAAA 查询
    #[test]
    #[ignore = "实机网络用例：cargo test -- --ignored 运行"]
    fn real_dns_query_aaaa() {
        let ip = query(
            IpAddr::V4(Ipv4Addr::new(223, 5, 5, 5)),
            "www.baidu.com",
            QTYPE_AAAA,
        )
        .expect("阿里 DNS AAAA 查询应成功");
        assert!(matches!(ip, IpAddr::V6(_)), "应返回 IPv6: {}", ip);
    }

    /// 实机：系统 DNS 链路全量查询
    #[test]
    #[ignore = "实机网络用例：cargo test -- --ignored 运行"]
    fn real_dns_resolve_system_all() {
        let ips = resolve_system_all("www.baidu.com", QTYPE_A).expect("系统链路 A 查询应成功");
        assert!(!ips.is_empty(), "应有至少一条 A 记录");
    }
}
