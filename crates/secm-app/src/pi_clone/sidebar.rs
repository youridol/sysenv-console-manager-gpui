// pi_clone::sidebar — 左侧边栏：SECM 工具页导航列表
//
// 产品调整（用户指令）：已移除全部会话功能（PROJECTS/会话树/会话行/NewChat/
// 搜索会话）。侧栏承载 11 个工具页的分组导航，点击切换 Main 区页面。
//
// 布局复刻沿用参考外壳数值：chrome controls 行、分组标题（10.5px uppercase dim）、
// 页面行 30px/28px、hover bg-hover、选中 bg-selected + text。
//
// 导航分组标题下方不再渲染重复的快捷图标条（与底部 footer 重复，已按用户指令移除），
// 底部工具条由外层壳统一置底渲染一份。

use gpui::{div, px, FontWeight, InteractiveElement, ParentElement, Styled, Window};
use gpui::prelude::*;
use gpui::{Context, SharedString};

use super::icons::{self, Icon};
use super::nav::{NavGroup, SecmPage, NAV_GROUPS};
use super::theme::Palette;
use super::PiShell;

impl PiShell {
    /// Sidebar 内部内容（外层负责开合裁剪/抽屉位移）
    pub fn render_sidebar_inner(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.palette();
        let width = self.sidebar_panel.width.max(0.0);

        div()
            .flex_col()
            .h_full()
            .overflow_hidden()
            .w(px(width))
            .child(self.sidebar_header(&pal, cx))
            // 导航区：外层 flex_1 占满剩余并裁剪，内层滚动（保证 footer 钉底）
            .child(
                div()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(self.nav_list(&pal, cx)),
            )
        // 注：底部三个工具图标(footer)不在此渲染，统一由外层壳
        // (render_sidebar_column 桌面列 / 移动抽屉)绝对定位钉底，仅一份。
    }

    /// 头部：品牌/拖动行 + controls（主题/折叠）
    /// 无标题栏后，品牌行整行（含顶部，无留白）为窗口拖动热区（按钮区除外）。
    fn sidebar_header(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_col()
            .w_full()
            .pb(px(2.0))
            .gap(px(2.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .w_full()
                    .h(px(34.0))
                    .px(px(4.0))
                    .gap(px(4.0))
                    // 可拖动标题区（logo + 全称 + 弹性空白），占满到按钮前
                    // 无标题栏：按下时发起系统窗口拖动
                    .child(
                        div()
                            .id("pi-sidebar-drag")
                            .flex()
                            .items_center()
                            .flex_1()
                            .min_w(px(0.0))
                            .h_full()
                            .gap(px(6.0))
                            .window_control_area(gpui::WindowControlArea::Drag)
                            .child(
                                div()
                                    .size(px(14.0))
                                    .rounded(px(4.0))
                                    .bg(pal.accent)
                                    .flex()
                                    .items_center()
                                    .justify_center(),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .truncate()
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(pal.text)
                                    .child(SharedString::from("SysEnv Console Manager")),
                            ),
                    )
                    .child(self.chrome_theme_button(pal, cx))
                    .child(self.chrome_collapse_button(pal, cx)),
            )
    }

    fn chrome_theme_button(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let icon = match self.appearance {
            super::theme::Appearance::Dark => Icon::Sun,
            super::theme::Appearance::Light => Icon::Moon,
        };
        let this = cx.entity();
        div()
            .id("pi-chrome-theme")
            .flex()
            .items_center()
            .justify_center()
            .size(px(26.0))
            .rounded(px(7.0))
            .cursor_pointer()
            .text_color(pal.text_muted)
            .hover(|s| s.bg(pal.bg_hover).text_color(pal.text))
            .on_click(move |_ev, _w, cx| {
                let _ = this.update(cx, |t, cx| t.toggle_theme(cx));
            })
            .child(icons::icon(icon, 14.0).text_color(pal.text_muted))
    }

    fn chrome_collapse_button(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        div()
            .id("pi-chrome-collapse")
            .flex()
            .items_center()
            .justify_center()
            .size(px(26.0))
            .rounded(px(7.0))
            .cursor_pointer()
            .text_color(pal.text_muted)
            .hover(|s| s.bg(pal.bg_hover).text_color(pal.text))
            .on_click(move |_ev, _w, cx| {
                let _ = this.update(cx, |t, cx| t.toggle_sidebar(cx));
            })
            .child(icons::icon(Icon::Sidebar, 14.0).text_color(pal.text_muted))
    }

    /// 分组导航列表（滚动区；父层已给 flex_1 尺寸）
    fn nav_list(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("pi-nav-list")
            .flex_col()
            .h_full()
            .overflow_y_scroll()
            .children(NAV_GROUPS.iter().map(|group| self.nav_group(*group, pal, cx)))
    }

    fn nav_group(&self, group: NavGroup, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_col()
            .child(
                div()
                    .px(px(10.0))
                    .py(px(6.0))
                    .text_size(px(10.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(pal.text_dim)
                    .child(SharedString::from(group.title())),
            )
            .children(
                SecmPage::ALL
                    .iter()
                    .filter(|p| group.contains(**p))
                    .map(|p| self.nav_row(*p, pal, cx)),
            )
    }

    /// 单页导航行（30px，图标 + 名称）
    fn nav_row(&self, page: SecmPage, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let active = page == self.current_page;
        let this = cx.entity();
        div()
            .id(page as usize)
            .flex()
            .items_center()
            .gap(px(8.0))
            .w_full()
            .h(px(30.0))
            .mx(px(8.0))
            .px(px(8.0))
            .my(px(1.0))
            .rounded(px(8.0))
            .cursor_pointer()
            .text_color(if active { pal.text } else { pal.text_muted })
            .bg(if active { pal.bg_selected } else { super::theme::TRANSPARENT })
            .hover(|s| s.bg(if active { pal.bg_selected } else { pal.bg_hover }).text_color(pal.text))
            .on_click(move |_ev, _w, cx| {
                let _ = this.update(cx, |t, cx| t.navigate_to(page, cx));
            })
            .child(
                icons::icon(page.icon(), 14.0)
                    .text_color(if active { pal.text } else { pal.text_muted }),
            )
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_size(px(12.5))
                    .font_weight(if active {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .child(SharedString::from(page.label())),
            )
    }

    /// Sidebar footer：设置入口（保留参考外壳的扁平工具按钮）
    pub(crate) fn sidebar_footer(&self, pal: &Palette, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .flex_shrink_0()
            .gap(px(3.0))
            .px(px(8.0))
            .py(px(6.0))
            .bg(pal.bg_panel)
            .border_t_1()
            .border_color(pal.separator)
            .child(self.footer_item(Icon::Gauge, pal))
            .child(self.footer_item(Icon::Info, pal))
            .child(self.footer_item(Icon::Settings, pal))
    }

    fn footer_item(&self, icon: Icon, pal: &Palette) -> impl IntoElement {
        div()
            .id(icon as usize)
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .h(px(28.0))
            .rounded(px(7.0))
            .cursor_pointer()
            .text_color(pal.text_muted)
            .hover(|s| s.bg(pal.bg_hover).text_color(pal.text))
            .child(icons::icon(icon, 14.0).text_color(pal.text_muted))
    }
}
















