//! 内存条静态信息采集（WMI Win32_PhysicalMemory，普通用户可读）
//!
//! v3.0.0 原生迁移：内存 SPD 型号原经 LHM sidecar 读取 DIMM 节点名，现改由
//! root\CIMV2 `Win32_PhysicalMemory`（SMBIOS 表）直接查询——普通用户可读，
//! 数据为准静态（插拔内存/重启才变化），调用方应缓存。
//!
//! 数据字段：PartNumber（型号）、Manufacturer（厂商）、Capacity（字节）、
//! SMBIOSMemoryType（代际）。汇总格式与 LHM 时代对齐（如 "2x16GB DDR5 威刚 ..."）。
//!
//! 线程模型：WMI 查询为几十-几百毫秒级同步调用，调用方须在后台线程执行并缓存（S8）。

use serde::Deserialize;

/// WMI 查询结果：Win32_PhysicalMemory（字段名与 WMI 属性 PascalCase 一致）
#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
struct WmiPhysicalMemory {
    #[serde(default)]
    PartNumber: Option<String>,
    #[serde(default)]
    Manufacturer: Option<String>,
    #[serde(default)]
    Capacity: Option<u64>,
    /// SMBIOS 内存代际（20=DDR 21=DDR2 24=DDR3 26=DDR4 34=DDR5；其他 → 未知）
    #[serde(default)]
    SMBIOSMemoryType: Option<u16>,
}

/// SMBIOS 内存代际 → 标签
fn smbios_type_label(t: Option<u16>) -> &'static str {
    match t {
        Some(20) => "DDR",
        Some(21) => "DDR2",
        Some(24) => "DDR3",
        Some(26) => "DDR4",
        Some(34) => "DDR5",
        _ => "内存",
    }
}

/// 清理 WMI 字符串（PartNumber 常带 NUL 填充 + 尾随空格；先去 NUL 再 trim）
fn clean_wmi_str(s: &Option<String>) -> String {
    s.as_deref()
        .unwrap_or("")
        .replace('\0', "")
        .trim()
        .to_string()
}

/// 汇总系统内存条型号（如 "2x16GB DDR5 AX5U6400W52A"）
///
/// 无内存条记录 / WMI 不可用 → None（上层展示空串，不伪造）。
/// 汇总规则：条数 × 单条容量 + 代际 + 首个非空 PartNumber（异构条取首个）。
pub fn spd_model_summary() -> Option<String> {
    let conn = wmi::WMIConnection::new().ok()?;
    let sticks: Vec<WmiPhysicalMemory> = conn
        .raw_query::<WmiPhysicalMemory>(
            "SELECT PartNumber,Manufacturer,Capacity,SMBIOSMemoryType FROM Win32_PhysicalMemory",
        )
        .ok()?;

    if sticks.is_empty() {
        return None;
    }

    // 条数与单条容量（按众数容量算条数，异构容量场景以首条为准，不虚报）
    let per_bytes = sticks.iter().find_map(|s| s.Capacity).unwrap_or(0);
    let count = sticks.len();
    let gen = smbios_type_label(sticks.first().and_then(|s| s.SMBIOSMemoryType));
    // 型号：首个非空 PartNumber，其次 Manufacturer
    let part = sticks
        .iter()
        .map(|s| clean_wmi_str(&s.PartNumber))
        .find(|p| !p.is_empty())
        .or_else(|| {
            sticks
                .iter()
                .map(|s| clean_wmi_str(&s.Manufacturer))
                .find(|m| !m.is_empty())
        })
        .unwrap_or_default();

    let mut out = String::new();
    if per_bytes > 0 {
        out.push_str(&format!(
            "{}x{:.0}GB",
            count,
            per_bytes as f64 / 1024.0 / 1024.0 / 1024.0
        ));
    }
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(gen);
    if !part.is_empty() {
        out.push(' ');
        out.push_str(&part);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smbios_type_label() {
        assert_eq!(smbios_type_label(Some(26)), "DDR4");
        assert_eq!(smbios_type_label(Some(34)), "DDR5");
        assert_eq!(smbios_type_label(None), "内存");
        assert_eq!(smbios_type_label(Some(0)), "内存");
    }

    #[test]
    fn test_clean_wmi_str() {
        assert_eq!(
            clean_wmi_str(&Some("AX5U6400W52A\0      ".to_string())),
            "AX5U6400W52A"
        );
        assert_eq!(clean_wmi_str(&None), "");
    }
}
