// pi_clone::right_panel — 右侧日志流输出面板（替代原文件工作台）
//
// 产品调整（用户指令 2026-09-06）：调试日志从左栏页面迁出，右栏整改为
// SECM 全链路日志流式输出面板 —— 记录本应用（sys env console manager）
// 所有行为与返回信息（前后端操作日志/行为日志）流式展示。
//
// 数据源：secm_core::logger::LogBuffer（log crate 全局桥接 + 环形 200 条），
// 与按天落盘共用同一入口 —— 全库 log::info!/warn!/debug! 均实时流式呈现。
//
// 结构（对齐参考壳右面板 tab-strip 48px 布局语义，改造为日志面板头）：
//   - 头部 48px：标题「日志流」+ 级别筛选（全部/Info/Warn/Error）+ 清空按钮
//   - 主体：滚动日志行（时间戳 [级别] 模块 消息），新日志到达自动滚到底
//   - 桌面 ≥960 并排（可拖）；641-959 覆盖层抽屉；≤640 全屏
//
// GPUI 0.2 轮询模式（与旧 LogsView 一致）：500ms 从 LogBuffer 拉全量比对追加，
// 简单可靠、无跨线程投递复杂度；容量 200 使全量克隆开销可忽略。

use gpui::{div, px, FontWeight, InteractiveElement, ParentElement, Styled, Window};
use gpui::prelude::*;
use gpui::{Context, SharedString};

use super::icons::{self, Icon};
use super::layout;
use super::theme::Palette;
use super::PiShell;

/// 自上次推送后保留的行数（超出丢弃最旧，防内存/渲染膨胀）
pub const KEEP_LINES: usize = 500;

impl PiShell {
    /// Right panel（桌面/覆盖/移动统一进入；宽度与模式已由 AppShell 决策）
    pub fn render_right_panel(
        &mut self,
        _fullscreen: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pal = self.palette();
        let vw = self.viewport_w(window);
        let mobile = layout::is_mobile(vw);
        let compact = layout::is_compact_overlay(vw);

        // 有效宽度
        let w = if mobile {
            vw
        } else if compact {
            // 覆盖层固定 min(560, vw-48)
            vw.min(560.0).max(layout::RIGHT_PANEL_MIN_WIDTH)
        } else {
            self.right_display_w
        };

        // 面板内容（共享，避免重复）
        let content = div()
            .flex_col()
            .size_full()
            .bg(pal.bg)
            .border_l_1()
            .border_color(pal.separator)
            .child(self.log_header(&pal, cx))
            .child(self.log_stream(&pal, window, cx));
        if compact {
            // 覆盖层抽屉（absolute 顶层，右缘）
            let open = self.right_open;
            let inner = if open { w } else { 0.0 };
            return div()
                .id("pi-right-compact")
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .w(px(inner))
                .flex_shrink_0()
                .overflow_hidden()
                .child(content)
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .w(px(1.0))
                        .bg(pal.separator),
                )
                .when(!open, |s| s.hidden())
                .into_any_element();
        }

        // 桌面并排：flex 列直接参与主 row
        div()
            .id("pi-right-panel")
            .flex_shrink_0()
            .h_full()
            .overflow_hidden()
            .w(px(if self.right_open { w } else { 0.0 }))
            .child(content)
            .into_any_element()
    }

    /// 面板头：标题 + 级别筛选 + 清空 + 开关
    fn log_header(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        div()
            .flex()
            .items_center()
            .h(px(48.0))
            .flex_shrink_0()
            .px(px(10.0))
            .gap(px(6.0))
            .bg(pal.surface_muted)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .flex_1()
                    .min_w(px(0.0))
                    .child(icons::icon(Icon::Terminal, 14.0).text_color(pal.text_muted))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(pal.text)
                            .child(SharedString::from("日志流")),
                    ),
            )
            // 级别筛选：全部 / Info / Warn / Error
            .child(self.log_filter_pill(pal, cx, "全部", ""))
            .child(self.log_filter_pill(pal, cx, "Info", "Info"))
            .child(self.log_filter_pill(pal, cx, "Warn", "Warn"))
            .child(self.log_filter_pill(pal, cx, "Error", "Error"))
            // 清空按钮
            .child(
                div()
                    .id("pi-log-clear")
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(28.0))
                    .h(px(28.0))
                    .rounded(px(7.0))
                    .cursor_pointer()
                    .text_color(pal.text_muted)
                    .hover(|s| s.bg(pal.bg_hover).text_color(pal.text))
                    .on_click(move |_ev, _w, cx| {
                        let _ = this.update(cx, |t, cx| t.clear_log_panel(cx));
                    })
                    .child(icons::icon(Icon::Close, 13.0).text_color(pal.text_muted)),
            )
            // 面板开关
            .child(self.right_toggle_in_strip(pal, cx))
    }

    /// 级别筛选 pill（点击切换）
    fn log_filter_pill(&self, pal: &Palette, cx: &mut Context<Self>, label: &str, lvl: &str) -> impl IntoElement {
        let this = cx.entity();
        // 当前筛选（存于 log_filter_level 字段；空 = 全部）
        let active = (self.log_filter_level.is_empty() && lvl.is_empty())
            || (!self.log_filter_level.is_empty() && self.log_filter_level == lvl);
        let owned_label = label.to_string();
        let owned_lvl = lvl.to_string();
        div()
            .id(SharedString::from(format!("pi-log-filter-{label}")))
            .px(px(7.0))
            .h(px(22.0))
            .flex()
            .items_center()
            .rounded(px(6.0))
            .cursor_pointer()
            .text_size(px(10.5))
            .text_color(if active { pal.text } else { pal.text_muted })
            .bg(if active { pal.bg_selected } else { super::theme::TRANSPARENT })
            .hover(|s| s.bg(pal.bg_hover))
            .on_click(move |_ev, _w, cx| {
                let _ = this.update(cx, |t, cx| {
                    t.log_filter_level = owned_lvl.clone();
                    log::info!("日志流筛选 → {}", if owned_lvl.is_empty() { "全部" } else { &owned_lvl });
                    cx.notify();
                });
            })
            .child(SharedString::from(owned_label))
    }

    fn right_toggle_in_strip(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        div()
            .id("pi-right-toggle-strip")
            .flex()
            .items_center()
            .justify_center()
            .w(px(34.0))
            .h(px(28.0))
            .rounded(px(7.0))
            .cursor_pointer()
            .text_color(if self.right_open { pal.text } else { pal.text_muted })
            .bg(if self.right_open {
                pal.bg_hover
            } else {
                super::theme::TRANSPARENT
            })
            .hover(|s| s.bg(pal.bg_hover).text_color(pal.text))
            .on_click(move |_ev, _w, cx| {
                let _ = this.update(cx, |t, cx| t.toggle_right(cx));
            })
            .child(icons::icon(Icon::Panel, 16.0).text_color(if self.right_open { pal.text } else { pal.text_muted }))
    }

    /// 日志流主体（滚动行列表；自动跟随最新；点击行复制该条日志）
    fn log_stream(&mut self, pal: &Palette, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let level = self.log_filter_level.clone();
        let rows: Vec<gpui::AnyElement> = self
            .log_rows
            .iter()
            .filter(|e| level.is_empty() || e.level == level)
            .enumerate()
            .map(|(ix, e)| {
                let color = match e.level.as_str() {
                    "Error" => pal.danger,
                    "Warn" => pal.warning,
                    "Info" => pal.success,
                    _ => pal.text_muted,
                };
                // 复制内容：整行日志文本
                let copy_text = format!(
                    "[{}] {} {}",
                    e.level,
                    e.timestamp,
                    e.message
                );
                div()
                    .id(SharedString::from(format!("pi-log-row-{ix}-{}", e.level)))
                    .flex()
                    .items_start()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .py(px(2.0))
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .border_b_1()
                    .border_color(pal.separator)
                    .hover(|s| s.bg(pal.bg_hover))
                    .on_click(move |_ev, _w, cx| {
                        // 点击日志行 → 复制该条到系统剪贴板
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_text.clone()));
                        log::debug!("日志流 · 已复制一行日志到剪贴板");
                    })
                    .child(
                        div()
                            .w(px(52.0))
                            .flex_none()
                            .pt(px(2.0))
                            .text_size(px(10.0))
                            .text_color(color)
                            .child(SharedString::from(e.level.clone())),
                    )
                    .child(
                        div()
                            .w(px(100.0))
                            .flex_none()
                            .pt(px(2.0))
                            .text_size(px(9.5))
                            .text_color(pal.text_dim)
                            .child(SharedString::from(e.timestamp.clone())),
                    )
                    // 消息占满剩余宽度并自动折行（min_w 0 允许收缩；不再被右侧裁断）
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(px(11.0))
                            .text_color(pal.text)
                            .child(SharedString::from(e.message.clone())),
                    )
                    .into_any_element()
            })
            .collect();

        // 滚动句柄（懒创建并持有；track_scroll 要求 stateful div）
        if self.log_scroll.is_none() {
            self.log_scroll = Some(gpui::ScrollHandle::new());
        }
        let scroll = self.log_scroll.clone().expect("已建");

        div()
            .id("pi-log-stream")
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .bg(pal.bg)
            .when(rows.is_empty(), |s| {
                s.child(
                    div()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .h_full()
                        .gap(px(6.0))
                        .text_size(px(11.5))
                        .text_color(pal.text_dim)
                        .child(SharedString::from("暂无日志")),
                )
            })
            .children(rows)
    }
}
