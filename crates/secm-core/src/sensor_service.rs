// secm-core::sensor_service — 统一硬件采集服务（ADR-0005/0006）
//
// 架构：后台线程 1s 轮询 → 唯一写入 HardwareSnapshot → UI 只读。
// 每指标唯一权威来源（audit/metric-mapping.md）：
//   CPU.Load   = PDH % Processor Utility（LiteMonitor 主路径）→ sysinfo 差分回退
//   CPU.Clock  = cpu_freq 降级链 ntapi→PDH→registry
//   温度/功耗/电压/风扇/GPU/主板/磁盘温度/电池 = sidecar LHM
//   内存       = sysinfo（GlobalMemoryStatusEx 等价）
//   磁盘 IO/活动 = PDH PhysicalDisk（LiteMonitor PerfCounter 等价）
//   磁盘容量   = sysinfo Disks（DriveInfo 等价）
//   网络       = GetIfTable2 差分（LiteMonitor LHM Throughput 等价底层）
//   AC 状态    = GetSystemPowerStatus（LiteMonitor PowerStatus 等价）
// 智能匹配（MOBO.Temp 策略 / FanMapper / 电池符号）= sensor_match。

use crate::lhm::LhmSensorResponse;
use crate::sensor::{
    unix_now_ms, BatteryData, CpuData, DiskData, GpuData, MemoryData, Metric, MotherboardData,
    MotherboardSensor, NetIfStat, NetSnapshot, SensorSnapshot, Source,
};
use crate::sensor_match;
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

/// 采集一帧统一快照（ADR-0005：Collector → Normalized HardwareSnapshot）
fn collect_snapshot(sys: &mut sysinfo::System) -> SensorSnapshot {
    let now = unix_now_ms();
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    // ---- LHM sidecar（温度/功耗/电压/GPU/主板/磁盘温度/电池；低频 ensure + 2s 缓存）----
    ensure_lhm_periodic();
    let (lhm_resp, lhm_diag) = crate::lhm::snapshot();
    let lhm_ok = matches!(&lhm_resp, Some(r) if r.available);

    // ---- CPU ----
    let cpu = collect_cpu(sys, lhm_resp.as_ref(), &lhm_diag, now);

    // ---- 内存（GlobalMemoryStatusEx 等价；LHM SPD 型号补充）----
    let mem_total = sys.total_memory();
    let mem_used = sys.used_memory();
    let mem_avail = mem_total.saturating_sub(mem_used);
    let mem_pct = if mem_total > 0 {
        (mem_used as f64 / mem_total as f64 * 100.0) as f32
    } else {
        0.0
    };
    let mut memory = MemoryData {
        total: mem_total,
        used: mem_used,
        available: mem_avail,
        usage_percent: mem_pct,
        model_name: String::new(),
    };
    if let Some(r) = &lhm_resp {
        if let Some(m) = &r.memory {
            if let Some(n) = &m.name {
                if !n.is_empty() {
                    memory.model_name = n.clone();
                }
            }
        }
    }

    // ---- GPU（LHM）----
    let gpu: Vec<GpuData> = match lhm_resp.as_ref() {
        Some(r) => r
            .gpu
            .iter()
            .map(|g| GpuData {
                name: g.name.clone(),
                usage: opt_metric(g.load_percent, now, "GPU 负载传感器不可用"),
                temperature: opt_metric(g.temperature_c, now, "GPU 温度传感器不可用"),
                clock_mhz: opt_metric(g.core_clock_mhz, now, "GPU 时钟传感器不可用"),
                power_w: opt_metric(g.power_w, now, "GPU 功耗传感器不可用"),
                vram_used: opt_metric_u64(
                    g.memory_used_bytes.map(|b| b as u64),
                    now,
                    "显存已用传感器不可用",
                ),
                vram_total: opt_metric_u64(
                    g.memory_total_bytes.map(|b| b as u64),
                    now,
                    "显存总量传感器不可用",
                ),
                fan_rpm: opt_metric(g.fan_rpm, now, "GPU 风扇传感器不可用"),
            })
            .collect(),
        None => Vec::new(),
    };

    // ---- 主板（LHM 原始列表 + 智能匹配产出）----
    let motherboard = lhm_resp
        .as_ref()
        .and_then(|r| r.motherboard.as_ref())
        .map(|m| collect_motherboard(m, now));

    // ---- 磁盘（容量 sysinfo + IO/活动 PDH + 温度 LHM Storage）----
    let disks = collect_disks(sys, lhm_resp.as_ref(), now);

    // ---- 网络（GetIfTable2 差分；唯一权威来源）----
    let net = collect_net(now);

    // ---- 电池（LHM Battery 原始值 + AC 符号修正）----
    let battery = collect_battery(lhm_resp.as_ref(), now);

    // ---- 诊断串 ----
    let mut diag = format!(
        "CPU={:.1}%({}) FREQ={} MEM={:.0}% disks={}",
        cpu.usage,
        cpu.usage_source.as_str(),
        cpu.clock_mhz
            .value
            .map(|v| format!("{:.0}MHz/{}", v, cpu.clock_mhz.source.as_str()))
            .unwrap_or_else(|| "n/a".into()),
        mem_pct,
        disks.len()
    );
    if !lhm_ok && !lhm_diag.is_empty() {
        diag.push_str(&format!(" LHM=n/a({})", lhm_diag));
    }

    SensorSnapshot {
        cpu,
        gpu,
        memory,
        disks,
        motherboard,
        net,
        battery,
        diag,
    }
}

/// CPU 域采集：负载（PDH Utility → sysinfo 差分回退）+ 频率（cpu_freq 链）+ LHM 温度/功耗/电压
fn collect_cpu(
    sys: &mut sysinfo::System,
    lhm: Option<&LhmSensorResponse>,
    lhm_diag: &str,
    now: u64,
) -> CpuData {
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

    // LHM 温度/功耗/电压（sidecar 已做 package 匹配与电压排除规则）
    let unavail = || Metric::unavailable(lhm_unavailable_reason(lhm, lhm_diag));
    let (temperature, power_w, voltage) = match lhm {
        Some(r) if r.available => (
            opt_metric(r.cpu.package_temp_c, now, "CPU 温度传感器不可用"),
            opt_metric(r.cpu.power_w, now, "CPU 功耗传感器不可用"),
            opt_metric(r.cpu.voltage_v, now, "CPU 电压传感器不可用"),
        ),
        _ => (unavail(), unavail(), unavail()),
    };

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
        // CPU 风扇在主板域匹配（sensor_match::match_fans），由快照消费者读 motherboard.cpu_fan_rpm
        fan_rpm: Metric::unavailable("见 motherboard.cpu_fan_rpm"),
    }
}

/// LHM 不可用原因（快照域错误文本）
fn lhm_unavailable_reason(lhm: Option<&LhmSensorResponse>, diag: &str) -> String {
    if let Some(r) = lhm {
        if let Some(e) = &r.error {
            return format!("LHM 不可用: {}", e);
        }
    }
    if !diag.is_empty() {
        return format!("LHM 不可用: {}", diag);
    }
    "LHM 不可用（sidecar 未就绪）".into()
}

/// 主板域采集：原始传感器列表 + MOBO.Temp 策略 + FanMapper 等价匹配
fn collect_motherboard(m: &crate::lhm::LhmMotherboardData, now: u64) -> MotherboardData {
    let sensors: Vec<MotherboardSensor> = m
        .sensors
        .iter()
        .map(|s| MotherboardSensor {
            name: s.name.clone(),
            kind: s.kind.clone(),
            hw: s.hw.clone(),
            value: s.value,
        })
        .collect();

    // MOBO.Temp（LiteMonitor SensorMap 智能策略）+ 硬上限校验
    let system_temp = sensor_match::match_system_temp(&sensors)
        .and_then(sensor_match::validate_mobo_temp)
        .map(|v| Metric::available(v, Source::Lhm, now))
        .unwrap_or_else(|| Metric::unavailable("主板温度传感器未匹配（需 LHM + ring0 驱动）"));

    // 风扇/水泵（LiteMonitor FanMapper 等价）
    let fm = sensor_match::match_fans(&sensors);
    let fan_metric = |v: Option<f32>| match v {
        Some(rpm) => Metric::available(rpm, Source::Lhm, now),
        None => Metric::unavailable("风扇传感器未匹配（需 LHM + ring0 驱动）"),
    };

    MotherboardData {
        name: m.name.clone(),
        sensors,
        system_temp,
        cpu_fan_rpm: fan_metric(fm.cpu_fan),
        cpu_pump_rpm: fan_metric(fm.cpu_pump),
        case_fan_rpm: fan_metric(fm.case_fan),
    }
}

/// 磁盘域采集：容量（sysinfo）+ IO/活动（PDH）+ 温度（LHM Storage）
fn collect_disks(
    _sys: &mut sysinfo::System,
    lhm: Option<&LhmSensorResponse>,
    now: u64,
) -> Vec<DiskData> {
    let io_map = secm_datasource::disk_io::get_disk_io_sample_map();
    let mut disks = Vec::new();
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
        // 盘温度：LHM Storage（30s 慢速刷新；名称匹配 sysinfo 卷名不可靠，按序对位由
        // UI 层展示全列表——此处以盘符无法对应 LHM Storage 名，温度暂挂全局列表）
        // 设计：DiskData.temperature 仅在 drive_key 与 LHM storage 名含该盘符时填充；
        // LHM Storage 名一般含型号不含盘符 → 温度主要在 motherboard/独立列表展示。
        // 为保证"每指标唯一来源"，温度在此按名称尽力匹配（无法匹配 → 不可用，不伪造）。
        let temperature = lhm
            .and_then(|r| {
                r.storage.iter().find(|st| {
                    !st.name.is_empty()
                        && (mount.contains(&st.name) || st.name.contains(&drive_key))
                })
            })
            .and_then(|st| st.temp_c)
            .map(|t| Metric::available(t, Source::Lhm, now))
            .unwrap_or_else(|| Metric::unavailable("LHM Storage 温度未匹配（30s 慢速刷新）"));

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
    disks
}

/// 网络域采集：GetIfTable2 差分（per-NIC 速率 + 累计字节）+ 本地 IPv4 + TCP 连接数
fn collect_net(now: u64) -> NetSnapshot {
    let bytes = secm_datasource::netif::if_bytes_map();
    let t = now;

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

    let interfaces: Vec<NetIfStat> = bytes
        .iter()
        .map(|(name, (in_oct, out_oct))| {
            let (rx, tx) = rates.get(name).copied().unwrap_or((0.0, 0.0));
            NetIfStat {
                name: name.clone(),
                rx_kbps: Metric::available(rx, Source::NativeNetwork, t),
                tx_kbps: Metric::available(tx, Source::NativeNetwork, t),
                in_octets: *in_oct,
                out_octets: *out_oct,
                link_speed: speeds.get(name).cloned().unwrap_or_default(),
            }
        })
        .collect();

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

/// 电池域采集：LHM Battery 原始值 + AC 状态符号修正（LiteMonitor BatteryService 等价）
fn collect_battery(lhm: Option<&LhmSensorResponse>, now: u64) -> Option<BatteryData> {
    let bat = lhm?.battery.clone()?;
    // AC 状态（GetSystemPowerStatus；失败沿用上次值由静态缓存处理——此处直接读，
    // 失败时按 ac_online=false 保守处理会误标放电 → 失败时不修正符号更诚实）
    let power = secm_datasource::power::get_power_status();
    let (ac_online, charging) = match power {
        Some(p) => (p.ac_online, p.charging),
        None => (true, false), // API 失败：不修正符号（abs 语义），标注为插电（多数台式机场景）
    };
    Some(BatteryData {
        percent: opt_metric(bat.percent, now, "电池电量传感器不可用"),
        power_w: bat
            .power_w
            .map(|v| {
                Metric::available(
                    sensor_match::fix_battery_sign(v, ac_online),
                    Source::Lhm,
                    now,
                )
            })
            .unwrap_or_else(|| Metric::unavailable("电池功率传感器不可用")),
        current_a: bat
            .current_a
            .map(|v| {
                Metric::available(
                    sensor_match::fix_battery_sign(v, ac_online),
                    Source::Lhm,
                    now,
                )
            })
            .unwrap_or_else(|| Metric::unavailable("电池电流传感器不可用")),
        voltage_v: opt_metric(bat.voltage_v, now, "电池电压传感器不可用"),
        ac_online,
        charging,
    })
}

/// Option<f32> → Metric（None → 不可用 + 诊断）
fn opt_metric(v: Option<f32>, now: u64, err: &str) -> Metric<f32> {
    match v {
        Some(x) if x.is_finite() => Metric::available(x, Source::Lhm, now),
        _ => Metric::unavailable(err),
    }
}

/// Option<u64> → Metric（None → 不可用 + 诊断）
fn opt_metric_u64(v: Option<u64>, now: u64, err: &str) -> Metric<u64> {
    match v {
        Some(x) => Metric::available(x, Source::Lhm, now),
        None => Metric::unavailable(err),
    }
}

/// LHM 低频 ensure（10s 节流：探测/启动开销不随 1s 轮询放大）
///
/// ensure_running 内部 wait_health 最长阻塞 ~10s（P1-4），故派发到独立线程执行，
/// 不冻结 1s 采集线程；LAST 在派发时即推进，本次失败由下个 10s 窗口重试。
fn ensure_lhm_periodic() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let last = LAST.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= 10 {
        LAST.store(now, Ordering::Relaxed);
        // 派发失败（线程资源耗尽，极罕见）忽略：下个窗口重试
        let _ = std::thread::Builder::new()
            .name("lhm-ensure".into())
            .spawn(crate::lhm::ensure_running);
    }
}
