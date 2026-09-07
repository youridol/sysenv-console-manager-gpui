//! CPU 总负载采集（PDH 性能计数器）— ADR-0004 Canonical 主路径
//!
//! 数据源（对齐 LiteMonitor PerformanceCounterManager.GetCpuLoad 链）：
//! 1. `\Processor Information(_Total)\% Processor Utility` —— Win8+ 任务管理器
//!    同源（考虑睿频），LiteMonitor 主路径；
//! 2. `\Processor Information(_Total)\% Processor Time` —— LiteMonitor 回退层；
//!    （注意：`\Processor(_Total)\...` 实测不可用，必须用 Processor Information 版本）
//! 3. 全部不可用 → 返回 None，上层回退 sysinfo 差分（= % Processor Time 语义）。
//!
//! 时序语义：PDH 两次采样间隔须 ≥1s；首调 warmup 补齐最小采样间隔（与
//! cpu_freq.rs 同模式），此后每次调用返回最近采样值，间隔不足时沿用上次值。
//! % Processor Utility 可 >100%（睿频聚合），沿用 LiteMonitor 语义截断到 100。
//!
//! 线程模型：同步阻塞 API，调用方须在后台线程执行（S8）。

use std::mem::MaybeUninit;
use std::sync::Mutex;
use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE,
    PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_INVALID_DATA,
};

/// PDH 计数器路径（Processor Information 类别，_Total 实例）
const COUNTER_UTILITY: &str = r"\Processor Information(_Total)\% Processor Utility";
const COUNTER_TIME: &str = r"\Processor Information(_Total)\% Processor Time";

/// PDH 两次采样最小间隔（与 cpu_freq.rs 一致：略大于 1s，防时钟抖动）
const PDH_MIN_SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1050);

/// PDH 查询状态（进程级单例）
struct PdhLoadState {
    query: isize,
    utility_counter: isize,
    time_counter: isize,
    /// 上次有效负载（%）；采样间隔不足/无效时沿用
    last_load: Option<f32>,
    /// 初始化失败标记——失败后不再重试（上层走 sysinfo 回退）
    failed: bool,
    baseline_at: Option<std::time::Instant>,
}

static PDH_LOAD_STATE: Mutex<Option<PdhLoadState>> = Mutex::new(None);

/// 字符串 → NUL 结尾 UTF-16。
fn to_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// 初始化 PDH 查询：主计数器 % Processor Utility + 回退计数器 % Processor Time
/// （两者可同时存在；读取时 Utility 失败自动落 Time，与 LiteMonitor 链一致）。
fn pdh_load_init() -> Result<PdhLoadState, String> {
    let mut query: PDH_HQUERY = std::ptr::null_mut();
    // SAFETY: NULL 数据源 = 本地实时性能数据；query 由 API 写入
    let rc = unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut query) };
    if rc != 0 {
        return Err(format!("PdhOpenQueryW failed win32=0x{:08X}", rc));
    }
    let add = |path: &str| -> Result<isize, String> {
        let wide = to_utf16(path);
        let mut counter: PDH_HCOUNTER = std::ptr::null_mut();
        // SAFETY: 计数器路径为 NUL 结尾 UTF-16；句柄由 API 写入
        let rc = unsafe { PdhAddEnglishCounterW(query, wide.as_ptr(), 0, &mut counter) };
        if rc != 0 {
            return Err(format!(
                "PdhAddEnglishCounterW({path}) failed win32=0x{rc:08X}"
            ));
        }
        Ok(counter as isize)
    };
    // Utility 主路径缺失（精简版 Windows）不视为整体失败——Time 回退层可能可用
    let utility = add(COUNTER_UTILITY).ok();
    let time = match add(COUNTER_TIME) {
        Ok(c) => Some(c),
        Err(e) => {
            if utility.is_none() {
                // SAFETY: query 为 PdhOpenQueryW 返回的有效句柄
                unsafe { PdhCloseQuery(query) };
                return Err(e);
            }
            None
        }
    };
    if utility.is_none() && time.is_none() {
        // SAFETY: query 为有效句柄（add 内部失败时已由上方分支关闭）
        unsafe { PdhCloseQuery(query) };
        return Err("Processor Information counters unavailable".to_string());
    }
    // 首次收集建立采样基线
    // SAFETY: query 为有效句柄
    let rc = unsafe { PdhCollectQueryData(query) };
    if rc != 0 && rc != PDH_CSTATUS_NEW_DATA {
        // SAFETY: query 为有效句柄
        unsafe { PdhCloseQuery(query) };
        return Err(format!("PdhCollectQueryData failed win32=0x{rc:08X}"));
    }
    Ok(PdhLoadState {
        query: query as isize,
        utility_counter: utility.unwrap_or(0),
        time_counter: time.unwrap_or(0),
        last_load: None,
        failed: false,
        baseline_at: Some(std::time::Instant::now()),
    })
}

/// 读取单个负载计数器当前值（%）。None = 采样间隔不足 / 无效 / 非有限。
fn read_load_counter(counter: isize) -> Option<f32> {
    let mut ty: u32 = 0;
    let mut value = MaybeUninit::<PDH_FMT_COUNTERVALUE>::zeroed();
    // SAFETY: value 为对齐正确的输出缓冲；ty 由 API 写入
    let rc = unsafe {
        PdhGetFormattedCounterValue(
            counter as PDH_HCOUNTER,
            PDH_FMT_DOUBLE,
            &mut ty,
            value.as_mut_ptr(),
        )
    };
    if rc != PDH_CSTATUS_VALID_DATA && rc != PDH_CSTATUS_NEW_DATA {
        return None;
    }
    // SAFETY: rc 校验通过后 CStatus 与值已由 API 填充
    let v = unsafe { value.assume_init() };
    if v.CStatus == PDH_INVALID_DATA {
        return None;
    }
    // SAFETY: CStatus 有效时 doubleValue 已由 API 写入
    let pct = unsafe { v.Anonymous.doubleValue };
    if !pct.is_finite() {
        return None;
    }
    Some(pct as f32)
}

/// 采样 CPU 总负载（%），返回 `Some(负载%)`；PDH 通道不可用时返回 `None`
/// （上层回退 sysinfo % Processor Time 差分，全部失败透出 unavailable）。
///
/// LiteMonitor 等价链：`% Processor Utility` → `% Processor Time` → 上层回退。
/// Utility >100% 截断到 100（任务管理器行为）。
pub fn get_cpu_load() -> Option<f32> {
    let mut guard = PDH_LOAD_STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = guard.as_mut() {
        if s.failed {
            return None;
        }
        // SAFETY: query 为有效句柄
        let rc = unsafe { PdhCollectQueryData(s.query as PDH_HQUERY) };
        if rc != 0 && rc != PDH_CSTATUS_NEW_DATA {
            log::warn!("[cpu_load] PdhCollectQueryData failed: win32=0x{rc:08X}, reuse last value");
            return s.last_load;
        }
        s.baseline_at = Some(std::time::Instant::now());
        // LiteMonitor 链：Utility 主 → Time 回退
        let val = if s.utility_counter != 0 {
            read_load_counter(s.utility_counter)
        } else {
            None
        }
        .or_else(|| {
            if s.time_counter != 0 {
                read_load_counter(s.time_counter)
            } else {
                None
            }
        });
        match val {
            // Utility 可 >100%（睿频聚合），截断到 100
            Some(pct) => {
                let clamped = pct.clamp(0.0, 100.0);
                s.last_load = Some(clamped);
                Some(clamped)
            }
            None => s.last_load,
        }
    } else {
        // 首次调用：初始化 + warmup 补齐采样间隔（仅进程内第一次阻塞）
        match pdh_load_init() {
            Ok(mut state) => {
                if let Some(b) = state.baseline_at {
                    let elapsed = b.elapsed();
                    if elapsed < PDH_MIN_SAMPLE_INTERVAL {
                        std::thread::sleep(PDH_MIN_SAMPLE_INTERVAL - elapsed);
                    }
                }
                // 基线后立即采样一次，保证首调返回有效值（PDH 首采样为 0 的问题
                // 由 warmup 间隔解决；本次读数可能仍为 0，属真实空载）
                // SAFETY: query 为有效句柄
                unsafe { PdhCollectQueryData(state.query as PDH_HQUERY) };
                let val = if state.utility_counter != 0 {
                    read_load_counter(state.utility_counter)
                } else {
                    None
                }
                .or_else(|| {
                    if state.time_counter != 0 {
                        read_load_counter(state.time_counter)
                    } else {
                        None
                    }
                })
                .map(|pct| pct.clamp(0.0, 100.0));
                state.last_load = val;
                *guard = Some(state);
                val
            }
            Err(e) => {
                log::warn!("[cpu_load] PDH init failed, falls back to sysinfo: {e}");
                *guard = Some(PdhLoadState {
                    query: 0,
                    utility_counter: 0,
                    time_counter: 0,
                    last_load: None,
                    failed: true,
                    baseline_at: None,
                });
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真机验证（默认忽略）：PDH Utility 主路径采样，空闲机负载 <50%。
    /// 运行：`cargo test -p secm-datasource -- --ignored real_machine_cpu_load`
    #[test]
    #[ignore]
    fn real_machine_cpu_load() {
        let first = get_cpu_load();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let second = get_cpu_load();
        eprintln!("[真机] cpu_load 首调={first:?} 次调={second:?}");
        if let Some(v) = second {
            assert!((0.0..=100.0).contains(&v), "负载越界: {v}");
        } else {
            eprintln!("[真机] PDH 通道不可用（预期回退 sysinfo）");
        }
    }
}
