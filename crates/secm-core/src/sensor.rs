// secm-core::sensor — 统一硬件快照数据契约 v2（ADR-0005）
//
// 架构（ADR-0005 统一 HardwareSnapshot；v3.0.0 起纯原生零 HTTP）：
//   Collector 层（secm-datasource，进程内 Rust 直调）
//     → secm-core::sensor_service（唯一写入者，1s 编排）
//     → SensorSnapshot（本模块，UI 只读）
//     → GPUI（dashboard / 侧栏 / 硬件页）
//
// 语义（ADR-0004/0008）：
// - 可能不可用的测量值一律用 Metric<T> 包装：value=None 表示不可用，
//   禁止 0/默认值冒充真实硬件值（LiteMonitor 历史缺陷不迁移）；
// - source 记录实际来源（回退成功时为回退层，即 fallback_used 追踪）；
// - error 保留最近一次失败诊断（core 保留，UI 可选择隐藏）；
// - 结构稳定的基础域（CPU 占用/内存/容量）保留裸类型，失败由 diag 汇总。
//
// 权限边界（v3.0.0 明确区分"非管理员可用"与"能读全部传感器"）：
// - ring0 专属指标（CPU 温度/功耗/电压、主板 SuperIO）在非管理员 Windows 下
//   物理不可得 → Metric::unavailable 如实标注，不伪造、不提权、不经 HTTP 绕过。

use serde::Serialize;

// ============================================================================
// 来源标识（对齐 ADR-0005 Source 枚举；v3.0.0 原生迁移）
// ============================================================================

/// 数据来源（序列化为稳定字符串；v3.0.0 移除 Lhm——sidecar HTTP 链路已删除）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub enum Source {
    /// NVIDIA NVML 用户态采集（GPU 温度/功耗/时钟/负载/显存；运行时加载驱动 nvml.dll）
    Nvml,
    /// DXGI 适配器枚举（全厂商 GPU 名称/专用显存总量，静态）
    Dxgi,
    /// PawnIO ring0 寄存器直读（CPU 温度：AMD SMN / Intel MSR；需管理员 + PawnIO 2.x）
    PawnIo,
    /// Windows 性能计数器体系（LiteMonitorPerfCounter：PDH % Processor Utility、PhysicalDisk 等）
    PerfCounter,
    /// IPHLPAPI 原生网络（LiteMonitorNativeNetwork：GetIfTable2/GetAdaptersAddresses）
    NativeNetwork,
    /// NtPowerInformation（CPU 频率实时层 + 电池充放电状态 SystemBatteryState）
    NtPower,
    /// 磁盘容量（LiteMonitorDriveInfo：GetDiskFreeSpaceExW 等价）
    DriveInfo,
    /// 注册表（标称频率保底层）
    Registry,
    /// 电池/AC 电源（GetSystemPowerStatus：AC 状态/电量百分比）
    Battery,
    /// SMART 域（NVMe 健康日志温度 = IOCTL 协议特定查询，用户态；SATA 温度需管理员透传）
    Smart,
    /// 不可用（无任何来源产出）
    #[default]
    Unavailable,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Nvml => "nvml",
            Self::Dxgi => "dxgi",
            Self::PawnIo => "pawnio",
            Self::PerfCounter => "perfcounter",
            Self::NativeNetwork => "native_network",
            Self::NtPower => "ntpower",
            Self::DriveInfo => "driveinfo",
            Self::Registry => "registry",
            Self::Battery => "battery",
            Self::Smart => "smart",
            Self::Unavailable => "unavailable",
        }
    }
}

// ============================================================================
// Metric<T> — 统一测量值
// ============================================================================

/// 统一测量值：value=None = 不可用（真实失败语义，不伪造数值）
#[derive(Debug, Clone, Default, Serialize)]
pub struct Metric<T> {
    /// 测量值；None = 不可用（error 说明原因）
    pub value: Option<T>,
    /// 实际来源（回退成功时为回退层 → fallback_used = source 与主路径不同）
    pub source: Source,
    /// 本值采集时间（Unix 毫秒；0 = 从未采集）
    pub updated_at_ms: u64,
    /// 最近一次失败诊断（core 保留；UI 可隐藏）
    pub error: Option<String>,
}

impl<T> Metric<T> {
    /// 可用值
    pub fn available(value: T, source: Source, now_ms: u64) -> Self {
        Self {
            value: Some(value),
            source,
            updated_at_ms: now_ms,
            error: None,
        }
    }

    /// 不可用（附诊断）
    pub fn unavailable(error: impl Into<String>) -> Self {
        Self {
            value: None,
            source: Source::Unavailable,
            updated_at_ms: 0,
            error: Some(error.into()),
        }
    }

    /// 是否可用
    pub fn is_available(&self) -> bool {
        self.value.is_some()
    }

    /// 回退语义标注：主路径失败后由回退层产出时调用
    pub fn fallback(value: T, source: Source, now_ms: u64, reason: impl Into<String>) -> Self {
        Self {
            value: Some(value),
            source,
            updated_at_ms: now_ms,
            error: Some(reason.into()),
        }
    }

    /// 取值映射（可用时）；UI 展示辅助
    pub fn value_or(&self, default: T) -> T
    where
        T: Copy,
    {
        self.value.unwrap_or(default)
    }
}

// ============================================================================
// 域结构 v2
// ============================================================================

/// CPU 数据
#[derive(Debug, Clone, Default, Serialize)]
pub struct CpuData {
    pub name: String,
    /// 总负载 %（PDH % Processor Utility → sysinfo % Processor Time 差分回退）
    pub usage: f32,
    /// 总负载实际来源（PerfCounter = PDH Utility/Time；NtPower = sysinfo 内核计数差分）
    pub usage_source: Source,
    pub per_core: Vec<f32>,
    pub core_count: usize,
    /// 频率（ntapi → PDH → registry 标称降级链）
    pub clock_mhz: Metric<f32>,
    /// Package 温度 ℃（ring0 专属：MSR 需内核驱动；非管理员环境恒 unavailable）
    pub temperature: Metric<f32>,
    /// Package 功耗 W（ring0 专属：RAPL/SMU 需内核驱动；非管理员环境恒 unavailable）
    pub power_w: Metric<f32>,
    /// 核心电压 V（ring0 专属：SuperIO/VRM 遥测需内核驱动；非管理员环境恒 unavailable）
    pub voltage: Metric<f32>,
    /// CPU 风扇转速 RPM（ring0 专属：SuperIO 需内核驱动；见 motherboard.cpu_fan_rpm）
    pub fan_rpm: Metric<f32>,
}

/// GPU 数据（单卡一条；来源 = NVML（NVIDIA 实时）+ DXGI（全厂商名称/显存））
#[derive(Debug, Clone, Default, Serialize)]
pub struct GpuData {
    pub name: String,
    /// 核心负载 %（NVML utilization.gpu；非 NVIDIA 恒 unavailable）
    pub usage: Metric<f32>,
    /// 核心温度 ℃（NVML；非 NVIDIA 无用户态来源 → unavailable）
    pub temperature: Metric<f32>,
    /// 核心时钟 MHz（NVML；非 NVIDIA 恒 unavailable）
    pub clock_mhz: Metric<f32>,
    /// 功耗 W（NVML power_usage；非 NVIDIA 恒 unavailable）
    pub power_w: Metric<f32>,
    /// 显存已用（字节；NVML，非 NVIDIA 恒 unavailable）
    pub vram_used: Metric<u64>,
    /// 显存总量（字节；NVIDIA = NVML，AMD/Intel = DXGI DedicatedVideoMemory）
    pub vram_total: Metric<u64>,
    /// 风扇转速 RPM（NVML 仅提供占空比百分比非 RPM；无用户态 RPM 来源恒 unavailable）
    pub fan_rpm: Metric<f32>,
}

impl GpuData {
    /// 显存占用 %（used/total 计算；任一不可用则不可用——LiteMonitor 同语义）
    pub fn vram_load_pct(&self) -> Metric<f32> {
        match (self.vram_used.value, self.vram_total.value) {
            (Some(u), Some(t)) if t > 0 => {
                let pct = (u as f32 / t as f32 * 100.0).clamp(0.0, 100.0);
                Metric::available(
                    pct,
                    self.vram_used.source,
                    self.vram_used
                        .updated_at_ms
                        .max(self.vram_total.updated_at_ms),
                )
            }
            (None, Some(_)) => Metric::unavailable("显存已用不可用"),
            (Some(_), None) => Metric::unavailable("显存总量不可用"),
            _ => Metric::unavailable("显存传感器不可用"),
        }
    }
}

/// 内存数据（GlobalMemoryStatusEx 等价；结构稳定基础域）
#[derive(Debug, Clone, Default, Serialize)]
pub struct MemoryData {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub usage_percent: f32,
    /// 内存型号（WMI Win32_PhysicalMemory SPD 汇总，v3 原生；无则空串）
    pub model_name: String,
}

/// 磁盘数据（单盘/卷一条）
#[derive(Debug, Clone, Default, Serialize)]
pub struct DiskData {
    /// 卷名（sysinfo mount name）
    pub name: String,
    /// 盘符键（"C:" 大写；PDH/IO 匹配键）
    pub drive_key: String,
    pub total_space: u64,
    pub available_space: u64,
    pub used_space: u64,
    pub usage_percent: f32,
    /// 读速率 MB/s（PDH PhysicalDisk）
    pub read_mbps: Metric<f32>,
    /// 写速率 MB/s（PDH PhysicalDisk）
    pub write_mbps: Metric<f32>,
    /// 活动时间 %（PDH % Disk Time；LiteMonitor DISK.Activity 等价）
    pub activity_pct: Metric<f32>,
    /// 盘温度 ℃（v3 原生：NVMe 健康日志 IOCTL 用户态；SATA 需管理员透传 → unavailable）
    pub temperature: Metric<f32>,
}

/// 主板原始传感器条目（SuperIO 子硬件；v3.0.0 起无数据源，类型保留维持契约稳定）
#[derive(Debug, Clone, Default, Serialize)]
pub struct MotherboardSensor {
    pub name: String,
    /// "temperature" | "fan" | "voltage"
    pub kind: String,
    /// 所属硬件名（SuperIO 已替换为主板名；FanMapper 等价匹配键）
    pub hw: String,
    pub value: f32,
}

/// 主板数据（原始传感器列表 + 智能匹配产出；v3.0.0 起快照恒 None——SuperIO 读
/// 取需 ring0 内核驱动（管理员），非管理员环境不可得，如实不可用不伪造）
#[derive(Debug, Clone, Default, Serialize)]
pub struct MotherboardData {
    pub name: Option<String>,
    /// 原始传感器列表（温度/风扇/电压）
    pub sensors: Vec<MotherboardSensor>,
    /// 系统/主板温度 ℃（LiteMonitor MOBO.Temp 智能策略产出）
    pub system_temp: Metric<f32>,
    /// CPU 风扇 RPM（FanMapper 等价匹配产出）
    pub cpu_fan_rpm: Metric<f32>,
    /// 水泵 RPM（FanMapper Pump 猜想产出）
    pub cpu_pump_rpm: Metric<f32>,
    /// 机箱风扇 RPM（FanMapper 剩余匹配产出）
    pub case_fan_rpm: Metric<f32>,
}

/// 单网卡网络统计（GetIfTable2 差分；原生 IPHLPAPI）
#[derive(Debug, Clone, Default, Serialize)]
pub struct NetIfStat {
    /// 接口别名（GetAdaptersAddresses FriendlyName 同源）
    pub name: String,
    /// 下行速率 KB/s（差分）
    pub rx_kbps: Metric<f32>,
    /// 上行速率 KB/s（差分）
    pub tx_kbps: Metric<f32>,
    /// 累计收发字节（UI 侧可调间隔差分基线；系统启动以来累计）
    pub in_octets: u64,
    pub out_octets: u64,
    /// 链路协商速度（"1 Gbps"；空 = 未协商/未连接）
    pub link_speed: String,
    /// 接口 IPv4（首个；无 = 空。v3.1：网络流量卡"已连接网卡链接信息"展示用）
    pub ipv4: String,
}

/// 网络快照（原生 IPHLPAPI 域；唯一权威来源）
#[derive(Debug, Clone, Default, Serialize)]
pub struct NetSnapshot {
    pub interfaces: Vec<NetIfStat>,
    /// 本机 IPv4（首个非 APIPA 的 Up 网卡）
    pub local_ipv4: String,
    /// 活跃 TCP 连接数（IPv4 ESTABLISHED；纯计数，失败 0）
    pub tcp_established: u32,
    /// 采集诊断（GetIfTable2 失败原因等；空 = 正常）
    pub error: Option<String>,
}

/// 电池数据（v3 原生：SystemBatteryState 充放电 + GetSystemPowerStatus AC 状态）
#[derive(Debug, Clone, Default, Serialize)]
pub struct BatteryData {
    /// 电量 %（SystemBatteryState RemainingCapacity/MaxCapacity，或 GetSystemPowerStatus 百分比）
    pub percent: Metric<f32>,
    /// 功率 W（正=充电输入 / 负=放电输出；SystemBatteryState Rate，用户态）
    pub power_w: Metric<f32>,
    /// 电流 A（Windows 用户态 API 不提供电池电压/电流 → 恒 unavailable，不推算伪造）
    pub current_a: Metric<f32>,
    /// 电压 V（同上：无用户态来源 → 恒 unavailable）
    pub voltage_v: Metric<f32>,
    /// 是否接通外接电源（GetSystemPowerStatus ACLineStatus / SystemBatteryState AcOnLine）
    pub ac_online: bool,
    /// 是否正在充电
    pub charging: bool,
}

/// 磁盘温度快照（v3 原生 NVMe 域；name = 型号 + PhysicalDriveN）
#[derive(Debug, Clone, Default, Serialize)]
pub struct StorageTemp {
    /// 磁盘名（"Samsung SSD 980 PRO（PhysicalDrive0）"；无型号退化为盘号）
    pub name: String,
    /// 盘温度 ℃（NVMe 健康日志；SATA 需管理员透传 → unavailable）
    pub temp: Metric<f32>,
}

/// 传感器全量快照（后台 1s 轮询填充；UI 各页订阅）
#[derive(Debug, Clone, Default, Serialize)]
pub struct SensorSnapshot {
    pub cpu: CpuData,
    pub gpu: Vec<GpuData>,
    pub memory: MemoryData,
    pub disks: Vec<DiskData>,
    pub motherboard: Option<MotherboardData>,
    /// 网络域（唯一权威来源：GetIfTable2 差分）
    pub net: NetSnapshot,
    /// 电池域（无电池/未启用 = None）
    pub battery: Option<BatteryData>,
    /// 磁盘温度列表（v3 原生：NVMe 健康日志，按物理盘展示；DiskData.temperature 为
    /// 卷盘符 → 物理盘关联匹配）
    pub storage_temps: Vec<StorageTemp>,
    /// 诊断串（各数据源降级原因汇总）
    pub diag: String,
}

/// 现在的 Unix 毫秒（时钟异常返回 0，仅影响 updated_at 展示）
pub fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
