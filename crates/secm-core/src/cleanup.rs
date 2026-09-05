// secm-core::cleanup — 清理优化与进程管理（对齐源 v1.19.0 cleanup.rs）
// Phase 3 首批：进程列表（sysinfo Top 200）、优先级设置（FFI）、DNS 刷新。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub memory_mb: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupResult {
    pub operation: String,
    pub success: bool,
    pub bytes_freed: u64,
    pub message: String,
}

impl CleanupResult {
    fn ok(operation: &str, bytes_freed: u64, message: String) -> Self {
        Self {
            operation: operation.to_string(),
            success: true,
            bytes_freed,
            message,
        }
    }

    fn err(operation: &str, message: String) -> Self {
        Self {
            operation: operation.to_string(),
            success: false,
            bytes_freed: 0,
            message,
        }
    }
}

/// 进程列表（内存 Top 200）
pub fn list_processes() -> Vec<ProcessInfo> {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    let mut procs: Vec<ProcessInfo> = sys
        .processes()
        .iter()
        .map(|(pid, p)| ProcessInfo {
            pid: pid.as_u32(),
            name: p.name().to_string(),
            memory_mb: p.memory() as f64 / (1024.0 * 1024.0),
        })
        .collect();
    procs.sort_by(|a, b| {
        b.memory_mb
            .partial_cmp(&a.memory_mb)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    procs.truncate(200);
    procs
}

/// 设置进程优先级
/// Valid priorities: idle, below_normal, normal, above_normal, high, realtime
pub fn set_process_priority(pid: u32, priority: &str) -> CleanupResult {
    const PROCESS_SET_INFORMATION: u32 = 0x0200;
    let priority_class: u32 = match priority.to_lowercase().as_str() {
        "idle" => 0x0000_0040,
        "below_normal" => 0x0000_4000,
        "normal" => 0x0000_0020,
        "above_normal" => 0x0000_8000,
        "high" => 0x0000_0080,
        "realtime" => 0x0000_0100,
        other => {
            return CleanupResult::err(
                "设置优先级",
                format!("无效的优先级: {}（可选 idle/below_normal/normal/above_normal/high/realtime）", other),
            )
        }
    };
    #[cfg(windows)]
    {
        extern "system" {
            fn OpenProcess(dw_desired_access: u32, b_inherit_handle: i32, dw_process_id: u32) -> *mut std::ffi::c_void;
            fn SetPriorityClass(process: *mut std::ffi::c_void, priority_class: u32) -> i32;
            fn CloseHandle(h_object: *mut std::ffi::c_void) -> i32;
        }
        // SAFETY: OpenProcess/SetPriorityClass/CloseHandle 为标准 Win32 导出
        unsafe {
            let handle = OpenProcess(PROCESS_SET_INFORMATION, 0, pid);
            if handle.is_null() {
                return CleanupResult::err(
                    "设置优先级",
                    format!("无法打开进程 {}（可能不存在或权限不足）", pid),
                );
            }
            let ok = SetPriorityClass(handle, priority_class);
            CloseHandle(handle);
            if ok == 0 {
                return CleanupResult::err(
                    "设置优先级",
                    format!("设置进程 {} 优先级失败（需管理员权限）", pid),
                );
            }
        }
    }
    CleanupResult::ok(
        "设置优先级",
        0,
        format!("进程 {} 优先级已设为 {}", pid, priority),
    )
}

/// DNS 缓存刷新
pub fn flush_dns() -> CleanupResult {
    #[cfg(windows)]
    {
        extern "system" {
            fn DnsFlushResolverCache() -> i32;
        }
        // SAFETY: DnsFlushResolverCache 为 dnsapi.dll 标准导出，无参调用
        let ok = unsafe { DnsFlushResolverCache() };
        if ok == 0 {
            return CleanupResult::err("DNS 缓存刷新", "DNS 缓存刷新失败".to_string());
        }
    }
    CleanupResult::ok("DNS 缓存刷新", 0, "DNS 解析缓存已成功刷新".to_string())
}

/// 进程是否以管理员运行（复用 settings::is_admin 语义；此处为独立实现避免依赖循环）
pub fn is_admin() -> bool {
    crate::settings::is_admin()
}
