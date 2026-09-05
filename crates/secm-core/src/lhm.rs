// secm-core::lhm — LHM sidecar（LibreHardwareMonitor .NET 进程）客户端
// 契约对齐源 v1.19.0 lhm.rs：HTTP 45980 JSON；主程序仅做进程探测/启动 + 轮询。
// 许可：LibreHardwareMonitorLib MPL-2.0（隔离于 sidecar 进程内，随包分发源码与许可）。

use serde::Deserialize;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// sidecar 监听端口（与 sidecar 默认一致）
pub const LHM_PORT: u16 = 45980;
/// HTTP 读取超时
const HTTP_TIMEOUT: Duration = Duration::from_secs(2);
/// 启动后就绪探测：重试次数与间隔（覆盖 .NET 冷启动 + UAC 提权移交）
const START_RETRIES: u32 = 10;
const START_RETRY_DELAY: Duration = Duration::from_millis(500);
/// CREATE_NO_WINDOW（sidecar 不弹控制台）
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ============================================================================
// sidecar JSON 契约（对齐源；字段与 serde 结构逐一对应）
// ============================================================================

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct LhmCpuData {
    pub package_temp_c: Option<f32>,
    pub core_temps_c: Vec<f32>,
    pub power_w: Option<f32>,
    pub fan_rpm: Option<f32>,
    pub voltage_v: Option<f32>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct LhmGpuData {
    pub name: String,
    pub temperature_c: Option<f32>,
    pub core_clock_mhz: Option<f32>,
    pub power_w: Option<f32>,
    pub load_percent: Option<f32>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub fan_rpm: Option<f32>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct LhmMbSensor {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub value: f32,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct LhmMotherboardData {
    pub name: Option<String>,
    pub sensors: Vec<LhmMbSensor>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct LhmMemoryData {
    pub name: Option<String>,
    pub total_bytes: Option<u64>,
    pub used_bytes: Option<u64>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct LhmSensorResponse {
    pub available: bool,
    pub error: Option<String>,
    pub contract_version: i64,
    pub cpu: LhmCpuData,
    pub gpu: Vec<LhmGpuData>,
    pub motherboard: Option<LhmMotherboardData>,
    pub memory: Option<LhmMemoryData>,
}

// ============================================================================
// sidecar 进程句柄（Drop 时清理）
// ============================================================================

/// 带 TTL 的传感器快照缓存（避免每帧 HTTP；对齐源 2s 窗口）
static SNAP_CACHE: OnceLock<Mutex<Option<(Instant, LhmSensorResponse, String)>>> =
    OnceLock::new();
const SNAP_TTL: Duration = Duration::from_secs(2);
/// 拉取失败退避（P1-4）：失败后该窗口内不再发起 HTTP，
/// 防 sidecar 掉线时采集线程反复阻塞在 2s 超时上
static SNAP_FAIL_UNTIL: AtomicU64 = AtomicU64::new(0);
const SNAP_FAIL_BACKOFF_SECS: u64 = 5;

/// 当前 Unix 秒（退避时间戳用；时钟异常时返回 0，退避立即过期，仅降级）
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 读取快照（2s 缓存内直接复用；未命中/超时则 HTTP 拉取；失败后 5s 退避）
pub fn snapshot() -> (Option<LhmSensorResponse>, String) {
    // 缓存命中
    if let Some(c) = SNAP_CACHE.get() {
        if let Some((t, resp, diag)) = c.lock().as_ref() {
            if t.elapsed() < SNAP_TTL {
                return (Some(resp.clone()), diag.clone());
            }
        }
    }
    // 失败退避窗口内：不发起 HTTP，复用旧快照（如有）避免采集线程反复阻塞
    if unix_now() < SNAP_FAIL_UNTIL.load(Ordering::Relaxed) {
        if let Some(c) = SNAP_CACHE.get() {
            if let Some((_, resp, diag)) = c.lock().as_ref() {
                return (Some(resp.clone()), diag.clone());
            }
        }
        return (None, "lhm sidecar 不可用（失败退避中）".to_string());
    }
    // 拉取
    let (resp, diag) = fetch_sensors();
    match &resp {
        Some(r) => {
            SNAP_FAIL_UNTIL.store(0, Ordering::Relaxed);
            let cache = SNAP_CACHE.get_or_init(|| Mutex::new(None));
            *cache.lock() = Some((Instant::now(), r.clone(), diag.clone()));
        }
        None => {
            SNAP_FAIL_UNTIL.store(
                unix_now().saturating_add(SNAP_FAIL_BACKOFF_SECS),
                Ordering::Relaxed,
            );
        }
    }
    (resp, diag)
}

pub struct LhmSidecar {
    /// sidecar 启动子进程句柄（shutdown 按 PID 精确兜底清理）
    child: Option<Child>,
}

/// 全局 sidecar 单例状态
use parking_lot::Mutex;
use std::sync::OnceLock;

struct State {
    sidecar: Option<LhmSidecar>,
    /// 最近一次失败诊断（降级提示用）
    last_error: String,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| {
        Mutex::new(State {
            sidecar: None,
            last_error: String::new(),
        })
    })
}

/// 候选 sidecar 路径（依序探测）：
/// 1. 环境变量 SECM_LHM_SIDECAR 显式指定（部署/调试覆盖）
/// 2. 当前 exe 同目录 lhm/publish（发布布局）
///
/// 安全约束（审计 P1-18）：不再探测 cwd 候选与任何硬编码绝对路径——
/// 从可写目录启动时 cwd 候选可被二进制植入；开发机路径字面量不得编入发布二进制。
fn candidate_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var("SECM_LHM_SIDECAR") {
        if !p.is_empty() {
            v.push(PathBuf::from(p).join("LhmSidecar.exe"));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            v.push(dir.join("lhm").join("publish").join("LhmSidecar.exe"));
        }
    }
    v
}

/// 定位存在的 sidecar（无则 None）
fn locate_sidecar() -> Option<PathBuf> {
    candidate_paths().into_iter().find(|p| p.is_file())
}

/// 健康检查
pub fn health_ok() -> bool {
    match ureq::get(&format!("http://127.0.0.1:{}/health", LHM_PORT))
        .timeout(HTTP_TIMEOUT)
        .call()
    {
        Ok(r) => r.status() == 200,
        Err(_) => false,
    }
}

/// 读取传感器快照（sidecar 不可用返回默认 + 诊断）
pub fn fetch_sensors() -> (Option<LhmSensorResponse>, String) {
    let url = format!("http://127.0.0.1:{}/api/lhm/sensors", LHM_PORT);
    match ureq::get(&url).timeout(HTTP_TIMEOUT).call() {
        Ok(r) => match r.into_string() {
            Ok(text) => match serde_json::from_str::<LhmSensorResponse>(&text) {
                Ok(resp) => (Some(resp), String::new()),
                Err(e) => (None, format!("lhm JSON 解析失败: {}", e)),
            },
            Err(e) => (None, format!("lhm 响应读取失败: {}", e)),
        },
        Err(e) => (None, format!("lhm HTTP 请求失败: {}", e)),
    }
}

/// 确保 sidecar 可用：未就绪则探测启动并等待 health（幂等）
pub fn ensure_running() -> String {
    if health_ok() {
        return String::new();
    }
    // 已有 spawn 记录（可能 UAC 等待中）→ 等待 health 一次；不行则重置
    let already = state().lock().sidecar.is_some();
    if already {
        if wait_health() {
            return String::new();
        }
        state().lock().sidecar = None;
    }
    // 定位并启动
    let Some(exe) = locate_sidecar() else {
        let msg = "LHM sidecar 未找到（resources/lhm/publish/LhmSidecar.exe）".to_string();
        state().lock().last_error = msg.clone();
        return msg;
    };
    let spawn_result = Command::new(&exe)
        .args(["--port", &LHM_PORT.to_string()])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    match spawn_result {
        Ok(child) => {
            state().lock().sidecar = Some(LhmSidecar { child: Some(child) });
        }
        Err(e) => {
            let msg = format!("LHM sidecar 启动失败: {}", e);
            state().lock().last_error = msg.clone();
            return msg;
        }
    }
    if wait_health() {
        String::new()
    } else {
        let msg = "LHM sidecar 启动但 health 超时（可能等待 UAC 提权确认）".to_string();
        state().lock().last_error = msg.clone();
        msg
    }
}

/// 等待 health 就绪（限时）
fn wait_health() -> bool {
    let deadline = Instant::now() + Duration::from_secs(START_RETRIES as u64 * 1);
    while Instant::now() < deadline {
        if health_ok() {
            return true;
        }
        std::thread::sleep(START_RETRY_DELAY);
    }
    false
}

/// 最近失败诊断
pub fn last_error() -> String {
    state().lock().last_error.clone()
}

/// 停止 sidecar（应用退出时清理；main 的 on_app_quit 钩子必须调用，P1-3）
///
/// 顺序：① 受控退出——HTTP /api/shutdown 让 sidecar 自行释放驱动句柄后退出
/// （对 UAC 提权的 sidecar 也有效：localhost HTTP 无需特权，而非提权
/// taskkill 杀不动提权进程）；② 按本应用 spawn 的 PID 精确 taskkill（含子树）；
/// ③ 按映像名清理历史孤儿（普通权限杀不动其他用户的进程，无越权副作用）。
pub fn shutdown() {
    // ① 受控退出
    if health_ok() {
        let url = format!("http://127.0.0.1:{}/api/shutdown", LHM_PORT);
        let _ = ureq::get(&url).timeout(Duration::from_secs(2)).call();
        // 等待端口释放（sidecar 退出），上限 ~3s
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if !health_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    // ② 按 PID 精确兜底（UAC 场景 child 可能已退出：PID 失效时 taskkill 自然失败，跳过）
    let pid = state()
        .lock()
        .sidecar
        .as_ref()
        .and_then(|s| s.child.as_ref().map(|c| c.id()));
    if let Some(pid) = pid {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    // ③ 清理历史孤儿（本应用此前版本/异常退出遗留的同名进程）
    let _ = Command::new("taskkill")
        .args(["/IM", "LhmSidecar.exe", "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    state().lock().sidecar = None;
}
