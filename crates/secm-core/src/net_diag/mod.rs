// secm-core::net_diag — 网络诊断引擎（ADR-0002，纯 Rust + Windows 原生 API）
//
// 迁移自原 Tauri 实现（youridol/sysenv-console-manager src-tauri）：
//   - ping / traceroute / nslookup / NAT(RFC 3489) / iperf3 / DHCP 探测与深度检查 / 取消注册表
// 与原实现的差异（增强，见 ADR-0001 §5）：
//   - ICMP 改用 iphlpapi IcmpSendEcho / Icmp6SendEcho2（系统态 ICMP，非管理员可用；
//     TTL 真实生效；Time Exceeded 由系统返回路由器地址，无需自解析内嵌 IP 头）
//   - DNS 为自研 RFC 1035 UDP 客户端（替代 trust-dns-resolver，去 tokio 依赖）
//   - TCP/UDP ping 真实现（原版 UI 有 proto 开关但后端静默忽略）
//   - iperf3 取消即 kill（原版依赖 stdout 逐行检查，长时间无输出时取消延迟）
//   - DHCP 探测/深度检查补取消（原版无）
// 事件契约（kind/data 字段与原版逐字段一致，ADR-0001 §3）。

pub mod cancel;
pub mod dhcp_probe;
pub mod dns_client;
pub mod icmp;
pub mod iperf3;
pub mod nat;
pub mod nslookup;
pub mod ping;
pub mod sites;
pub mod traceroute;

/// 流式诊断事件（对应原版 Tauri Channel<StreamEvent> 契约）
///
/// - `kind`：事件类型（"ping" / "trace-hop" / "dns-record" / "info" / "stun-step" /
///   "summary" / "error"，GPUI 页面侧另用 "done" 作为任务结束哨兵）
/// - `data`：JSON 负载或纯文本
#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamEvent {
    pub kind: String,
    pub data: String,
}

impl StreamEvent {
    /// 纯文本事件
    pub fn text(kind: &str, data: impl Into<String>) -> Self {
        Self {
            kind: kind.to_string(),
            data: data.into(),
        }
    }

    /// JSON 负载事件（序列化失败降级为原文，不中断事件流）
    pub fn json<T: serde::Serialize>(kind: &str, payload: &T) -> Self {
        Self {
            kind: kind.to_string(),
            data: serde_json::to_string(payload).unwrap_or_default(),
        }
    }
}

/// GPUI 页面侧任务结束哨兵事件（非原版契约，仅本重构内部使用）
pub const KIND_DONE: &str = "done";
