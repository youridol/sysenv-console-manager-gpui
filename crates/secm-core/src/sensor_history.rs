// secm-core::sensor_history — 硬件趋势历史（60s 趋势图数据源 + 跨重启持久化）
//
// 职责（硬件信息页趋势图 / 网络速率趋势图）：
// - 1s 固定节拍采样：CPU 占用、GPU 占用（首个 GPU）、内存占用、网络总量下行/上行
//   速率（KB/s），写入有界序列；
// - 序列持久化到 %LOCALAPPDATA%\SECM\cache\sensor_history.json（脏后 10s 落盘 +
//   flush() 退出兜底），应用重启时恢复 —— 趋势图不因重启清零；
// - 网络流量卡的可调间隔（0.5s–5s）采样由 UI 侧驱动（DashboardView 网络采样任务），
//   经 record_adapter_rates 写入单网卡序列（仅内存态，不持久化）；总量序列以本模块
//   1s 节拍为准（避免双写）。
//
// 速率语义：网络速率来自 GetIfTable2 累计字节差分（if_bytes_map，两次快照
// Δbytes/Δt），无 PDH ≥1s 间隔限制；首次采样仅建立基线（记 0）。
//
// 线程模型：专职 std::thread 采样（与 SensorService 同款模式）；
// parking_lot::Mutex 保护状态；GetIfTable2 为微秒级同步调用，不阻塞 UI（S8）。

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 主序列最大点数（1800 点 ≈ 30 分钟 @1s，控制持久化体积）
const MAX_POINTS: usize = 1800;
/// 单网卡序列最大点数（240 点 @0.5s ≈ 2 分钟窗口）
const ADAPTER_MAX_POINTS: usize = 240;
/// 落盘脏检查周期（毫秒）
const SAVE_INTERVAL_MS: u64 = 10_000;
/// 趋势图窗口（毫秒；UI 取"最近 60s"）
pub const CHART_WINDOW_MS: u64 = 60_000;
/// 历史文件路径（相对 %LOCALAPPDATA%）
const CACHE_DIR_SUFFIX: &str = "SECM\\cache";
const CACHE_FILE: &str = "sensor_history.json";

/// 单个趋势点（t = Unix 毫秒；跨重启恢复按墙上时钟对齐）
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HistoryPoint {
    pub t: u64,
    pub v: f32,
}

/// 主序列快照（UI 每秒拉取渲染）
#[derive(Debug, Clone, Default)]
pub struct HistorySnapshot {
    pub cpu: Vec<HistoryPoint>,
    pub gpu: Vec<HistoryPoint>,
    pub mem: Vec<HistoryPoint>,
    pub rx: Vec<HistoryPoint>,
    pub tx: Vec<HistoryPoint>,
}

#[derive(Default)]
struct HistoryState {
    cpu: Vec<HistoryPoint>,
    gpu: Vec<HistoryPoint>,
    mem: Vec<HistoryPoint>,
    rx: Vec<HistoryPoint>,
    tx: Vec<HistoryPoint>,
    /// 单网卡序列（仅内存态；UI 可调间隔采样写入）
    adapter_rx: HashMap<String, Vec<HistoryPoint>>,
    adapter_tx: HashMap<String, Vec<HistoryPoint>>,
    /// 上次累计字节快照：(unix_ms, 别名 → (InOctets, OutOctets))
    prev_bytes: Option<(u64, HashMap<String, (u64, u64)>)>,
    dirty: bool,
}

/// 持久化格式（v1：[t,v] 元组对；f32 以 JSON number 存取）
#[derive(Serialize, Deserialize, Default)]
struct Persisted {
    v: u32,
    cpu: Vec<(u64, f32)>,
    gpu: Vec<(u64, f32)>,
    mem: Vec<(u64, f32)>,
    rx: Vec<(u64, f32)>,
    tx: Vec<(u64, f32)>,
}

static STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

/// 进程级状态（OnceLock 惰性初始化，规避 static 构造的非 const 路径）
fn state() -> &'static Mutex<HistoryState> {
    static S: std::sync::OnceLock<Mutex<HistoryState>> = std::sync::OnceLock::new();
    S.get_or_init(|| Mutex::new(HistoryState::default()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 推入点并裁剪到容量上限
fn push(series: &mut Vec<HistoryPoint>, pt: HistoryPoint, cap: usize) {
    series.push(pt);
    if series.len() > cap {
        let drop = series.len() - cap;
        series.drain(..drop);
    }
}

/// 启动历史采样（幂等；加载持久化 + 拉起专职采样线程）。
/// 同时确保 SensorService 已启动（幂等）——历史采样以其快照为数据源。
pub fn start_once() {
    if STARTED.set(()).is_err() {
        return;
    }
    crate::sensor_service::SensorService::start_once();

    // 加载持久化历史（跨重启恢复；裁掉 6h 外旧点防陈旧膨胀）
    let restored = load_from_disk();
    {
        let mut g = state().lock();
        g.cpu = restored.cpu;
        g.gpu = restored.gpu;
        g.mem = restored.mem;
        g.rx = restored.rx;
        g.tx = restored.tx;
    }
    log::info!("传感器历史 · 已加载持久化趋势（趋势图跨重启恢复）");

    std::thread::Builder::new()
        .name("secm-sensor-history".into())
        .spawn(|| {
            let mut last_save = now_ms();
            loop {
                std::thread::sleep(Duration::from_millis(1000));
                sample_once();
                // 脏检查落盘（10s 周期；退出另有 flush 兜底）
                let now = now_ms();
                let should_save = state().lock().dirty && now - last_save >= SAVE_INTERVAL_MS;
                if should_save {
                    save_to_disk();
                    last_save = now;
                }
            }
        })
        .expect("spawn secm-sensor-history");
}

/// 单拍采样：传感器快照 → CPU/GPU/内存序列；累计字节差分 → 总量速率序列
fn sample_once() {
    let t = now_ms();
    let snap = crate::sensor_service::SensorService::snapshot();

    // 网络总量速率（差分基线在状态内维护）
    let map = secm_datasource::netif::if_bytes_map();
    let (rx_kbps, tx_kbps) = {
        let mut g = state().lock();
        let (drx, dtx) = match g.prev_bytes {
            Some((pt, ref prev)) if t > pt => {
                let dt = (t - pt) as f32 / 1000.0;
                let mut acc = (0.0f32, 0.0f32);
                for (name, (rx, tx)) in &map {
                    if let Some((prx, ptx)) = prev.get(name) {
                        if rx >= prx {
                            acc.0 += (rx - prx) as f32 / dt / 1024.0;
                        }
                        if tx >= ptx {
                            acc.1 += (tx - ptx) as f32 / dt / 1024.0;
                        }
                    }
                }
                acc
            }
            _ => (0.0, 0.0), // 首拍仅建立基线
        };
        g.prev_bytes = Some((t, map));
        (drx, dtx)
    };

    let mut g = state().lock();
    // 传感器未就绪（服务未起/首拍全零）时不记点，避免污染历史
    if snap.cpu.core_count > 0 {
        push(&mut g.cpu, HistoryPoint { t, v: snap.cpu.usage }, MAX_POINTS);
        if !snap.gpu.is_empty() {
            let gpu_v = snap.gpu.first().map(|gp| gp.usage).unwrap_or(0.0);
            push(&mut g.gpu, HistoryPoint { t, v: gpu_v }, MAX_POINTS);
        }
        push(
            &mut g.mem,
            HistoryPoint { t, v: snap.memory.usage_percent },
            MAX_POINTS,
        );
    }
    push(&mut g.rx, HistoryPoint { t, v: rx_kbps }, MAX_POINTS);
    push(&mut g.tx, HistoryPoint { t, v: tx_kbps }, MAX_POINTS);
    g.dirty = true;
}

/// 拉取主序列（仅最近 window_ms 窗口；UI 60s 趋势图用，避免整序列克隆）
pub fn snapshot_series_window(window_ms: u64) -> HistorySnapshot {
    let g = state().lock();
    let cutoff = now_ms().saturating_sub(window_ms);
    let take = |s: &Vec<HistoryPoint>| -> Vec<HistoryPoint> {
        let start = s.partition_point(|p| p.t < cutoff);
        s[start..].to_vec()
    };
    HistorySnapshot {
        cpu: take(&g.cpu),
        gpu: take(&g.gpu),
        mem: take(&g.mem),
        rx: take(&g.rx),
        tx: take(&g.tx),
    }
}

/// 记录单网卡实时速率（UI 网络采样任务回填；仅内存态，不持久化）
pub fn record_adapter_rates(samples: &[(String, f32, f32)]) {
    if samples.is_empty() {
        return;
    }
    let t = now_ms();
    let mut g = state().lock();
    for (name, rx, tx) in samples {
        push(
            g.adapter_rx.entry(name.clone()).or_default(),
            HistoryPoint { t, v: *rx },
            ADAPTER_MAX_POINTS,
        );
        push(
            g.adapter_tx.entry(name.clone()).or_default(),
            HistoryPoint { t, v: *tx },
            ADAPTER_MAX_POINTS,
        );
    }
}

/// 读取单网卡序列 (下行, 上行)（最近 window_ms 窗口；无该网卡历史返回空）
pub fn adapter_series_window(name: &str, window_ms: u64) -> (Vec<HistoryPoint>, Vec<HistoryPoint>) {
    let g = state().lock();
    let cutoff = now_ms().saturating_sub(window_ms);
    let take = |s: &Vec<HistoryPoint>| -> Vec<HistoryPoint> {
        let start = s.partition_point(|p| p.t < cutoff);
        s[start..].to_vec()
    };
    (
        g.adapter_rx.get(name).map(take).unwrap_or_default(),
        g.adapter_tx.get(name).map(take).unwrap_or_default(),
    )
}

/// 退出兜底：脏时立即落盘（on_app_quit 调用）
pub fn flush() {
    if state().lock().dirty {
        save_to_disk();
    }
}

/// 持久化文件路径（%LOCALAPPDATA%\SECM\cache\sensor_history.json；环境缺失返回 None）
fn cache_path() -> Option<std::path::PathBuf> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    let dir = std::path::Path::new(&base).join(CACHE_DIR_SUFFIX);
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    Some(dir.join(CACHE_FILE))
}

fn save_to_disk() {
    let payload = {
        let g = state().lock();
        let conv = |s: &Vec<HistoryPoint>| -> Vec<(u64, f32)> {
            s.iter().map(|p| (p.t, p.v)).collect()
        };
        Persisted {
            v: 1,
            cpu: conv(&g.cpu),
            gpu: conv(&g.gpu),
            mem: conv(&g.mem),
            rx: conv(&g.rx),
            tx: conv(&g.tx),
        }
    };
    let Some(path) = cache_path() else {
        return;
    };
    match serde_json::to_string(&payload) {
        Ok(json) => {
            // 临时文件 + 原子替换，避免写一半被结束进程截断
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path)).is_ok() {
                state().lock().dirty = false;
            } else {
                log::warn!("传感器历史 · 持久化写入失败（趋势图跨重启恢复降级）");
            }
        }
        Err(e) => log::warn!("传感器历史 · 序列化失败: {}", e),
    }
}

fn load_from_disk() -> HistorySnapshot {
    let Some(path) = cache_path() else {
        return HistorySnapshot::default();
    };
    let Ok(json) = std::fs::read_to_string(&path) else {
        return HistorySnapshot::default();
    };
    let parsed: Result<Persisted, _> = serde_json::from_str(&json);
    match parsed {
        Ok(p) if p.v == 1 => {
            let cutoff = now_ms().saturating_sub(6 * 3600 * 1000);
            let conv = |s: Vec<(u64, f32)>| -> Vec<HistoryPoint> {
                s.into_iter()
                    .filter(|(t, _)| *t >= cutoff)
                    .map(|(t, v)| HistoryPoint { t, v })
                    .collect()
            };
            HistorySnapshot {
                cpu: conv(p.cpu),
                gpu: conv(p.gpu),
                mem: conv(p.mem),
                rx: conv(p.rx),
                tx: conv(p.tx),
            }
        }
        Ok(_) => {
            log::warn!("传感器历史 · 缓存版本不识别，忽略（将重新积累）");
            HistorySnapshot::default()
        }
        Err(e) => {
            log::warn!("传感器历史 · 缓存解析失败（{}），将重新积累", e);
            HistorySnapshot::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 纯逻辑：窗口过滤语义（partition_point 按时间升序裁剪）
    #[test]
    fn test_window_filter_keeps_recent_points() {
        let now = 10_000u64;
        let series: Vec<HistoryPoint> = (0..10)
            .map(|i| HistoryPoint { t: i * 1000, v: i as f32 })
            .collect();
        // 60s 窗口应包含全部 10 点（跨度 9s；cutoff 饱和不回绕）
        let start = series.partition_point(|p| p.t < now.saturating_sub(60_000));
        assert_eq!(start, 0);
        // 5s 窗口应只保留 t >= 5000 的 5 点
        let start5 = series.partition_point(|p| p.t < now.saturating_sub(5_000));
        assert_eq!(start5, 5);
        assert_eq!(series[start5..].len(), 5);
    }

    /// 纯逻辑：容量裁剪（push 超上限后丢弃最旧）
    #[test]
    fn test_push_caps_series() {
        let mut s = Vec::new();
        for i in 0..12 {
            push(&mut s, HistoryPoint { t: i, v: i as f32 }, 10);
        }
        assert_eq!(s.len(), 10);
        assert_eq!(s[0].t, 2, "最旧 2 点应被裁剪");
    }
}
