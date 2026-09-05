// secm-app — SECM GPUI 桌面应用入口（v2.0.0 骨架，Phase 1 起实现页面）
use gpui::{
    App, Application, Bounds, Context, SharedString, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
};

struct AppRoot {
    title: SharedString,
}

impl Render for AppRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1e2e))
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                div()
                    .text_color(rgb(0xffffff))
                    .text_size(px(26.0))
                    .child("SysEnv Console Manager v2.0.0"),
            )
            .child(
                div()
                    .text_color(rgb(0x89b4fa))
                    .text_size(px(16.0))
                    .child("纯 Rust + GPUI — 骨架运行中"),
            )
            .child(
                div()
                    .text_color(rgb(0x585b70))
                    .text_size(px(13.0))
                    .child(format!("{}", &self.title)),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
        let _ = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| {
                cx.new(|_| AppRoot {
                    title: "骨架：等待 Phase 1 界面实现".into(),
                })
            },
        );
    });
}
