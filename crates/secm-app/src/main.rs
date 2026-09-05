// secm-app — SECM GPUI 桌面应用入口（v2.0.0）

mod app;
mod pages;
mod theme;
mod ui;

use gpui::{
    App, Application, AppContext, Bounds, WindowOptions, WindowBounds, size, px,
};

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
        let _ = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(app::AppRoot::new),
        );
    });
}
