// secm-core::hardware — 硬件检测（磁盘清单 + S.M.A.R.T 详情）
// Phase 3/5：薄封装 secm-datasource::disk（IOCTL→WMI 三级降级链保留），
// 附加健康告警摘要供 Hardware 页直接消费。

use secm_datasource::disk::{self, DiskSmartData};

use crate::error::CoreError;

/// 磁盘清单（含容量换算字段，UI 直接渲染）
#[derive(Debug, Clone)]
pub struct DiskListItem {
    pub id: String,
    pub model: String,
    pub serial: String,
    pub interface_type: String,
    pub media_type: String,
    pub size_gb: f64,
}

/// 健康告警（Hardware 页每块盘一行摘要）
#[derive(Debug, Clone)]
pub struct DiskHealthSummary {
    /// 是否健康（false = 存在告警/异常）
    pub healthy: bool,
    /// 告警标题（健康时 "正常"）
    pub title: String,
    /// 告警详情（多行/分号分隔）
    pub detail: String,
}

/// 完整 SMART 视图（详情弹窗数据）
#[derive(Debug, Clone)]
pub struct DiskSmartView {
    pub disk: DiskSmartData,
    pub summary: DiskHealthSummary,
}

/// 枚举系统全部物理磁盘
pub fn list_disks() -> Vec<DiskListItem> {
    disk::enumerate_disks()
        .into_iter()
        .map(|d| DiskListItem {
            size_gb: d.size_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            id: d.id,
            model: d.model,
            serial: d.serial,
            interface_type: d.interface_type,
            media_type: d.media_type,
        })
        .collect()
}

/// 读取指定磁盘 SMART（IOCTL → WMI 三级降级链）
pub fn read_smart(disk_id: &str) -> Result<DiskSmartView, CoreError> {
    let data = disk::read_smart(disk_id).map_err(map_err)?;
    let summary = summarize_health(&data);
    Ok(DiskSmartView {
        disk: data,
        summary,
    })
}

/// 健康摘要（按数据来源分别判定）
fn summarize_health(d: &DiskSmartData) -> DiskHealthSummary {
    let mut problems: Vec<String> = Vec::new();

    match d.source.as_str() {
        // ---- IOCTL 全量 ----
        "ioctl" => {
            if let Some(nv) = &d.nvme_health {
                // NVMe：临界警告位 + 寿命
                if nv.critical_warning & 0x01 != 0 {
                    problems.push("温度过高临界警告".to_string());
                }
                if nv.critical_warning & 0x02 != 0 {
                    problems.push("备用空间不足".to_string());
                }
                if nv.critical_warning & 0x04 != 0 {
                    problems.push("可靠性已降级".to_string());
                }
                if nv.critical_warning & 0x08 != 0 {
                    problems.push("介质进入只读模式".to_string());
                }
                if nv.critical_warning & 0x10 != 0 {
                    problems.push("易失性备份失败".to_string());
                }
                if nv.percentage_used >= 90 {
                    problems.push(format!("寿命已用 {}%（建议尽快备份）", nv.percentage_used));
                }
                if nv.media_errors > 0 {
                    problems.push(format!("媒体错误 {} 次", nv.media_errors));
                }
            }
            // ATA：任一属性状态非 OK
            for a in &d.attributes {
                if a.status == "FAILING" || a.status == "FAILED" {
                    problems.push(format!("属性「{}」状态异常（{}）", a.name, a.status));
                }
            }
        }
        // ---- WMI 兜底 ----
        "wmi" => {
            if let Some(w) = &d.wmi_health {
                match w.health_status {
                    1 => problems.push("健康状态：警告".to_string()),
                    2 => problems.push("健康状态：不健康".to_string()),
                    5 => problems.push("健康状态未知".to_string()),
                    _ => {}
                }
                if let Some(errs) = w.read_errors_total {
                    if errs > 0 {
                        problems.push(format!("读取错误 {} 次", errs));
                    }
                }
                if let Some(errs) = w.write_errors_total {
                    if errs > 0 {
                        problems.push(format!("写入错误 {} 次", errs));
                    }
                }
            }
        }
        _ => {}
    }

    if problems.is_empty() {
        DiskHealthSummary {
            healthy: true,
            title: "正常".to_string(),
            detail: "未检测到健康异常".to_string(),
        }
    } else {
        DiskHealthSummary {
            healthy: false,
            title: "存在告警".to_string(),
            detail: problems.join("；"),
        }
    }
}

/// 转换 datasource CollectError → CoreError（保留中文消息语义）
fn map_err(e: secm_datasource::error::CollectError) -> CoreError {
    match e {
        secm_datasource::error::CollectError::WinApi { api, op, detail } => {
            CoreError::WinApi { api, op, detail }
        }
        secm_datasource::error::CollectError::Registry { path, detail } => {
            CoreError::Registry { path, detail }
        }
        secm_datasource::error::CollectError::Http { url, detail } => {
            CoreError::Http { url, detail }
        }
        secm_datasource::error::CollectError::Parse { what, detail } => {
            CoreError::Parse { what, detail }
        }
        secm_datasource::error::CollectError::NeedsAdmin { op } => {
            CoreError::NeedsAdmin { op }
        }
        secm_datasource::error::CollectError::NotFound { what } => {
            CoreError::NotFound { what }
        }
    }
}
