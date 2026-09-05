// secm-app — SECM GPUI 桌面应用入口（v2.0.0）

mod app;
mod pages;
mod single_instance;
mod theme;
mod tray;
mod ui;

use gpui::{
    App, Application, AppContext, AsyncApp, Bounds, WindowOptions, WindowBounds, size, px,
};

fn main() {
    // 单实例锁：已有实例运行时直接退出（防双开）
    if !single_instance::acquire() {
        std::process::exit(0);
    }

    Application::new().run(|cx: &mut App| {
        // 文本输入控件按键绑定（TextField keymap 上下文）
        crate::ui::text_input::bind_text_field_keys(cx);

        let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
        let _ = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(app::AppRoot::new),
        );

        // 系统托盘：后台线程 + 动作通道；主线程消费（显示窗口 / 退出）
        // 注意：必须用非阻塞 try_recv + 定时轮询——GPUI 前台任务跑在主线程消息循环上，
        // 若阻塞 recv() 会 park 主线程导致窗口无响应（空白窗体 BUG 根因）。
        let tray_rx = tray::spawn_tray();
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
