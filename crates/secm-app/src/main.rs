// secm-app — SECM GPUI 桌面应用入口
//
// 修复"启动弹命令提示符黑框"（v2.1.1）：release 构建声明为 Windows GUI 子系统，
// 启动时系统不再为其分配控制台；debug 构建（cargo run）保留控制台以便查看
// eprintln 开发日志。
// 子进程侧黑框防护已全量覆盖：全部 Command::new 均带 CREATE_NO_WINDOW
// （proc_util::run_command_with_timeout / lhm spawn+taskkill）。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod icons;
mod pages;
mod pi_clone;
mod single_instance;
mod theme;
mod tray;
mod ui;
mod win32;

use gpui::{
    App, Application, AppContext, AsyncApp, Bounds, WindowOptions, WindowBounds, size, px,
};

fn main() {
    // 单实例锁：已有实例运行时直接退出（防双开）
    if !single_instance::acquire() {
        std::process::exit(0);
    }

    Application::new()
        .with_assets(crate::pi_clone::icons::PiAssets::new())
        .run(|cx: &mut App| {
        // 日志后端：log crate → LogBuffer（调试日志页）+ 按天落盘（P1-2）
        secm_core::logger::init();

        // 文本输入控件按键绑定（TextField keymap 上下文）
        crate::ui::text_input::bind_text_field_keys(cx);

        let bounds = Bounds::centered(None, size(px(1600.0), px(900.0)), cx);
        let _ = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                // 全链路图标统一：窗口图标（标题栏/任务栏/Alt-Tab）从 exe 嵌入资源挂接
                // （GPUI 0.2 无窗口图标 API，经 raw_window_handle 桥接 Win32 WM_SETICON）
                icons::set_window_icon_from_gpui(window);
                // 移除原生标题栏：左右侧栏贯穿窗体顶部（客户区 = 全窗口）
                if let Some(hwnd) = win32::hwnd_from_window(window) {
                    win32::strip_title_bar(hwnd);
                    // 主窗体四角改圆角（DWM；Win11 生效，最大化自动方角）
                    win32::set_rounded_corners(hwnd);
                }
                cx.new(pi_clone::PiShell::new)
            },
        );

        // 系统托盘：后台线程 + 动作通道；主线程消费（显示窗口 / 退出）
        // 注意：必须用非阻塞 try_recv + 定时轮询——GPUI 前台任务跑在主线程消息循环上，
        // 若阻塞 recv() 会 park 主线程导致窗口无响应（空白窗体 BUG 根因）。
        let tray_rx = tray::spawn_tray();

        // 应用退出前清理 LHM sidecar（受控 HTTP 退出 + PID/映像名 taskkill 兜底，P1-3）。
        // on_app_quit 覆盖所有退出路径（托盘退出/系统关机）；detach 使订阅常驻不被注销。
        cx.on_app_quit(|_| async {
            secm_core::lhm::shutdown();
        })
        .detach();

        cx.spawn(async move |cx: &mut AsyncApp| {
            loop {
                // 非阻塞取出当前已排队的动作（无则立即让出主线程）
                while let Ok(action) = tray_rx.try_recv() {
                    match action {
                        tray::TrayAction::ShowWindow => {
                            let _ = cx.update(|app| {
                                app.activate(false);
                            });
                        }
                        tray::TrayAction::Quit => {
                            let _ = cx.update(|app| {
                                app.quit();
                            });
                            return;
                        }
                    }
                }
                // 让出主线程（托盘动作延迟 ≤ 200ms，可接受）
                gpui::Timer::after(std::time::Duration::from_millis(200)).await;
            }
        })
        .detach();
    });
}

