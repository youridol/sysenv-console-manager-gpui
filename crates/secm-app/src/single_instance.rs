// secm-app::single_instance — 单实例锁（Windows 命名 Mutex）
// 原理：进程启动时创建命名 Mutex；若 ERROR_ALREADY_EXISTS → 已有实例运行，
// 直接退出（对齐源 tauri-plugin-single-instance 语义，防止双开冲突）。
// Mutex 句柄保存在静态变量中，进程存活期间不释放（Drop 即释放锁）。

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Threading::CreateMutexW;

/// 单实例锁句柄（RAII 语义的简化版：进程结束由 OS 回收）
static mut LOCK_HANDLE: HANDLE = INVALID_HANDLE_VALUE;

/// 应用唯一标识（防与其他同名 Mutex 冲突；全局命名空间需 SeCreateGlobalPrivilege，
/// 普通权限用 Local\ 前缀会话内唯一即可——桌面应用单会话足够）
const MUTEX_NAME: &str = "Local\\SysEnvConsoleManager-GPUI-SingleInstance";

/// 尝试获取单实例锁。返回 false 表示已有实例在运行，调用方应立即退出。
pub fn acquire() -> bool {
    // SAFETY: CreateMutexW 传入 NUL 结尾宽字符串；句柄存静态由进程生命周期管理
    let wide: Vec<u16> = MUTEX_NAME.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
    if handle.is_null() {
        // 创建失败（极罕见）：放行（不阻塞启动）
        eprintln!("[single-instance] Mutex 创建失败，放行启动");
        return true;
    }
    // SAFETY: GetLastError 无参线程局部
    let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    if err == ERROR_ALREADY_EXISTS {
        // 已有实例：释放本句柄并返回 false
        // SAFETY: 有效 Mutex 句柄
        unsafe { CloseHandle(handle) };
        false
    } else {
        // 首个实例：持有句柄直到进程退出
        // SAFETY: 静态写入仅在此函数一次调用中发生（main 启动期单线程）
        unsafe {
            LOCK_HANDLE = handle;
        }
        true
    }
}
