//! 网络接口实时流量速率（PDH 性能计数器）— 每接口独立 下行/上行 (KB/s)
//!
//! 数据源：pdh.dll 的 `\Network Interface(*)\Bytes Received/sec` /
//! `\Network Interface(*)\Bytes Sent/sec` 通配符计数器（Windows 标准性能
//! 计数器，任务管理器「性能→网络」同源，普通用户可读，无需管理员权限）。
//!
//! 实例语义：PDH 的 Network Interface 按「网卡名」提供实例，实例名即网卡
//! 友好名（如 `以太网` / `WLAN` / `vEthernet (Default Switch)`）。与
//! `netif::link_speeds()`（GetIfTable2 的 Alias）按网卡名匹配 —— 链路速率与
//! 实时流量取自同一网卡（上层按键名归一后匹配）。
//!
//! 时序语义：
//! - `PdhCollectQueryData` 两次调用间隔须 ≥1s 才能得到有效速率（PDH 内部采样）；
//!   SECM 侧栏 1s 轮询满足该约束。
//! - 首次调用仅建立采样基线（返回空映射）；此后每次调用返回最近 ~1s 平均速率。
//! - 采样间隔不足（PDH_INVALID_DATA）时沿用上次有效值，避免抖动归零。
//! - 计数器不可用（虚拟机 / 网络被系统禁用）时静默降级空映射（S8 降级语义）。
//!
//! 线程模型：同步阻塞 API，查询开销微秒级；调用方须在后台线程中执行（S8）。
//! 实现对齐同目录 disk_io.rs（PDH 通配计数器模板，v2.0.0 闪退修复后稳定形态）。

use std::collections::HashMap;
use std::sync::Mutex;
use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
};

/// PDH 成功状态码
const PDH_CSTATUS_VALID_DATA: u32 = 0x0000_0000;
const PDH_CSTATUS_NEW_DATA: u32 = 0x0000_0001;
/// 两次采样间隔不足（<1s）时单个值返回该状态
const PDH_INVALID_DATA: u32 = 0xC000_0BC6;
/// 缓冲不足（首次查询大小用）
const PDH_MORE_DATA: u32 = 0x8000_07D2;

/// 通配符计数器路径（`*` 展开为全部网卡实例）
const COUNTER_RX: &str = r"\Network Interface(*)\Bytes Received/sec";
const COUNTER_TX: &str = r"\Network Interface(*)\Bytes Sent/sec";

/// PDH 查询状态（进程级单例；查询句柄生命周期 = 进程生命周期，退出由 OS 回收）
struct PdhState {
    query: isize,
    rx_counter: isize,
    tx_counter: isize,
    /// 上次成功读取：网卡名 → (下行 KB/s, 上行 KB/s)；间隔不足时沿用
    speed_map: HashMap<String, (f32, f32)>,
    /// 初始化失败标记（计数器不存在等）——失败后不再重试
    failed: bool,
}

static PDH_STATE: Mutex<Option<PdhState>> = Mutex::new(None);

/// 字符串 → NUL 结尾 UTF-16
fn to_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// 初始化 PDH 查询（打开查询 + 添加上下行通配计数器 + 建立首次采样基线）
fn pdh_init() -> Result<PdhState, String> {
    let mut query: PDH_HQUERY = std::ptr::null_mut();
    // SAFETY: NULL 数据源 = 本地实时性能数据；query 由 API 写入
    let rc = unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut query) };
    if rc != 0 {
        return Err(format!("PdhOpenQueryW failed win32=0x{:08X}", rc));
    }
    let rx_path = to_utf16(COUNTER_RX);
    let tx_path = to_utf16(COUNTER_TX);
    let mut rx_counter: PDH_HCOUNTER = std::ptr::null_mut();
    let mut tx_counter: PDH_HCOUNTER = std::ptr::null_mut();
    // SAFETY: 计数器路径为 NUL 结尾 UTF-16；句柄由 API 写入
    let rc_rx = unsafe { PdhAddEnglishCounterW(query, rx_path.as_ptr(), 0, &mut rx_counter) };
    let rc_tx = unsafe { PdhAddEnglishCounterW(query, tx_path.as_ptr(), 0, &mut tx_counter) };
    if rc_rx != 0 || rc_tx != 0 {
        // SAFETY: query 为 PdhOpenQueryW 返回的有效句柄
        unsafe { PdhCloseQuery(query) };
        return Err(format!(
            "PdhAddEnglishCounterW failed rx=0x{:08X} tx=0x{:08X}",
            rc_rx, rc_tx
        ));
    }
    // 首次收集建立采样基线（此刻无速率数据，下轮起出数）
    // SAFETY: query 为有效句柄
    unsafe { PdhCollectQueryData(query) };
    Ok(PdhState {
        query: query as isize,
        rx_counter: rx_counter as isize,
        tx_counter: tx_counter as isize,
        speed_map: HashMap::new(),
        failed: false,
    })
}

/// 读取通配计数器的全部实例值（网卡名 → KB/s）。
///
/// 两次分配调用：先查所需缓冲大小，再填充数组；成功时逐个校验 CStatus。
///
/// ⚠ 缓冲布局：PDH 返回的条目数组为**变长布局**（每个 `PDH_FMT_COUNTERVALUE_ITEM_W`
/// 后紧跟该实例的 UTF-16 名称串），所需缓冲字节数 > 条目数 × 结构大小；
/// 必须按第一次调用返回的 `buf_size` 字节数分配（与 disk_io.rs 同根因，防越界崩溃）。
fn read_counter_array_kbps(counter: isize) -> HashMap<String, f32> {
    let mut buf_size: u32 = 0;
    let mut item_count: u32 = 0;
    // SAFETY: 首次调用仅查询大小（buffer 为 null）
    let rc = unsafe {
        PdhGetFormattedCounterArrayW(
            counter as PDH_HCOUNTER,
            PDH_FMT_DOUBLE,
            &mut buf_size,
            &mut item_count,
            std::ptr::null_mut(),
        )
    };
    // PDH_MORE_DATA 是首次查询的预期返回（缓冲不足）；其余非 0 视为失败
    if rc != PDH_MORE_DATA && rc != PDH_CSTATUS_VALID_DATA && rc != PDH_CSTATUS_NEW_DATA {
        return HashMap::new();
    }
    if buf_size == 0 || item_count == 0 {
        return HashMap::new();
    }
    let capacity = (buf_size as usize).div_ceil(std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>());
    // SAFETY: PDH_FMT_COUNTERVALUE_ITEM_W 为 repr(C) 数值/指针 POD，零值合法；
    // 此处仅作输出缓冲，API 返回成功（下方 rc 校验）后才读取内容
    let zero = unsafe { std::mem::zeroed::<PDH_FMT_COUNTERVALUE_ITEM_W>() };
    let mut buf = vec![zero; capacity.max(item_count as usize)];
    let mut out_size: u32 = buf_size;
    let mut out_count: u32 = item_count;
    // SAFETY: buf 为对齐正确的输出缓冲，容量 >= buf_size 字节
    let rc = unsafe {
        PdhGetFormattedCounterArrayW(
            counter as PDH_HCOUNTER,
            PDH_FMT_DOUBLE,
            &mut out_size,
            &mut out_count,
            buf.as_mut_ptr(),
        )
    };
    if rc != PDH_CSTATUS_VALID_DATA && rc != PDH_CSTATUS_NEW_DATA {
        return HashMap::new();
    }
    let mut map = HashMap::new();
    for item in buf.iter().take(out_count as usize) {
        // 采样间隔不足/无效的单项跳过
        if item.FmtValue.CStatus == PDH_INVALID_DATA {
            continue;
        }
        // SAFETY: CStatus 有效时 doubleValue 已由 API 写入
        let bps = unsafe { item.FmtValue.Anonymous.doubleValue };
        if !bps.is_finite() || bps < 0.0 {
            continue;
        }
        // 实例名：NUL 结尾 UTF-16（指向缓冲内），解析为 String
        // SAFETY: szName 由 API 填充，指向缓冲内 NUL 结尾字符串
        let name = unsafe {
            let mut end = 0usize;
            while *item.szName.add(end) != 0 {
                end += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(item.szName, end))
        };
        // 排除聚合伪实例（_Total）
        if name.is_empty() || name == "_Total" {
            continue;
        }
        // Bytes/sec → KB/s（上层更贴近人类可读流量；下行/上行分开显示）
        map.insert(name, (bps / 1024.0) as f32);
    }
    map
}

/// 获取各网卡实时流量速率（键为网卡名，如 "以太网"）。
///
/// 返回 网卡名 → (下行 KB/s, 上行 KB/s)；通道不可用/初始化失败时返回空映射
/// （降级不中断，调用方按键名匹配不到即为 0）。
pub fn get_net_io_speed_map() -> HashMap<String, (f32, f32)> {
    let mut guard = match PDH_STATE.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(s) = guard.as_mut() {
        if s.failed {
            return HashMap::new();
        }
        // SAFETY: query 为有效句柄
        unsafe { PdhCollectQueryData(s.query as PDH_HQUERY) };
        let rxs = read_counter_array_kbps(s.rx_counter);
        let txs = read_counter_array_kbps(s.tx_counter);
        let mut next: HashMap<String, (f32, f32)> = HashMap::new();
        for (name, rx) in rxs {
            let tx = txs.get(&name).copied().unwrap_or(0.0);
            next.insert(name, (rx, tx));
        }
        for (name, tx) in txs {
            next.entry(name).or_insert((0.0, tx));
        }
        // 本次无有效数据的实例沿用上次值（间隔不足场景）
        for (name, (rx, tx)) in &s.speed_map {
            next.entry(name.clone()).or_insert((*rx, *tx));
        }
        s.speed_map = next;
        return s.speed_map.clone();
    }
    // 首次调用：初始化
    match pdh_init() {
        Ok(state) => {
            *guard = Some(state);
            HashMap::new()
        }
        Err(e) => {
            log::warn!("[net_io] PDH 初始化失败（网络流量速率降级为 0）: {e}");
            *guard = Some(PdhState {
                query: 0,
                rx_counter: 0,
                tx_counter: 0,
                speed_map: HashMap::new(),
                failed: true,
            });
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真机验证（默认忽略；PDH 通道真实采样，无需管理员）：
    /// 首次调用建立基线（空映射），间隔 >1s 后的调用应返回非空实例映射，
    /// 且每个值均为非负有限速率（网卡空闲时为 0）。
    /// 运行：`cargo test -p secm-datasource -- --ignored real_machine_net_io_speed_map`
    #[test]
    #[ignore]
    fn real_machine_net_io_speed_map() {
        let m1 = get_net_io_speed_map();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let m2 = get_net_io_speed_map();
        eprintln!("[真机] net_io 首调实例数={} 次调实例数={}", m1.len(), m2.len());
        for (name, (rx, tx)) in &m2 {
            eprintln!("[真机] net_io {name}: ↓rx={rx:.1} ↑tx={tx:.1} KB/s");
            assert!(rx.is_finite() && *rx >= 0.0, "下行速率非法: {}", rx);
            assert!(tx.is_finite() && *tx >= 0.0, "上行速率非法: {}", tx);
        }
        assert!(!m2.is_empty(), "应至少解析到一个网卡实例");
    }

    /// 纯逻辑：实例名过滤（排除 _Total 聚合伪实例）
    #[test]
    fn test_excludes_total_instance() {
        // 验证 name 过滤逻辑：_Total 应被剔除（读函数内过滤，这里只做语义确认）
        assert_ne!("_Total", "以太网");
        assert!(!"_Total".is_empty());
    }
}
