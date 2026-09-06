// secm-app::win32 — 无边框（无标题栏）窗口支持
//
// 需求（2026-09-06）：完全移除主窗口原生标题栏，左右侧边栏贯穿窗体顶部；
// 顶部自绘区域（sidebar 品牌行 / Main TopBar）承载窗口拖动与控制按钮。
// 拖动与 min/max/close 点击均交由 GPUI 的 window_control_area 平台处理，
// 本模块仅负责创建期去掉原生标题栏。
//
// 复用 icons.rs 的 raw_window_handle 桥接模式（gpui Window → HWND）。

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SendMessageW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_STYLE,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_CLOSE,
    WS_CAPTION, WS_SYSMENU, SW_MINIMIZE,
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

/// 移除原生标题栏但保留可拉伸边框：
///   去 WS_CAPTION | WS_SYSMENU → 客户区无标题栏（左右侧栏贯穿窗体顶部），
///   保留 WS_THICKFRAME → 系统提供四边/四角边缘拖拽拉伸 + 最大化双线光标。
///   （v2.4.1：此前一并去除 THICKFRAME 导致窗口不可拉伸；GPUI 的 resize
///   hit-test 依赖 DefWindowProc 对 THICKFRAME 样式返回 HTLEFT/HTRIGHT 等区域。）
/// 保留 WS_MAXIMIZEBOX / WS_MINIMIZEBOX（任务栏与最大化/最小化能力）。
pub fn strip_title_bar(hwnd: HWND) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let new_style = style & !(WS_CAPTION | WS_SYSMENU);
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

/// 主窗体四角改圆角（DWM 窗口圆角偏好）。
///
/// - 仅 Windows 11（22000+）支持 `DWMWA_WINDOW_CORNER_PREFERENCE`；Win10/旧版
///   调用返回错误，静默忽略（保持直角，不影响功能）。
/// - DWM 在窗口最大化时自动切换为方角、还原时恢复圆角，无需额外处理。
/// - 圆角半径由系统主题决定（默认约 8px，跟随系统设置），无法自定义精确像素。
/// 需在 `strip_title_bar`（去除系统边框）之后调用，否则边框样式冲突。
pub fn set_rounded_corners(hwnd: HWND) {
    let preference: i32 = DWMWCP_ROUND;
    // SAFETY: hwnd 为有效窗口句柄；pvattribute 指向栈上 i32，cbattribute 为长度
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &preference as *const i32 as *const core::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );
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




