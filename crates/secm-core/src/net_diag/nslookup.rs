// net_diag::nslookup — 流式 Nslookup（A / AAAA）
//
// 行为对齐原版 nslookup_streaming（ADR-0001 §3）：
// - 事件序列：info("开始 DNS 查询: …") → dns-record("[X 记录] ip")×N → summary(DnsResult)
// - 记录类型仅支持 A / AAAA（大写匹配，其余报错）
// 实现差异：解析走自研 RFC 1035 客户端（本机 DNS 服务器逐台 + 系统解析器回退），
// server 字段标注实际使用的链路（"系统默认 DNS" / 指定服务器 IP）。

use super::cancel::is_cancelled;
use super::dns_client::{self, QTYPE_A, QTYPE_AAAA};
use super::StreamEvent;

/// 结果契约（与原版一致）
#[derive(Debug, Clone, serde::Serialize)]
pub struct DnsResult {
    pub domain: String,
    pub record_type: String,
    pub records: Vec<String>,
    pub server: String,
}

/// 流式 Nslookup 主流程（阻塞；在后台线程执行）
pub fn nslookup_streaming(
    domain: &str,
    record_type: &str,
    cmd_id: &str,
    emit: &dyn Fn(StreamEvent),
) -> Result<DnsResult, String> {
    let record_type = record_type.to_uppercase();
    if is_cancelled(cmd_id) {
        emit(StreamEvent::text("error", "用户取消"));
        return Err("Cancelled".into());
    }

    let qtype = match record_type.as_str() {
        "A" => QTYPE_A,
        "AAAA" => QTYPE_AAAA,
        other => {
            let msg = format!("不支持的记录类型: {other}");
            emit(StreamEvent::text("error", msg.clone()));
            return Err(msg);
        }
    };

    // 系统 DNS 服务器（展示用；逐台尝试，全部失败回退系统解析器）
    let servers = dns_client::system_dns_servers();
    let server_label = servers
        .first()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "系统默认 DNS".to_string());
    emit(StreamEvent::text(
        "info",
        format!(
            "开始 DNS 查询: {}  记录类型: {}  服务器: {}",
            domain, record_type, server_label
        ),
    ));

    if is_cancelled(cmd_id) {
        emit(StreamEvent::text("error", "用户取消"));
        return Err("Cancelled".into());
    }

    let result: Result<Vec<String>, String> = (|| {
        // 全量记录列表（对齐原版 resolver.ipv4_lookup/ipv6_lookup 多记录语义）
        let ips = dns_client::resolve_system_all(domain, qtype)?;
        Ok(ips.iter().map(|ip| ip.to_string()).collect())
    })();

    let records = match result {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("{}记录查询失败: {}", record_type, e);
            emit(StreamEvent::text("error", msg.clone()));
            return Err(msg);
        }
    };

    for rec in &records {
        emit(StreamEvent::text(
            "dns-record",
            format!("[{} 记录] {}", record_type, rec),
        ));
    }

    let result = DnsResult {
        domain: domain.to_string(),
        record_type: record_type.clone(),
        records,
        server: server_label,
    };
    emit(StreamEvent::json("summary", &result));
    Ok(result)
}
