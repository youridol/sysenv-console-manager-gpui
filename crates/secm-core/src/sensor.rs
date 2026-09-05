// secm-core::sensor — 传感器快照数据契约（对齐源 v1.19.0 SensorData JSON 结构）
// 这些类型是 UI 层（Dashboard 等）与采集层的共享数据接口。

use serde::Serialize;

/// CPU 数据（前端契约字段全量保留）
#[derive(Debug, Clone, Default, Serialize)]
pub struct CpuData {
    pub name: String,
    pub usage: f32,
    pub per_core: Vec<f32>,
    pub core_count: usize,
    pub clock_mhz: f32,
    /// 频率数据源: "ntapi" | "pdh" | "registry" | "sysinfo" | "none"
    pub freq_source: String,
    pub temperature: f32,
    pub power_w: f32,
    /// 温度数据源: "lhm" | "winring0" | "acpi" | "none"
    pub temp_source: String,
    /// 功耗数据源: "rapl" | "estimated" | "none"
    pub power_source: String,
    /// 功耗是否估算（前端显示"估算"角标）
    pub power_estimated: bool,
}

/// GPU 数据
#[derive(Debug, Clone, Default, Serialize)]
pub struct GpuData {
    pub name: String,
    pub usage: f32,
    pub temperature: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub clock_mhz: f32,
    pub power_w: f32,
}

/// 内存数据
#[derive(Debug, Clone, Default, Serialize)]
pub struct MemoryData {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub usage_percent: f32,
    /// 内存型号（LHM SPD 补充；无则空串）
    pub model_name: String,
}

/// 磁盘数据
#[derive(Debug, Clone, Default, Serialize)]
pub struct DiskData {
    pub name: String,
    pub total_space: u64,
    pub available_space: u64,
    pub used_space: u64,
    pub usage_percent: f32,
    pub read_mbps: f32,
    pub write_mbps: f32,
}

/// LHM 主板传感器（对齐源 LhmMotherboardData）
#[derive(Debug, Clone, Default, Serialize)]
pub struct MotherboardSensor {
    pub name: String,
    /// "temperature" | "fan" | "voltage"
    pub kind: String,
    pub value: f32,
}

/// 主板数据
#[derive(Debug, Clone, Default, Serialize)]
pub struct MotherboardData {
    pub name: Option<String>,
    pub sensors: Vec<MotherboardSensor>,
}

/// 传感器全量快照（后台 1s 轮询填充；UI 各页订阅）
#[derive(Debug, Clone, Default, Serialize)]
pub struct SensorSnapshot {
    pub cpu: CpuData,
    pub gpu: Vec<GpuData>,
    pub memory: MemoryData,
    pub disks: Vec<DiskData>,
    pub motherboard: Option<MotherboardData>,
    /// 诊断串（各数据源降级原因，格式对齐源 diag）
    pub diag: String,
}

/// 温度源标记（对齐源枚举）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempSource {
    Lhm,
    WinRing0,
    Acpi,
    None,
}

impl TempSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Lhm => "lhm",
            Self::WinRing0 => "winring0",
            Self::Acpi => "acpi",
            Self::None => "none",
        }
    }
}

/// 频率源标记
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreqSource {
    Ntapi,
    Pdh,
    Registry,
    Sysinfo,
    None,
}

impl FreqSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ntapi => "ntapi",
            Self::Pdh => "pdh",
            Self::Registry => "registry",
            Self::Sysinfo => "sysinfo",
            Self::None => "none",
        }
    }
}
