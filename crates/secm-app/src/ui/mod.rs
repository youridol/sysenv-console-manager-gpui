// secm-app::ui — 主题化基础控件（对齐源 shadcn 视觉：Card/Button/Badge/SectionTitle）
// 全部基于 GPUI 原语，颜色取自定义 Theme。

use crate::theme::Theme;
use gpui::{
    div, px, rgb, Rgba, SharedString,
};
use gpui::prelude::*;
pub use crate::theme::from_hex as color;

/// 获取当前应用主题（Phase 1 固定深色；主题切换后续实现）
pub fn current_theme() -> Theme {
    Theme::dark()
}

// ---------------------------------------------------------------------------
// Card
// ---------------------------------------------------------------------------

/// 卡片容器（面板圆角 + 边框 + 背景）
pub fn card(theme: &Theme) -> gpui::Div {
    div()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.panel)
        .p_4()
}

// ---------------------------------------------------------------------------
// Button 样式（按钮本体用 div().on_click 在页面层加交互，这里只提供视觉样式）
// ---------------------------------------------------------------------------

/// 按钮视觉（背景/文字/悬停色），返回已应用样式的 div；调用方自行 .on_click
pub fn button_primary(theme: &Theme) -> gpui::Div {
    div()
        .px_4()
        .py_1p5()
        .rounded_md()
        .bg(theme.brand)
        .text_color(rgb(0xffffff))
        .text_size(px(13.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(|s| s.bg(color(0x3d66e6)))
}

pub fn button_secondary(theme: &Theme) -> gpui::Div {
    div()
        .px_4()
        .py_1p5()
        .rounded_md()
        .bg(theme.panel_hover)
        .border_1()
        .border_color(theme.border)
        .text_color(theme.text)
        .text_size(px(13.0))
        .cursor_pointer()
        .hover(|s| s.bg(theme.panel_hover))
}

pub fn button_danger(theme: &Theme) -> gpui::Div {
    div()
        .px_4()
        .py_1p5()
        .rounded_md()
        .bg(theme.danger)
        .text_color(rgb(0xffffff))
        .text_size(px(13.0))
        .cursor_pointer()
        .hover(|s| s.bg(color(0xdc6262)))
}

// ---------------------------------------------------------------------------
// Badge
// ---------------------------------------------------------------------------

/// 小徽章（状态标签）
pub fn badge(_theme: &Theme, text: &str, border_color: Rgba) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .px_2()
        .h(px(20.0))
        .rounded_full()
        .border_1()
        .border_color(border_color)
        .text_color(border_color)
        .text_size(px(11.0))
        .child(text.to_string())
}

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

