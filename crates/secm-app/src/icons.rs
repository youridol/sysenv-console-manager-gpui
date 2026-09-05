// secm-app::icons — 全链路统一图标接入（资源源：crates/icons/）
//
// 三个图标面全部出自 crates/icons 资源（v2.1.0 统一）：
// 1. exe 文件图标：build.rs 经 winresource 嵌入 icon.ico（资源 ID 1）
// 2. 窗口图标（标题栏/任务栏/Alt-Tab）：运行时 LoadImageW 从同一资源加载 + WM_SETICON
// 3. 托盘图标：解码 32x32.png → RGBA → tray_icon::Icon
//
// 历史实现：托盘为程序化生成的 16x16 蓝色圆点，窗口/exe 无图标 —— 本模块统一收口。

use windows_sys::Win32::UI::WindowsAndMessaging::{
    LoadImageW, SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTSIZE, WM_SETICON,
};

/// 32x32 托盘图标源（编译期嵌入，无运行时文件依赖）
pub const ICON_PNG_32: &[u8] = include_bytes!("../../icons/32x32.png");

/// 托盘图标 RGBA 数据（解码 32x32.png；失败返回 None，调用方降级）
pub fn tray_rgba() -> Option<(Vec<u8>, i32, i32)> {
    let img = image::load_from_memory(ICON_PNG_32).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as i32, rgba.height() as i32);
    Some((rgba.into_raw(), w, h))
}

/// 把嵌入资源（ID 1，build.rs 注入的 icon.ico）加载为窗口图标并挂到指定 HWND。
///
/// 大图标（任务栏/Alt-Tab，系统默认尺寸）+ 小图标（标题栏，SM_CXSMICON 尺寸）。
/// 资源加载失败静默降级（图标缺失不影响功能）。
pub fn apply_window_icon(hwnd: windows_sys::Win32::Foundation::HWND) {
    // SAFETY: GetModuleHandleW(NULL) 取本进程模块句柄；
    // MAKEINTRESOURCE 语义 = 整数 ID 转 PCWSTR（资源 ID 1）；
    // SendMessageW 目标为自身窗口句柄。
    unsafe {
        let hinstance = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(
            std::ptr::null(),
        );
        if hinstance.is_null() {
            return;
        }
        // MAKEINTRESOURCEW(1)
        let name: windows_sys::core::PCWSTR = 1u16 as *const u16;
        let big = LoadImageW(hinstance, name, IMAGE_ICON, 0, 0, LR_DEFAULTSIZE);
        if !big.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as isize);
        }
        let small = LoadImageW(hinstance, name, IMAGE_ICON, 0, 0, LR_DEFAULTSIZE);
        if !small.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as isize);
        }
    }
}

/// gpui Window → 原生 HWND 桥接并挂接窗口图标
///
/// GPUI 0.2 无窗口图标 API；经 raw_window_handle 取 Win32 句柄后手动 WM_SETICON。
pub fn set_window_icon_from_gpui(window: &mut gpui::Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = window.window_handle() else {
        return;
    };
    if let RawWindowHandle::Win32(win) = handle.as_raw() {
        // windows-sys 0.61 的 HWND 与 raw_window_handle 的 hwnd 同为 *mut c_void
        let hwnd = win.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        apply_window_icon(hwnd);
    }
}
