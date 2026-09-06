// secm-app::pages::about — 关于页（版本 / 技术栈 / 开源 / 许可）

use gpui::{div, px, Window, Context, Render};
use gpui::prelude::*;

use crate::theme::Theme;

pub struct AboutView;

impl AboutView {
    pub fn new() -> Self {
        log::info!("关于 · 页面已打开（v{}）", env!("CARGO_PKG_VERSION"));
        Self
    }
}

impl Render for AboutView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::dark();

        let info_rows: &[(&str, &str)] = &[
            ("版本", concat!("v", env!("CARGO_PKG_VERSION"), " (GPUI 重构版)")),
            ("UI 框架", "GPUI 0.2 (Zed, Apache-2.0)"),
            ("后端语言", "Rust (workspace: secm-app / secm-core / secm-datasource)"),
            ("温度传感", "LHM sidecar (.NET 8, MPL-2.0 进程隔离)；WinRing0/ACPI 降级链为后续版本计划"),
            ("平台", "Windows 10/11 (x64)"),
            ("许可证", "MIT"),
            ("历史版本", "Tauri + React v1.x（见原仓库）"),
        ];

        div()
            .id("about-page-root")
            .flex_col()
            .size_full()
            .p_6()
            .gap_4()
            // 内容超高时整页纵向滚动
            .overflow_y_scroll()
            .child(
                div()
                    .text_size(px(24.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.text)
                    .child("关于"),
            )
            .child(
                div()
                    .flex_col()
                    .p_5()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.panel)
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(17.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("SysEnv Console Manager"),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(theme.text_muted)
                            .child("Windows 10/11 系统环境管理工具 — 纯 Rust + GPUI"),
                    ),
            )
            .child(
                div()
                    .flex_col()
                    .p_5()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.panel)
                    .gap_2()
                    .children(info_rows.iter().map(|(k, v)| {
                        div()
                            .flex()
                            .gap_3()
                            .child(
                                div()
                                    .w(px(110.0))
                                    .flex_none()
                                    .text_color(theme.text_muted)
                                    .text_size(px(12.5))
                                    .child(k.to_string()),
                            )
                            .child(
                                div()
                                    .text_color(theme.text)
                                    .text_size(px(12.5))
                                    .child(v.to_string()),
                            )
                    })),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.text_muted)
                    .child("架构决策见 docs/adr/ · 功能基准见 docs/spec/ · MIT © 2026 SECM Team"),
            )
    }
}
