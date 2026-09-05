// secm-app::pages::logs — 调试日志页
// 订阅 secm-core 全局日志缓冲，500ms 刷新；级别筛选 + 关键词过滤。

use gpui::{div, px, rgb, Window, Context, Render, Timer, SharedString};
use gpui::prelude::*;
use secm_core::logger::{LogBuffer, LogEntry};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::theme::Theme;

pub struct LogsView {
    entries: Vec<LogEntry>,
    /// 级别筛选（空 = 全部）
    filter_level: SharedString,
    /// 关键词
    keyword: SharedString,
    /// 页面可见性门控（P1-12）：仅当前页激活时拉取日志并 notify
    active: Arc<AtomicBool>,
}

impl LogsView {
    pub fn new(active: Arc<AtomicBool>, cx: &mut Context<Self>) -> Self {
        LogBuffer::global().append("Info", "logs", "日志页已就绪");
        let mut v = Self {
            entries: Vec::new(),
            filter_level: SharedString::from(""),
            keyword: SharedString::from(""),
            active,
        };
        v.schedule_refresh(cx);
        v
    }

    fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        let active = self.active.clone();
        cx.spawn(
            async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                loop {
                    Timer::after(Duration::from_millis(500)).await;
                    // 不可见时仅睡眠轮询，不 clone 200 条不 notify（P1-12）
                    if !active.load(Ordering::Relaxed) {
                        continue;
                    }
                    let all = LogBuffer::global().get_all();
                    if let Some(view) = this.upgrade() {
                        view.update(cx, |view, cx| {
                            view.entries = all;
                            cx.notify();
                        })
                        .ok();
                    }
                }
            },
        )
        .detach();
    }

    fn level_color(level: &str, theme: &Theme) -> gpui::Rgba {
        match level {
            "Error" => theme.danger,
            "Warn" => theme.warn,
            "Info" => theme.info,
            _ => theme.text_muted,
        }
    }

    /// 过滤后的条目
    fn filtered(&self) -> Vec<&LogEntry> {
        self.entries
            .iter()
            .filter(|e| {
                (self.filter_level.is_empty() || e.level == self.filter_level.as_str())
                    && (self.keyword.is_empty()
                        || e.message.contains(self.keyword.as_str())
                        || e.module.contains(self.keyword.as_str()))
            })
            .collect()
    }
}

impl Render for LogsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::dark();
        let entries = self.filtered();

        div()
            .flex_col()
            .size_full()
            .p_6()
            .gap_3()
            // 页头 + 筛选
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(24.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("调试日志"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text_muted)
                            .child(SharedString::from(format!(
                                "{} 条 · {} 毫秒刷新",
                                self.entries.len(),
                                500
                            ))),
                    ),
            )
            // 级别筛选行（点击切筛选）
            .child(
                div()
                    .flex()
                    .gap_2()
                    .children(
                        ["全部", "Info", "Warn", "Error"].iter().map(|lvl| {
                            let active = (self.filter_level.is_empty() && *lvl == "全部")
                                || self.filter_level.as_str() == *lvl;
                            let label = (*lvl).to_string();
                            let lvl_val = (*lvl).to_string();
                            div()
                                .id(*lvl)
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .cursor_pointer()
                                .text_size(px(12.0))
                                .when(active, |s| {
                                    s.bg(theme.brand).text_color(rgb(0xffffff))
                                })
                                .when(!active, |s| s.text_color(theme.text_muted))
                                .hover(|s| s.bg(theme.panel_hover))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.filter_level = if lvl_val == "全部" {
                                        SharedString::from("")
                                    } else {
                                        SharedString::from(lvl_val.clone())
                                    };
                                    cx.notify();
                                }))
                                .child(label)
                        }),
                    ),
            )
            // 日志列表
            .child(
                div()
                    .flex_col()
                    .id("log-list")
                    .flex_1()
                    .overflow_scroll()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.panel)
                    .children(entries.iter().map(|e| {
                        let color = Self::level_color(&e.level, &theme);
                        div()
                            .flex()
                            .gap_2()
                            .px_3()
                            .py_1()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(
                                div()
                                    .w(px(60.0))
                                    .flex_none()
                                    .text_color(color)
                                    .text_size(px(11.0))
                                    .child(e.level.clone()),
                            )
                            .child(
                                div()
                                    .w(px(150.0))
                                    .flex_none()
                                    .text_color(theme.text_muted)
                                    .text_size(px(11.0))
                                    .child(e.timestamp.clone()),
                            )
                            .child(
                                div()
                                    .text_color(theme.text)
                                    .text_size(px(12.0))
                                    .child(e.message.clone()),
                            )
                    })),
            )
    }
}
