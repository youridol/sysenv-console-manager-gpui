// secm-app::tray — 系统托盘（tray-icon 后台线程 + win32 消息泵；事件回主线程）
// 模式已验证（tray-spike）：托盘线程独立跑消息循环，GPUI 主循环不受影响。

use std::sync::mpsc;

/// 托盘用户动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    ShowWindow,
    Quit,
}

/// 托盘线程内错误上报（P1-13：历史实现静默 return，托盘失败用户零感知；
/// 经 LogBuffer 直写 + stderr 双通道，Logs 页可见）
fn tray_fail(msg: &str) {
    eprintln!("[tray] {}", msg);
    secm_core::logger::LogBuffer::global().append("Error", "tray", msg);
}

/// 在后台线程创建托盘并返回动作通道。
/// 调用方负责在主线程消费 rx（例如 cx.spawn 循环），并执行对应 UI 动作。
pub fn spawn_tray() -> mpsc::Receiver<TrayAction> {
    let (tx, rx) = mpsc::channel::<TrayAction>();
    let spawned = std::thread::Builder::new()
        .name("tray-thread".into())
        .spawn(move || {
            use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
            // 全链路图标统一：托盘图标解码 crates/icons/32x32.png（编译期嵌入）；
            // 解码失败降级为程序化占位圆点（v2.1.0 前的历史实现），保证托盘可用性
            let (rgba, w, h) = match crate::icons::tray_rgba() {
                Some((rgba, w, h)) => (rgba, w as u32, h as u32),
                None => {
                    tray_fail("托盘图标 PNG 解码失败，降级为占位图标");
                    let (w, h) = (16u32, 16u32);
                    let mut rgba: Vec<u8> = Vec::with_capacity((w * h * 4) as usize);
                    for y in 0..h {
                        for x in 0..w {
                            let in_circle = (x as i32 - 7).pow(2) + (y as i32 - 7).pow(2) <= 36;
                            if in_circle {
                                rgba.extend_from_slice(&[0x4f, 0x7c, 0xff, 0xff]);
                            } else {
                                rgba.extend_from_slice(&[0, 0, 0, 0]);
                            }
                        }
                    }
                    (rgba, w, h)
                }
            };
            let Ok(icon) = tray_icon::Icon::from_rgba(rgba, w, h) else {
                tray_fail("托盘图标构建失败（RGBA 数据异常），托盘不可用；请从主窗口操作");
                return;
            };
            let menu = Menu::new();
            let show_item = MenuItem::new("显示主窗口", true, None);
            let quit_item = MenuItem::new("退出", true, None);
            if menu
                .append_items(&[&show_item, &PredefinedMenuItem::separator(), &quit_item])
                .is_err()
            {
                tray_fail("托盘菜单构建失败，托盘不可用；请从主窗口操作");
                return;
            }
            let show_tx = tx.clone();
            let quit_tx = tx.clone();
            let show_id = show_item.id().0.clone();
            let quit_id = quit_item.id().0.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if event.id.0 == show_id {
                    let _ = show_tx.send(TrayAction::ShowWindow);
                } else if event.id.0 == quit_id {
                    let _ = quit_tx.send(TrayAction::Quit);
                }
            }));
            if tray_icon::TrayIconBuilder::new()
                .with_icon(icon)
                .with_tooltip("SysEnv Console Manager")
                .with_menu(Box::new(menu))
                .build()
                .is_err()
            {
                tray_fail("托盘创建失败（系统托盘不可用？），托盘不可用；请从主窗口操作");
                return;
            }
            unsafe { run_win32_message_loop() };
        });
    if let Err(e) = spawned {
        // 线程启动失败不再 panic（P1-13）：托盘缺失但应用可用
        tray_fail(&format!("托盘线程启动失败: {}（应用以无托盘模式运行）", e));
    }
    rx
}

/// win32 消息泵：GetMessageW + Translate + Dispatch 直到 WM_QUIT。
unsafe fn run_win32_message_loop() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG,
    };
    loop {
        let mut msg: MSG = std::mem::zeroed();
        let ret = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
        if ret <= 0 {
            break;
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}
