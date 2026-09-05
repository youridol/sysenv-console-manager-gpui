// secm-app::ui — 主题化基础控件（页头/区块标题/数据表辅助）
// 全部基于 GPUI 原语，颜色取自定义 Theme。

pub mod text_input;

use crate::theme::Theme;
use gpui::{div, px, SharedString};
use gpui::prelude::*;

// ---------------------------------------------------------------------------
// PageHeader / SectionTitle
// ---------------------------------------------------------------------------

/// 区块标题
pub fn section_title(theme: &Theme, text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().size(px(4.0)).rounded_full().bg(theme.brand))
        .child(
            div()
                .text_color(theme.text)
                .text_size(px(15.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(text.into()),
        )
}

/// 页头（标题 + 副标题）
pub fn page_header(
    theme: &Theme,
    title: impl Into<SharedString>,
    subtitle: impl Into<SharedString>,
) -> impl IntoElement {
    div()
        .flex_col()
        .child(
            div()
                .text_size(px(22.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(theme.text)
                .child(title.into()),
        )
        .child(
            div()
                .mt_0p5()
                .text_size(px(13.0))
                .text_color(theme.text_muted)
                .child(subtitle.into()),
        )
}

// ---------------------------------------------------------------------------
// 简易数据表辅助（服务/进程等列表通用：表头 + 行，列宽按比例 flex）
// ---------------------------------------------------------------------------

/// 表容器（圆角卡片内嵌表）
pub fn table_container(theme: &Theme) -> gpui::Div {
    div()
        .flex_col()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.panel)
}

/// 表头行
pub fn table_head(theme: &Theme, cols: &[&str]) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_4()
        .py_2()
        .bg(theme.panel_hover)
        .children(cols.iter().map(|c| {
            div()
                .flex_1()
                .text_size(px(11.5))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.text_muted)
                .child(c.to_string())
        }))
}

/// 空表提示
pub fn table_empty(theme: &Theme, msg: &str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .px_4()
        .py_8()
        .child(
            div()
                .text_size(px(12.5))
                .text_color(theme.text_muted)
                .child(msg.to_string()),
        )
}
