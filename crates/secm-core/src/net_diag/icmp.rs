// net_diag::icmp — ICMP 引擎（Windows 原生 iphlpapi，非管理员可用）
//
// 决策依据（ADR-0002）：
// - 原版 Ping 用 surge_ping（tokio 依赖 + DGRAM ICMP socket）；Traceroute 用 socket2
//   自研 raw ICMP（**需要管理员**）。本引擎统一改用系统态 ICMP API：
//     * IPv4：IcmpSendEcho（iphlpapi.dll，同步，TTL 经 IP_OPTION_INFORMATION 生效）
//     * IPv6：Icmp6SendEcho2（同步模式，event/apc 均传 NULL）
//   两者均不需要管理员权限，满足 ADR「Windows 非管理员场景逐项验证」要求。
// - Time Exceeded（IP_TTL_EXPIRED_TRANSIT）时系统直接给出响应路由器地址
//   （ICMP_ECHO_REPLY.Address / 手工解析的 v6 回复头），无需自解析内嵌 IP 头。
//
// v6 回复缓冲布局说明：windows-sys 0.61 的 IPV6_ADDRESS_EX 为 packed(1) 无 family
// 字段；实机回复缓冲验证（::1 回环）确认该布局即 API 真实布局——地址在偏移 6..22、
// Status@26、RTT@30，故 v6 回复按此布局手工解析（见 parse_v6_reply）。
//
// SAFETY 总注：所有 unsafe 块仅操作 windows-sys 声明的 FFI 入参与调用方提供的缓冲；
// 句柄 RAII 关闭（IcmpCloseHandle），无跨线程共享句柄。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, UdpSocket};

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::NetworkManagement::IpHelper::{
    Icmp6CreateFile, Icmp6SendEcho2, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho,
    ICMP_ECHO_REPLY, IP_OPTION_INFORMATION, IP_TTL_EXPIRED_REASSEM,
};
use windows_sys::Win32::Networking::WinSock::{
    WSAGetLastError, WSAStartup, AF_INET6, SOCKADDR_IN6, WSADATA,
};

/// IP_STATUS：成功（0）
pub const IP_STATUS_SUCCESS: u32 = 0;
/// IP_STATUS：请求超时
pub const IP_REQ_TIMED_OUT: u32 = 11010;
/// IP_STATUS：TTL 在传输中超限（traceroute 中间跳命中态）
pub const IP_TTL_EXPIRED_TRANSIT: u32 = 11013;
/// 内部错误码：ICMP 句柄创建失败（非 IP_STATUS）
pub const ICMP_HANDLE_ERROR: u32 = u32::MAX;

/// 一次 ICMP 探测的原始结果
#[derive(Debug, Clone)]
pub struct IcmpReply {
    /// IP_STATUS 码（0 = 成功；11010 = 超时；11013 = TTL 超限 …）
    pub status: u32,
    /// 响应者地址（Echo Reply = 目标；Time Exceeded = 中间路由器）
    pub responder: Option<IpAddr>,
    /// 往返耗时（毫秒；仅 status == 0 有效）
    pub rtt_ms: u32,
}

impl IcmpReply {
    /// 探测是否成功（Echo Reply）
    pub fn is_success(&self) -> bool {
        self.status == IP_STATUS_SUCCESS
    }

    /// 是否为 TTL 超限（traceroute 中间跳）
    pub fn is_ttl_expired(&self) -> bool {
        self.status == IP_TTL_EXPIRED_TRANSIT
    }
}

/// WinSock 初始化（进程级幂等；ICMP API 依赖 WSAStartup 后的套接字环境）
fn ensure_winsock() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        // SAFETY: WSADATA 为调用方提供的有效可写缓冲；0x0202 = MAKEWORD(2,2)
        unsafe {
            let mut data: WSADATA = std::mem::zeroed();
            let _ = WSAStartup(0x0202, &mut data);
            // 失败不中断：多数环境已由宿主初始化；后续调用各自处理错误
        }
    });
}

/// IPv4 地址 → IPAddr（IPAddr 的内存字节即地址八位组顺序，from_ne_bytes 保持内存序；
/// 错误用法 from_be_bytes 会得到字节反转的地址，表现为全体超时）
fn ipaddr_to_u32(ip: Ipv4Addr) -> u32 {
    u32::from_ne_bytes(ip.octets())
}

/// IPAddr → IPv4 地址（读方向惯用法：值按大端解释回主机序 u32）
fn u32_to_ipaddr(v: u32) -> Ipv4Addr {
    Ipv4Addr::from(u32::from_be(v))
}

/// IPv4 单次 ICMP Echo（IcmpSendEcho，同步）
///
/// - `size`：载荷字节数（IP 包总长上限 65507，由调用方钳制）
/// - `ttl`：IP TTL（1..=255）
/// - `timeout_ms`：单包等待超时
pub fn ping_v4(ip: Ipv4Addr, size: usize, ttl: u8, timeout_ms: u32) -> IcmpReply {
    ensure_winsock();
    // SAFETY: IcmpCreateFile 无入参；返回 INVALID_HANDLE_VALUE 时走错误路径
    let handle = unsafe { IcmpCreateFile() };
    if handle == INVALID_HANDLE_VALUE {
        return IcmpReply {
            status: ICMP_HANDLE_ERROR,
            responder: None,
            rtt_ms: 0,
        };
    }
    let data = vec![0u8; size];
    let options = IP_OPTION_INFORMATION {
        Ttl: ttl,
        Tos: 0,
        Flags: 0,
        OptionsSize: 0,
        OptionsData: std::ptr::null_mut(),
    };
    // 回复缓冲：ICMP_ECHO_REPLY 结构 + 载荷 + 对齐余量（MSDN 要求 ≥ sizeof+RequestSize）
    let reply_size = std::mem::size_of::<ICMP_ECHO_REPLY>() + size + 64;
    let mut buf = vec![0u8; reply_size];
    // SAFETY: 各指针均为有效缓冲；IcmpSendEcho 为同步调用
    let replies = unsafe {
        IcmpSendEcho(
            handle,
            ipaddr_to_u32(ip),
            data.as_ptr() as *const core::ffi::c_void,
            size as u16,
            &options,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            reply_size as u32,
            timeout_ms,
        )
    };
    let reply = if replies > 0 {
        // SAFETY: replies > 0 表示缓冲内含至少一个完整 ICMP_ECHO_REPLY
        let r = unsafe { *(buf.as_ptr() as *const ICMP_ECHO_REPLY) };
        IcmpReply {
            status: r.Status,
            responder: Some(IpAddr::V4(u32_to_ipaddr(r.Address))),
            rtt_ms: r.RoundTripTime,
        }
    } else {
        // 返回 0：WSAGetLastError 携带 IP_STATUS（如 IP_REQ_TIMED_OUT）
        let err = unsafe { WSAGetLastError() };
        IcmpReply {
            status: err as u32,
            responder: None,
            rtt_ms: 0,
        }
    };
    // SAFETY: handle 为 IcmpCreateFile 返回的有效句柄
    unsafe {
        IcmpCloseHandle(handle);
    }
    reply
}

/// 通过 UDP connect 让协议栈为本机选定 IPv6 源地址（对齐原版 traceroute 做法）
fn pick_v6_source(dest: Ipv6Addr) -> Option<Ipv6Addr> {
    let sock = UdpSocket::bind(if dest.is_loopback() {
        "[::1]:0"
    } else {
        "[::]:0"
    })
    .ok()?;
    sock.connect((dest, 9)).ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V6(v6) => Some(v6),
        _ => None,
    }
}

/// 构造 SOCKADDR_IN6（family + 地址；端口/流标签/scope 归零）
fn sockaddr_in6(ip: Ipv6Addr) -> SOCKADDR_IN6 {
    // SAFETY: 全零初始化合法（POD 结构）
    let mut sa: SOCKADDR_IN6 = unsafe { std::mem::zeroed() };
    sa.sin6_family = AF_INET6 as u16;
    sa.sin6_addr.u.Byte = ip.octets();
    sa
}

/// IPv6 单次 ICMPv6 Echo（Icmp6SendEcho2 同步模式）
pub fn ping_v6(ip: Ipv6Addr, size: usize, ttl: u8, timeout_ms: u32) -> IcmpReply {
    ensure_winsock();
    // SAFETY: Icmp6CreateFile 无入参
    let handle = unsafe { Icmp6CreateFile() };
    if handle == INVALID_HANDLE_VALUE {
        return IcmpReply {
            status: ICMP_HANDLE_ERROR,
            responder: None,
            rtt_ms: 0,
        };
    }
    let src = pick_v6_source(ip).unwrap_or(Ipv6Addr::UNSPECIFIED);
    let src_sa = sockaddr_in6(src);
    let dst_sa = sockaddr_in6(ip);
    let data = vec![0u8; size];
    let options = IP_OPTION_INFORMATION {
        Ttl: ttl,
        Tos: 0,
        Flags: 0,
        OptionsSize: 0,
        OptionsData: std::ptr::null_mut(),
    };
    // 回复缓冲：SDK 布局头 36 字节 + 载荷 + 余量（见模块头注释）
    let reply_size = 64 + size;
    let mut buf = vec![0u8; reply_size];
    // SAFETY: 同步模式 event/apc/context 为 NULL；其余为有效缓冲
    let replies = unsafe {
        Icmp6SendEcho2(
            handle,
            std::ptr::null_mut(),
            None,
            std::ptr::null(),
            &src_sa,
            &dst_sa,
            data.as_ptr() as *const core::ffi::c_void,
            size as u16,
            &options,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            reply_size as u32,
            timeout_ms,
        )
    };
    let reply = if replies > 0 {
        parse_v6_reply(&buf)
    } else {
        let err = unsafe { WSAGetLastError() };
        IcmpReply {
            status: err as u32,
            responder: None,
            rtt_ms: 0,
        }
    };
    // SAFETY: handle 为 Icmp6CreateFile 返回的有效句柄
    unsafe {
        IcmpCloseHandle(handle);
    }
    reply
}

/// 按 ICMPV6_ECHO_REPLY 实际布局解析 Icmp6SendEcho2 回复（本机实证确认）：
/// windows-sys 0.61 的 IPV6_ADDRESS_EX 为 packed(1) 无 family 字段——与实机回复缓冲
/// 一致（实测 ::1：地址在偏移 6..22 末字节=01，Status@26=IP_SUCCESS，RTT@30）：
///   [0]  sin6_port: u16        [2]  sin6_flowinfo: u32
///   [6]  sin6_addr:   [u8;16]  [22] sin6_scope_id: u32
///   [26] Status:      u32      [30] RoundTripTime: u32
fn parse_v6_reply(buf: &[u8]) -> IcmpReply {
    // 最小长度：头 34 字节（无 options 数据）
    if buf.len() < 34 {
        return IcmpReply {
            status: ICMP_HANDLE_ERROR,
            responder: None,
            rtt_ms: 0,
        };
    }
    let rd_u32 =
        |off: usize| u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]);
    let mut octets = [0u8; 16];
    octets.copy_from_slice(&buf[6..22]);
    IcmpReply {
        status: rd_u32(26),
        responder: Some(IpAddr::V6(Ipv6Addr::from(octets))),
        rtt_ms: rd_u32(30),
    }
}

/// 按协议分派的单次探测入口
pub fn ping_once(addr: IpAddr, size: usize, ttl: u8, timeout_ms: u32) -> IcmpReply {
    match addr {
        IpAddr::V4(v4) => ping_v4(v4, size, ttl, timeout_ms),
        IpAddr::V6(v6) => ping_v6(v6, size, ttl, timeout_ms),
    }
}

/// IP_STATUS → 中文描述（错误路径与日志用）
pub fn status_text(status: u32) -> String {
    match status {
        IP_STATUS_SUCCESS => "成功".to_string(),
        11001 => "IP 缓冲不足".to_string(),
        11002 => "目标网络不可达".to_string(),
        11003 => "目标主机不可达".to_string(),
        11004 => "目标协议不可达".to_string(),
        11005 => "目标端口不可达".to_string(),
        11006 => "缺少所需资源".to_string(),
        11007 => "资源不足".to_string(),
        11008 => "主机名未知".to_string(),
        11009 => "需要分片但禁止分片（包过大）".to_string(),
        IP_REQ_TIMED_OUT => "请求超时".to_string(),
        IP_TTL_EXPIRED_TRANSIT => "TTL 在传输中超限".to_string(),
        IP_TTL_EXPIRED_REASSEM => "TTL 在重组中超限".to_string(),
        11015 => "参数错误".to_string(),
        11016 => "缓冲区过小".to_string(),
        11018 => "一般性失败".to_string(),
        ICMP_HANDLE_ERROR => "ICMP 句柄创建失败".to_string(),
        other => format!("IP_STATUS {other}"),
    }
}

/// 探测结果的用户可读一行文案
pub fn reply_line(reply: &IcmpReply) -> String {
    if reply.is_success() {
        format!("成功 · RTT {} ms", reply.rtt_ms)
    } else {
        status_text(reply.status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// IPv4 回环实机验证（IcmpSendEcho 基础链路）
    #[test]
    #[ignore = "实机网络用例：cargo test -- --ignored 运行"]
    fn real_icmp_v4_loopback() {
        let r = ping_v4(Ipv4Addr::LOCALHOST, 32, 128, 2000);
        assert!(r.is_success(), "回环 ICMPv4 应成功: {:?}", r);
        assert_eq!(
            r.responder,
            Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            "响应者应为 127.0.0.1"
        );
    }

    /// IPv6 回环实机验证（Icmp6SendEcho2 + 手工布局解析）
    #[test]
    #[ignore = "实机网络用例：cargo test -- --ignored 运行"]
    fn real_icmp_v6_loopback() {
        let r = ping_v6(Ipv6Addr::LOCALHOST, 32, 128, 2000);
        assert!(r.is_success(), "回环 ICMPv6 应成功: {:?}", r);
        assert_eq!(
            r.responder,
            Some(IpAddr::V6(Ipv6Addr::LOCALHOST)),
            "响应者应为 ::1（验证 SDK 布局手工解析）"
        );
    }

    /// 外网可达性验证（有网环境运行）
    #[test]
    #[ignore = "实机网络用例：cargo test -- --ignored 运行"]
    fn real_icmp_v4_public() {
        // 阿里公共 DNS（ICMP 通常放行）
        let r = ping_v4(Ipv4Addr::new(223, 5, 5, 5), 32, 128, 2000);
        assert!(r.is_success(), "223.5.5.5 应可达: {:?}", r);
    }

    /// 不存在的地址 → 超时路径（IP_REQ_TIMED_OUT）
    #[test]
    #[ignore = "实机网络用例：cargo test -- --ignored 运行"]
    fn real_icmp_v4_timeout_path() {
        // TEST-NET-1（192.0.2.0/24）保留段，常规网络不可达
        let r = ping_v4(Ipv4Addr::new(192, 0, 2, 1), 32, 128, 1000);
        assert!(!r.is_success(), "保留段不应成功");
        assert_eq!(r.status, IP_REQ_TIMED_OUT, "应命中超时状态: {:?}", r);
    }
}
