// secm-core::lhm — LHM sidecar（LibreHardwareMonitor .NET 进程）客户端
// 契约对齐源 v1.19.0 lhm.rs：HTTP 45980 JSON；主程序仅做进程探测/启动 + 轮询。
// 许可：LibreHardwareMonitorLib MPL-2.0（隔离于 sidecar 进程内，随包分发源码与许可）。

use serde::Deserialize;
use std::path::PathBuf;
use std::process::{Child, Command};
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

/// 读取快照（2s 缓存内直接复用；未命中/超时则 HTTP 拉取）
pub fn snapshot() -> (Option<LhmSensorResponse>, String) {
    // 缓存命中
    if let Some(c) = SNAP_CACHE.get() {
        if let Some((t, resp, diag)) = c.lock().as_ref() {
            if t.elapsed() < SNAP_TTL {
                return (Some(resp.clone()), diag.clone());
            }
        }
    }
    // 拉取
    let (resp, diag) = fetch_sensors();
    if let Some(r) = &resp {
        let cache = SNAP_CACHE.get_or_init(|| Mutex::new(None));
        *cache.lock() = Some((Instant::now(), r.clone(), diag.clone()));
    }
    (resp, diag)
}

pub struct LhmSidecar {
    /// sidecar 启动子进程句柄（Drop 时释放；sidecar 提权后可能已脱离）
    #[allow(dead_code)]
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
/// 1. 环境变量 SECM_LHM_SIDECAR 显式指定
/// 2. 当前 exe 同目录 lhm/publish（发布布局）
/// 3. 开发期：原项目 src-tauri/resources/lhm/publish（由 stage-lhm 生成）
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
    if let Ok(cwd) = std::env::current_dir() {
        v.push(cwd.join("resources").join("lhm").join("publish").join("LhmSidecar.exe"));
    }
    // 开发期指向原 Tauri 仓库 stage 产物（本机开发环境；发布时无需此项）
    v.push(PathBuf::from(r"Y:\SysEnv-Console-Manager\src-tauri\resources\lhm\publish\LhmSidecar.exe"));
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

/// 停止 sidecar（应用退出时清理）
pub fn shutdown() {
    let mut st = state().lock();
    if let Some(s) = st.sidecar.take() {
        drop(s); // Child Drop 不杀进程；用 taskkill 兜底（对齐源）
    }
    let _ = Command::new("taskkill")
        .args(["/IM", "LhmSidecar.exe", "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}
