// secm-core::sensor_service — 统一硬件采集服务（ADR-0005/0006；v3.0.0 纯原生零 HTTP）
//
// 架构：后台线程 1s 轮询 → 唯一写入 SensorSnapshot → UI 只读。
// 采集层全部为进程内 Rust 模块直调（secm-datasource），无 HTTP/localhost/JSON
// 反序列化链路（原 LHM sidecar HTTP 客户端已随 v3.0.0 移除）。
// 每指标唯一权威来源（v3.0.0 原生迁移）：
//   CPU.Load   = PDH % Processor Utility（LiteMonitor 主路径）→ sysinfo 差分回退
//   CPU.Clock  = cpu_freq 降级链 ntapi→PDH→registry
//   CPU 温度/功耗/电压 = ring0 专属（MSR/RAPL/SuperIO 需内核驱动）→ 非管理员恒不可用（如实标注）
//   GPU.*      = NVML（NVIDIA 用户态实时）+ DXGI（全厂商名称/显存）— datasource::gpu
//   内存       = sysinfo（GlobalMemoryStatusEx 等价）+ WMI Win32_PhysicalMemory（SPD 型号，静态缓存）
//   磁盘 IO/活动 = PDH PhysicalDisk（LiteMonitor PerfCounter 等价）
//   磁盘容量   = sysinfo Disks（DriveInfo 等价）
//   磁盘温度   = IOCTL NVMe 健康日志（用户态；SATA 需管理员透传 → 不可用）
//   主板/SuperIO = ring0 专属 → 快照恒 None（如实不可用）
//   网络       = GetIfTable2 差分（LiteMonitor Throughput 等价底层）
//   电池       = CallNtPowerInformation(SystemBatteryState) + GetSystemPowerStatus（全用户态）

use crate::sensor::{
    unix_now_ms, BatteryData, CpuData, DiskData, GpuData, MemoryData, Metric, NetIfStat,
    NetSnapshot, SensorSnapshot, Source, StorageTemp,
};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// 采集服务句柄
pub struct SensorService {
    /// 最新快照（共享，UI 读）
    snapshot: Arc<Mutex<SensorSnapshot>>,
    /// 运行标志（false = 停止）
    running: Arc<std::sync::atomic::AtomicBool>,
}

/// 进程级单例（首个访问者触发启动；UI 任意处读取快照）
static GLOBAL: std::sync::OnceLock<Arc<SensorService>> = std::sync::OnceLock::new();

/// 每秒刷新频率（对齐源 1s 轮询）
const POLL_INTERVAL: Duration = Duration::from_millis(1000);
/// 网络速率安全阀：单拍速率超过该值（KB/s）视为计数器异常丢弃
/// （LiteMonitor 流量域 10GB/拍安全阀的速率域等价语义）
const NET_RATE_SANITY_KBPS: f32 = 10_000_000.0;

impl SensorService {
    /// 启动全局采集服务（幂等；多次调用仅首次真正启动）
    pub fn start_once() -> &'static Arc<SensorService> {
        GLOBAL.get_or_init(SensorService::start)
    }

    /// 读取全局最新快照（未启动时返回默认空快照）
    pub fn snapshot() -> SensorSnapshot {
        match GLOBAL.get() {
            Some(svc) => svc.snapshot.lock().clone(),
            None => SensorSnapshot::default(),
        }
    }

    /// 启动后台采集线程（进程生命周期内常驻；无窗口时也保持快照新鲜）
    fn start() -> Arc<Self> {
        let snapshot = Arc::new(Mutex::new(SensorSnapshot::default()));
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let svc = Arc::new(Self {
            snapshot: snapshot.clone(),
            running: running.clone(),
        });

        std::thread::Builder::new()
            .name("sensor-service".into())
            .spawn(move || {
                // sysinfo System 需跨轮询复用（避免每轮全量快照开销）
                let mut sys = sysinfo::System::new();
                loop {
                    if !running.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    let snap = collect_snapshot(&mut sys);
                    *snapshot.lock() = snap;
                    std::thread::sleep(POLL_INTERVAL);
                }
            })
            .expect("spawn sensor-service");
        svc
    }

    /// 停止服务
    pub fn stop(&self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// 网络差分基线（service 内维护；每拍 if_bytes_map → per-NIC 速率）
#[derive(Default)]
struct NetDiffState {
    /// (unix_ms, 别名 → (InOctets, OutOctets))
    prev: Option<(u64, HashMap<String, (u64, u64)>)>,
}

/// 采集一帧统一快照（ADR-0005：Collector → Normalized SensorSnapshot；v3 全原生）
fn collect_snapshot(sys: &mut sysinfo::System) -> SensorSnapshot {
    let now = unix_now_ms();
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    // ---- GPU（NVML + DXGI，进程内直调；零 HTTP）----
    let gpu = collect_gpu(now);

    // ---- CPU ----
    let cpu = collect_cpu(sys, now);

    // ---- 内存（GlobalMemoryStatusEx 等价；WMI SPD 型号静态缓存）----
    let mem_total = sys.total_memory();
    let mem_used = sys.used_memory();
    let mem_avail = mem_total.saturating_sub(mem_used);
    let mem_pct = if mem_total > 0 {
        (mem_used as f64 / mem_total as f64 * 100.0) as f32
    } else {
        0.0
    };
    let memory = MemoryData {
        total: mem_total,
        used: mem_used,
        available: mem_avail,
        usage_percent: mem_pct,
        model_name: spd_model_cached(),
    };

    // ---- 磁盘（容量 sysinfo + IO/活动 PDH + 温度 NVMe 健康日志 5s TTL）----
    let (disks, storage_temps, disk_diag) = collect_disks(now);

    // ---- 网络（GetIfTable2 差分；唯一权威来源）----
    let net = collect_net(now);

    // ---- 电池（SystemBatteryState + GetSystemPowerStatus，全用户态）----
    let battery = collect_battery(now);

    // ---- 诊断串 ----
    let mut diag = format!(
        "CPU={:.1}%({}) FREQ={} MEM={:.0}% disks={} gpu={}",
        cpu.usage,
        cpu.usage_source.as_str(),
        cpu.clock_mhz
            .value
            .map(|v| format!("{:.0}MHz/{}", v, cpu.clock_mhz.source.as_str()))
            .unwrap_or_else(|| "n/a".into()),
        mem_pct,
        disks.len(),
        gpu.len(),
    );
    if !disk_diag.is_empty() {
        diag.push_str(&format!(" DISK[{}]", disk_diag));
    }

    SensorSnapshot {
        cpu,
        gpu,
        memory,
        disks,
        // 主板/SuperIO 域：ring0 专属（需内核驱动 + 管理员），v3 起如实不可用
        motherboard: None,
        net,
        battery,
        storage_temps,
        diag,
    }
}

/// CPU 域采集：负载（PDH Utility → sysinfo 差分回退）+ 频率（cpu_freq 链）；
/// 温度/功耗/电压为 ring0 专属（MSR/RAPL/SuperIO 需内核驱动），非管理员环境
/// 物理不可得 → 如实 unavailable（v3.0.0 原生迁移：不再经 sidecar 提权获取）
fn collect_cpu(sys: &mut sysinfo::System, now: u64) -> CpuData {
    // 负载：LiteMonitor 主路径 PDH % Processor Utility；回退 sysinfo 差分（% Processor Time 语义）
    let (usage, usage_source) = match secm_datasource::cpu_load::get_cpu_load() {
        Some(v) => (v, Source::PerfCounter),
        None => {
            let v = sys.global_cpu_info().cpu_usage().clamp(0.0, 100.0);
            (v, Source::NtPower)
        }
    };
    let per_core: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
    let core_count = per_core.len();
    let name = sys
        .cpus()
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_else(|| "Unknown CPU".into());

    // 频率：cpu_freq 降级链（ntapi → pdh → registry）
    let (freq, freq_src, freq_reason) = secm_datasource::cpu_freq::get_cpu_freq_diag();
    let clock_mhz = match (freq, freq_src) {
        (Some(mhz), "ntapi") => Metric::available(mhz, Source::NtPower, now),
        (Some(mhz), "pdh") => Metric::available(mhz, Source::PerfCounter, now),
        (Some(mhz), "registry") => Metric::available(mhz, Source::Registry, now),
        _ => Metric::unavailable(format!("频率不可用: {}", freq_reason)),
    };

    // 温度：PawnIO ring0 直读（v3.1.0 BUG 修复：管理员 + 已部署 PawnIO 时恢复
    // 真实 CPU 温度；非管理员/未部署 PawnIO → 如实 unavailable，不伪造）
    let temperature = match secm_datasource::cpu_temp::read_cpu_temperature() {
        Ok(t) => Metric::available(t, Source::PawnIo, now),
        Err(e) => Metric::unavailable(format!("CPU 温度不可用：{e}")),
    };
    // 功耗/电压：ring0 专属（RAPL/SMU/SuperIO），本轮未接入 → 如实 unavailable
    const RING0_CPU_REST: &str = "需 ring0 内核寄存器（RAPL/SMU，管理员权限），当前不可用";
    let power_w = Metric::<f32>::unavailable(RING0_CPU_REST);
    let voltage = Metric::<f32>::unavailable(RING0_CPU_REST);

    CpuData {
        name,
        usage,
        usage_source,
        per_core,
        core_count,
        clock_mhz,
        temperature,
        power_w,
        voltage,
        // CPU 风扇在主板域匹配（SuperIO ring0 专属；v3 起如实不可用）
        fan_rpm: Metric::unavailable("CPU 风扇转速需 SuperIO ring0 读取（管理员权限），当前不可用"),
    }
}

/// 磁盘域采集：容量（sysinfo）+ IO/活动（PDH）+ 温度（NVMe 健康日志，5s TTL 缓存）
///
/// 返回 (卷列表, 物理盘温度列表, 磁盘域诊断)。温度关联：卷盘符 →
/// `IOCTL_STORAGE_GET_DEVICE_NUMBER` 物理盘号 → NVMe 温度采样；
/// 非 NVMe（SATA/ATA 温度需管理员透传）→ 如实 unavailable。
fn collect_disks(now: u64) -> (Vec<DiskData>, Vec<StorageTemp>, String) {
    let io_map = secm_datasource::disk_io::get_disk_io_sample_map();
    let (temp_samples, temp_map) = disk_temp_snapshot(now);
    let mut disks = Vec::new();
    let mut diag_unmatched = 0usize;
    for d in sysinfo::Disks::new_with_refreshed_list().list() {
        let total = d.total_space();
        let avail = d.available_space();
        let used = total.saturating_sub(avail);
        let pct = if total > 0 {
            (used as f64 / total as f64 * 100.0) as f32
        } else {
            0.0
        };
        // PDH 实例键为盘符 "C:" 形式，从挂载点提取首字母盘符匹配
        let mount = d.mount_point().to_string_lossy();
        let drive_key = mount
            .chars()
            .next()
            .map(|c| format!("{}:", c.to_ascii_uppercase()))
            .unwrap_or_default();
        let sample = io_map.get(&drive_key);
        let (read_mbps, write_mbps, activity_pct) = match sample {
            Some(s) => (
                Metric::available(s.read_mbps, Source::PerfCounter, now),
                Metric::available(s.write_mbps, Source::PerfCounter, now),
                Metric::available(s.activity_pct, Source::PerfCounter, now),
            ),
            None => (
                Metric::unavailable("PDH 磁盘 IO 通道不可用"),
                Metric::unavailable("PDH 磁盘 IO 通道不可用"),
                Metric::unavailable("PDH % Disk Time 通道不可用"),
            ),
        };
        // 盘温度：卷盘符 → 物理盘号 → NVMe 健康日志温度（全用户态，v3 原生）。
        // 关联失败/非 NVMe → 如实 unavailable（区分诊断文案，不伪造）。
        let temperature = temp_map
            .get(&drive_key)
            .and_then(|pi| temp_samples.iter().find(|s| s.physical_index == *pi))
            .map(|s| (s.temp_c, s.bus.clone()))
            .map(|(temp_c, bus)| match temp_c {
                Some(t) => Metric::available(t, Source::Smart, now),
                None => Metric::unavailable(disk_temp_reason(&bus)),
            })
            .unwrap_or_else(|| {
                diag_unmatched += 1;
                Metric::unavailable("盘温度：卷未关联到物理盘（映射不可得）")
            });

        disks.push(DiskData {
            name: d.name().to_string_lossy().to_string(),
            drive_key,
            total_space: total,
            available_space: avail,
            used_space: used,
            usage_percent: pct,
            read_mbps,
            write_mbps,
            activity_pct,
            temperature,
        });
    }
    // 物理盘温度列表（硬件页/仪表盘独立展示；名称 = 型号 + 物理盘号）
    let storage_temps = temp_samples
        .iter()
        .map(|s| StorageTemp {
            name: if s.model.is_empty() {
                format!("PhysicalDrive{}", s.physical_index)
            } else {
                format!("{}（PhysicalDrive{}）", s.model, s.physical_index)
            },
            temp: s
                .temp_c
                .map(|t| Metric::available(t, Source::Smart, now))
                .unwrap_or_else(|| Metric::unavailable(disk_temp_reason(&s.bus))),
        })
        .collect();

    let diag = if diag_unmatched > 0 {
        format!("{} 个卷未关联物理盘温度", diag_unmatched)
    } else {
        String::new()
    };
    (disks, storage_temps, diag)
}

/// 磁盘温度不可用原因（按总线类型区分，如实标注权限边界）
fn disk_temp_reason(bus: &str) -> String {
    match bus {
        "NVMe" => "NVMe 健康日志查询失败".to_string(),
        "SATA" | "ATA" => {
            "SATA 温度需管理员 SMART 透传（IOCTL_ATA_PASS_THROUGH），非管理员不可用".to_string()
        }
        other => format!("{} 总线不支持温度查询", other),
    }
}

/// 磁盘温度快照缓存（5s TTL：温度为慢变指标，降低 IOCTL 频次；首拍立即采集）
/// 缓存载荷：(采集时刻 Unix ms, 物理盘温度采样, 卷盘符 → 物理盘号)
type DiskTempCacheEntry = (
    u64,
    Vec<secm_datasource::disk::DiskTempSample>,
    HashMap<String, u32>,
);
static DISK_TEMP_CACHE: Mutex<Option<DiskTempCacheEntry>> = Mutex::new(None);
const DISK_TEMP_TTL_MS: u64 = 5000;

/// 读取磁盘温度采样 + 卷→物理盘映射（TTL 缓存命中直接复用）
fn disk_temp_snapshot(
    now: u64,
) -> (
    Vec<secm_datasource::disk::DiskTempSample>,
    HashMap<String, u32>,
) {
    {
        let cache = DISK_TEMP_CACHE.lock();
        if let Some((t, samples, map)) = cache.as_ref() {
            if now.saturating_sub(*t) < DISK_TEMP_TTL_MS {
                return (samples.clone(), map.clone());
            }
        }
    }
    // 进程内直调 datasource（同步 IOCTL，采集线程执行，不触 UI 线程）
    let samples = secm_datasource::disk::disk_temperature_samples();
    let map = secm_datasource::disk::volume_physical_map();
    *DISK_TEMP_CACHE.lock() = Some((now, samples.clone(), map.clone()));
    (samples, map)
}

/// GPU 域采集：NVML（NVIDIA 实时）+ DXGI（全厂商名称/显存），进程内直调
fn collect_gpu(now: u64) -> Vec<GpuData> {
    secm_datasource::gpu::sample_gpus()
        .into_iter()
        .map(|g| {
            // 非 NVIDIA 卡：实时指标无用户态来源（ADLX/IGCL 为厂商专有 SDK）→ 如实 unavailable
            let nv_only = |v: Option<f32>, what: &str| match v {
                Some(x) => Metric::available(x, Source::Nvml, now),
                None => Metric::unavailable(format!(
                    "{}：仅 NVIDIA NVML 提供用户态采集（AMD 需 ADLX / Intel 需 IGCL，均非用户态公开 API）",
                    what
                )),
            };
            GpuData {
                name: g.name.clone(),
                usage: nv_only(g.load_percent, "GPU 负载"),
                temperature: nv_only(g.temperature_c, "GPU 温度"),
                clock_mhz: nv_only(g.core_clock_mhz, "GPU 时钟"),
                power_w: nv_only(g.power_w, "GPU 功耗"),
                vram_used: match g.memory_used_bytes {
                    Some(b) => Metric::available(b, Source::Nvml, now),
                    None => Metric::unavailable("显存已用：仅 NVIDIA NVML 提供用户态采集"),
                },
                vram_total: match g.memory_total_bytes {
                    // NVIDIA 卡取 NVML 值；AMD/Intel 卡取 DXGI DedicatedVideoMemory（全厂商可用）
                    Some(b) if g.load_percent.is_some() || g.temperature_c.is_some() => {
                        Metric::available(b, Source::Nvml, now)
                    }
                    Some(b) => Metric::available(b, Source::Dxgi, now),
                    None => Metric::unavailable("显存总量不可得（DXGI/NVML 均未提供）"),
                },
                // NVML 风扇为占空比百分比、非 RPM → 无真实 RPM 来源，不伪造
                fan_rpm: Metric::unavailable(
                    "GPU 风扇 RPM 无用户态来源（NVML 仅提供占空比百分比）",
                ),
            }
        })
        .collect()
}

/// 内存 SPD 型号（WMI Win32_PhysicalMemory；静态数据仅查询一次缓存）
fn spd_model_cached() -> String {
    static SPD: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    SPD.get_or_init(|| match secm_datasource::memory::spd_model_summary() {
        Some(s) => {
            log::info!("传感器 · 内存 SPD 型号（WMI）：{}", s);
            Some(s)
        }
        None => {
            log::info!("传感器 · 内存 SPD 型号不可得（WMI 查询无结果）");
            None
        }
    })
    .clone()
    .unwrap_or_default()
}

/// 网络域采集：GetIfTable2 差分（per-NIC 速率 + 累计字节）+ 本地 IPv4 + TCP 连接数
fn collect_net(now: u64) -> NetSnapshot {
    let bytes = secm_datasource::netif::if_bytes_map();
    let t = now;

    // per-NIC IPv4（GetAdaptersAddresses 一次取全；网络流量卡"已连接网卡链接信息"用）
    let ipv4_map: HashMap<String, String> = secm_datasource::netif::adapter_configs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|a| {
            let ip = a.ipv4.iter().find(|ip| !ip.starts_with("169.254."))?;
            Some((a.name.clone(), ip.clone()))
        })
        .collect();

    let mut state = NET_DIFF.lock();
    let rates: HashMap<String, (f32, f32)> = match state.prev {
        Some((pt, ref prev)) if t > pt => {
            let dt = (t - pt) as f32 / 1000.0;
            let mut acc = HashMap::new();
            for (name, (rx, tx)) in &bytes {
                if let Some((prx, ptx)) = prev.get(name) {
                    // 仅累计递增有效（计数器回绕/重置 → 本拍丢弃，LiteMonitor 安全阀语义）
                    let drx = if *rx >= *prx {
                        (*rx - *prx) as f32 / dt / 1024.0
                    } else {
                        0.0
                    };
                    let dtx = if *tx >= *ptx {
                        (*tx - *ptx) as f32 / dt / 1024.0
                    } else {
                        0.0
                    };
                    // 速率安全阀：>10GB/s 视为异常（物理不可达）
                    let drx = if drx.is_finite() && drx <= NET_RATE_SANITY_KBPS {
                        drx
                    } else {
                        0.0
                    };
                    let dtx = if dtx.is_finite() && dtx <= NET_RATE_SANITY_KBPS {
                        dtx
                    } else {
                        0.0
                    };
                    acc.insert(name.clone(), (drx, dtx));
                }
            }
            acc
        }
        _ => HashMap::new(), // 首拍仅建立基线
    };
    state.prev = Some((t, bytes.clone()));
    drop(state);

    // 链路速度（准静态；GetIfTable2 TransmitLinkSpeed）
    let speeds = secm_datasource::netif::link_speeds().unwrap_or_default();

    // 虚拟/过滤层接口过滤（LiteMonitor _virtualNicKW 等价 + Windows 组件层模式——
    // QoS 包调度器 / 过滤驱动层为同一物理卡的软件层实例，按位域 + 关键词双重剔除）
    let is_virtual = |name: &str| -> bool {
        let lower = name.to_ascii_lowercase();
        const KW: [&str; 21] = [
            "virtual",
            "vmware",
            "hyper-v",
            "hyper v",
            "vbox",
            "loopback",
            "tunnel",
            "tap",
            "tun",
            "bluetooth",
            "zerotier",
            "tailscale",
            "wan miniport",
            "wfp ",
            "ndis capture",
            "qos packet scheduler",
            "filter driver",
            "本地连接*",
            "wi-fi direct",
            "npcap",
            "6to4",
        ];
        KW.iter().any(|k| lower.contains(k))
            || name.contains("LightWeight Filter")
            || name.contains("Filter-0000")
            || name.contains("Filter-0001")
    };

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut interfaces: Vec<NetIfStat> = bytes
        .iter()
        .filter(|(name, _)| !is_virtual(name) && seen.insert(name.to_ascii_uppercase()))
        .map(|(name, (in_oct, out_oct))| {
            let (rx, tx) = rates.get(name).copied().unwrap_or((0.0, 0.0));
            NetIfStat {
                name: name.clone(),
                rx_kbps: Metric::available(rx, Source::NativeNetwork, t),
                tx_kbps: Metric::available(tx, Source::NativeNetwork, t),
                in_octets: *in_oct,
                out_octets: *out_oct,
                link_speed: speeds.get(name).cloned().unwrap_or_default(),
                ipv4: ipv4_map.get(name).cloned().unwrap_or_default(),
            }
        })
        .collect();

    // 稳定展示顺序：按名称升序（HashMap 迭代序不稳定，防流量卡每秒行抖动）
    interfaces.sort_by_key(|i| i.name.to_ascii_lowercase());

    // 本地 IPv4（首个非 APIPA 的 Up 网卡；GetAdaptersAddresses）
    let local_ipv4 = local_ipv4_from_adapters();

    NetSnapshot {
        interfaces,
        local_ipv4,
        tcp_established: secm_datasource::netif::tcp_connection_count(),
        error: None,
    }
}

static NET_DIFF: Mutex<NetDiffState> = Mutex::new(NetDiffState { prev: None });

/// 首个非 APIPA 的 Up 网卡 IPv4（net_info 同语义；此处供快照内嵌）
fn local_ipv4_from_adapters() -> String {
    let Ok(adapters) = secm_datasource::netif::adapter_configs() else {
        return String::new();
    };
    let is_apipa = |ip: &str| ip.starts_with("169.254.");
    let up: Vec<_> = adapters.iter().filter(|a| a.status == "Up").collect();
    let main = up
        .iter()
        .copied()
        .find(|a| a.ipv4.iter().any(|ip| !is_apipa(ip)))
        .or_else(|| up.first().copied());
    main.and_then(|a| a.ipv4.iter().find(|ip| !is_apipa(ip)).cloned())
        .unwrap_or_default()
}

/// 电池域采集（v3 全用户态原生）：
/// - SystemBatteryState（NtPower）：电量推算 / 充放电功率（符号由 Charging/Discharging
///   标志直接给出，无需启发式修正）/ AC 状态；
/// - GetSystemPowerStatus（Battery）：电量百分比回退 + 充电标志；
/// - 电压/电流：Windows 用户态 API 不提供 → 恒 unavailable（不推算伪造）。
fn collect_battery(now: u64) -> Option<BatteryData> {
    let state = secm_datasource::power::get_battery_state();
    let power = secm_datasource::power::get_power_status();

    // 无电池判定：SystemBatteryState 无电池 且 GetSystemPowerStatus 无电量百分比
    let battery_present =
        state.is_some() || power.map(|p| p.battery_percent.is_some()).unwrap_or(false);
    if !battery_present {
        return None;
    }

    // AC/充电状态：SystemBatteryState 优先，GetSystemPowerStatus 回退
    let (ac_online, charging) = match (&state, &power) {
        (Some(s), _) => (s.ac_online, s.charging),
        (None, Some(p)) => (p.ac_online, p.charging),
        (None, None) => return None,
    };

    // 电量 %：SystemBatteryState 容量推算优先（NtPower），GetSystemPowerStatus 回退（Battery）
    let percent = match (&state, &power) {
        (Some(s), _) if s.percent.is_some() => {
            Metric::available(s.percent.unwrap(), Source::NtPower, now)
        }
        (_, Some(p)) if p.battery_percent.is_some() => {
            Metric::available(p.battery_percent.unwrap() as f32, Source::Battery, now)
        }
        _ => Metric::unavailable("电池电量不可得（容量字段无效）"),
    };

    // 充放电功率 W（SystemBatteryState.Rate，符号已按充放电标志给出）
    let power_w = state
        .and_then(|s| s.power_w)
        .map(|v| Metric::available(v, Source::NtPower, now))
        .unwrap_or_else(|| {
            Metric::unavailable("充放电功率不可得（SystemBatteryState 无有效速率）")
        });

    Some(BatteryData {
        percent,
        power_w,
        current_a: Metric::unavailable("Windows 用户态 API 不提供电池电流"),
        voltage_v: Metric::unavailable("Windows 用户态 API 不提供电池电压"),
        ac_online,
        charging,
    })
}
