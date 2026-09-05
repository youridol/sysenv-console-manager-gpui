// secm-app::win32 — 无边框（无标题栏）窗口支持
//
// 需求（2026-09-06）：完全移除主窗口原生标题栏，左右侧边栏贯穿窗体顶部；
// 顶部自绘区域（sidebar 品牌行 / Main TopBar）承载窗口拖动与控制按钮。
// 拖动与 min/max/close 点击均交由 GPUI 的 window_control_area 平台处理，
// 本模块仅负责创建期去掉原生标题栏。
//
// 复用 icons.rs 的 raw_window_handle 桥接模式（gpui Window → HWND）。

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SendMessageW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_STYLE,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_CLOSE,
    WS_CAPTION, WS_SYSMENU, WS_THICKFRAME, SW_MINIMIZE,
};

/// 从 gpui Window 取 HWND（与 icons.rs 相同桥接）
pub fn hwnd_from_window(window: &mut gpui::Window) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle().ok()?;
    if let RawWindowHandle::Win32(win) = handle.as_raw() {
        return Some(win.hwnd.get() as HWND);
    }
    None
}

/// 完全移除原生窗口边框与标题栏：
///   去 WS_CAPTION | WS_SYSMENU | WS_THICKFRAME → 客户区 = 窗口全尺寸，
///   内容直达窗口最顶（无顶部刘海/边框留白）。
/// 保留 WS_MAXIMIZEBOX / WS_MINIMIZEBOX（任务栏与最大化/最小化能力）。
/// 可调大小改为自绘边缘（见 ui 层 resize 热区），此处不再依赖系统边框。
pub fn strip_title_bar(hwnd: HWND) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let new_style = style & !(WS_CAPTION | WS_SYSMENU | WS_THICKFRAME);
        if new_style != style {
            SetWindowLongPtrW(hwnd, GWL_STYLE, new_style as isize);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}




/// 最小化窗口
pub fn minimize_window(hwnd: HWND) {
    unsafe {
        ShowWindow(hwnd, SW_MINIMIZE);
    }
}

/// 关闭窗口（WM_CLOSE → GPUI should_close 默认 true → 应用退出/托盘清理）
pub fn close_window(hwnd: HWND) {
    unsafe {
        SendMessageW(hwnd, WM_CLOSE, 0, 0);
    }
}




