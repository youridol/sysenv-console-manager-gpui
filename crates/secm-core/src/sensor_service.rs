// secm-core::sensor_service — 传感器常驻采集服务（后台线程 1s 轮询 → 共享快照）
// 对齐源 sensor.rs 编排语义：Dashboard 可见时驱动 1s 轮询，UI 订阅最新快照。

use crate::sensor::{
    CpuData, DiskData, GpuData, MemoryData, MotherboardData, MotherboardSensor, SensorSnapshot,
};
use parking_lot::Mutex;
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
        self.running.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// 采集一帧快照（CPU/内存/磁盘真实采集；温度/功耗/GPU 走 LHM sidecar）
fn collect_snapshot(sys: &mut sysinfo::System) -> SensorSnapshot {
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    // ---- LHM 温度/功耗（主通道；低频 ensure + 2s 缓存读取）----
    ensure_lhm_periodic();
    let (lhm_resp, lhm_diag) = crate::lhm::snapshot();

    // ---- CPU ----
    let cpus = sys.cpus();
    let core_count = cpus.len();
    let usage = sys.global_cpu_info().cpu_usage();
    let per_core: Vec<f32> = cpus.iter().map(|c| c.cpu_usage()).collect();
    let name = cpus
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_else(|| "Unknown CPU".into());
    // 频率：sysinfo frequency()（MHz）
    let clock_mhz = cpus.first().map(|c| c.frequency() as f32).unwrap_or(0.0);
    let freq_source = if clock_mhz > 0.0 { "sysinfo" } else { "none" };

    // LHM 温度/功耗注入（可用时）
    let (temperature, temp_source, power_w, power_source, power_estimated) =
        match &lhm_resp {
            Some(r) if r.available => {
                let t = r.cpu.package_temp_c.unwrap_or(0.0);
                let p = r.cpu.power_w.unwrap_or(0.0);
                if t > 0.0 {
                    (
                        t,
                        "lhm".to_string(),
                        p,
                        if p > 0.0 { "lhm" } else { "estimated" }.to_string(),
                        p <= 0.0,
                    )
                } else {
                    (0.0, "none".to_string(), 0.0, "estimated".to_string(), true)
                }
            }
            _ => (0.0, "none".to_string(), 0.0, "estimated".to_string(), true),
        };

    let cpu = CpuData {
        name,
        usage,
        per_core,
        core_count,
        clock_mhz,
        freq_source: freq_source.to_string(),
        temperature,
        power_w,
        temp_source,
        power_source,
        power_estimated,
    };

    // ---- 内存 ----
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
    // LHM 内存型号（SPD）补充
    if memory.model_name.is_empty() {
        if let Some(r) = &lhm_resp {
            if let Some(m) = &r.memory {
                if let Some(n) = &m.name {
                    if !n.is_empty() {
                        memory.model_name = n.clone();
                    }
                }
            }
        }
    }

    // ---- GPU（LHM）----
    let gpu: Vec<GpuData> = match &lhm_resp {
        Some(r) if r.available => r
            .gpu
            .iter()
            .map(|g| GpuData {
                name: g.name.clone(),
                usage: g.load_percent.unwrap_or(0.0),
                temperature: g.temperature_c.unwrap_or(0.0),
                memory_used: g.memory_used_bytes.unwrap_or(0),
                memory_total: g.memory_total_bytes.unwrap_or(0),
                clock_mhz: g.core_clock_mhz.unwrap_or(0.0),
                power_w: g.power_w.unwrap_or(0.0),
            })
            .collect(),
        _ => Vec::new(),
    };

    // ---- 主板（LHM）----
    let motherboard = match &lhm_resp {
        Some(r) => r.motherboard.clone().map(|m| MotherboardData {
            name: m.name.clone(),
            sensors: m
                .sensors
                .iter()
                .map(|s| MotherboardSensor {
                    name: s.name.clone(),
                    kind: s.kind.clone(),
                    value: s.value,
                })
                .collect(),
        }),
        None => None,
    };

    // ---- 磁盘（挂载点摘要）----
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
        disks.push(DiskData {
            name: d.name().to_string_lossy().to_string(),
            total_space: total,
            available_space: avail,
            used_space: used,
            usage_percent: pct,
            read_mbps: 0.0, // P15 PDH 后续
            write_mbps: 0.0,
        });
    }

    // ---- 诊断串 ----
    let mut diag = format!(
        "CPU={:.1}% FREQ={:.0}({}) mem={:.0}% disks={}",
        usage,
        clock_mhz,
        freq_source,
        mem_pct,
        disks.len()
    );
    if cpu.temperature <= 0.0 && !lhm_diag.is_empty() {
        diag.push_str(&format!(" TEMP=n/a({})", lhm_diag));
    }

    SensorSnapshot {
        cpu,
        gpu,
        memory,
        disks,
        motherboard,
        diag,
    }
}

/// LHM 低频 ensure（10s 节流：探测/启动开销不随 1s 轮询放大）
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
        crate::lhm::ensure_running();
    }
}
