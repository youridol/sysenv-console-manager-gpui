// pi_clone::shell — SECM 三栏桌面壳（唯一主界面；克隆壳迁移旧 11 页导航）
//
// 产品调整（用户指令，2026-09-06）：
//   1. 旧导航（AppRoot/Page/11 页外层切换）迁移到本壳，全链路移除旧导航旧代码；
//   2. 严格三栏 Flex Layout：Sidebar(导航) | Main(工具页内容) | RightPanel(文件工作台)；
//   3. 链路移除所有会话相关功能（项目/会话树、会话行、mock 会话等）。
//
// 页面实体懒加载（沿用旧 P1-11）+ 可见性门控（P1-12，dashboard/logs 轮询开关）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gpui::{
    div, px, Entity, InteractiveElement, ParentElement, Render, SharedString, Styled, Window,
};
use gpui::prelude::*;
use gpui::{Context, MouseButton, MouseDownEvent, MouseMoveEvent};

use crate::pages::about::AboutView;
use crate::pages::ai_environment::AiEnvironmentView;
use crate::pages::cleanup::CleanupView;
use crate::pages::dashboard::DashboardView;
use crate::pages::environment::EnvironmentView;
use crate::pages::hardware::HardwareView;
use crate::pages::logs::LogsView;
use crate::pages::net_config::NetConfigView;
use crate::pages::network::NetworkView;
use crate::pages::services::ServicesView;
use crate::pages::settings::SettingsView;

use super::data::FileTab;
use super::icons::{self, Icon};
use super::layout;
use super::nav::SecmPage;
use super::panel::{GrowDirection, PanelWidth};
use super::theme::{Appearance, Palette};

pub struct PiShell {
    pub appearance: Appearance,
    pub sidebar_open: bool,
    pub sidebar_panel: PanelWidth,
    pub sidebar_display_w: f32,
    pub right_open: bool,
    pub right_panel: PanelWidth,
    pub right_display_w: f32,
    pub current_page: SecmPage,
    pub file_tabs: Vec<FileTab>,
    pub active_file_tab_id: Option<String>,
    pub pages: PageEntities,
}

/// 11 页实体 + 可见性标志（dashboard/logs 轮询门控）
pub struct PageEntities {
    pub flags: Vec<Arc<AtomicBool>>,
    pub dashboard: Option<Entity<DashboardView>>,
    pub logs: Option<Entity<LogsView>>,
    pub settings: Option<Entity<SettingsView>>,
    pub services: Option<Entity<ServicesView>>,
    pub cleanup: Option<Entity<CleanupView>>,
    pub network: Option<Entity<NetworkView>>,
    pub net_config: Option<Entity<NetConfigView>>,
    pub hardware: Option<Entity<HardwareView>>,
    pub environment: Option<Entity<EnvironmentView>>,
    pub ai_environment: Option<Entity<AiEnvironmentView>>,
    pub about: Option<Entity<AboutView>>,
}

impl PageEntities {
    fn new() -> Self {
        let flags: Vec<Arc<AtomicBool>> = SecmPage::ALL
            .iter()
            .map(|p| Arc::new(AtomicBool::new(*p == SecmPage::Dashboard)))
            .collect();
        Self {
            flags,
            dashboard: None,
            logs: None,
            settings: None,
            services: None,
            cleanup: None,
            network: None,
            net_config: None,
            hardware: None,
            environment: None,
            ai_environment: None,
            about: None,
        }
    }

    fn flag_for(&self, page: SecmPage) -> Arc<AtomicBool> {
        let idx = SecmPage::ALL.iter().position(|&p| p == page).unwrap_or(0);
        self.flags[idx].clone()
    }
}

impl PiShell {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            appearance: Appearance::Dark,
            sidebar_open: true,
            sidebar_panel: PanelWidth::new(
                layout::SIDEBAR_MIN_WIDTH,
                layout::SIDEBAR_MAX_WIDTH,
                layout::SIDEBAR_DEFAULT_WIDTH,
                "sidebar-width",
            ),
            sidebar_display_w: layout::SIDEBAR_DEFAULT_WIDTH,
            right_open: false,
            right_panel: PanelWidth::new(
                layout::RIGHT_PANEL_MIN_WIDTH,
                layout::RIGHT_PANEL_MAX_WIDTH,
                layout::RIGHT_PANEL_FALLBACK_WIDTH,
                "right-panel-width",
            ),
            right_display_w: 0.0,
            current_page: SecmPage::Dashboard,
            file_tabs: vec![],
            active_file_tab_id: None,
            pages: PageEntities::new(),

        };
        this.ensure_page(SecmPage::Dashboard, cx);
        this
    }

    pub fn palette(&self) -> Palette {
        Palette::for_appearance(self.appearance)
    }

    pub fn viewport_w(&self, window: &Window) -> f32 {
        f32::from(window.viewport_size().width)
    }

    pub fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        self.appearance = match self.appearance {
            Appearance::Light => Appearance::Dark,
            Appearance::Dark => Appearance::Light,
        };
        cx.notify();
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        cx.notify();
    }

    pub fn toggle_right(&mut self, cx: &mut Context<Self>) {
        self.right_open = !self.right_open;
        if self.right_open && self.right_display_w <= 0.0 {
            self.right_display_w = self.right_panel.width;
        }
        cx.notify();
    }

    /// 切页：停旧页轮询、启新页，懒构造新页实体
    pub fn navigate_to(&mut self, page: SecmPage, cx: &mut Context<Self>) {
        if page == self.current_page {
            return;
        }
        self.pages
            .flag_for(self.current_page)
            .store(false, Ordering::Relaxed);
        self.pages.flag_for(page).store(true, Ordering::Relaxed);
        self.current_page = page;
        self.ensure_page(page, cx);
        cx.notify();
    }

    fn ensure_page(&mut self, page: SecmPage, cx: &mut Context<Self>) {
        match page {
            SecmPage::Dashboard => {
                if self.pages.dashboard.is_none() {
                    let flag = self.pages.flag_for(page);
                    self.pages.dashboard = Some(cx.new(|cx| DashboardView::new(flag, cx)));
                }
            }
            SecmPage::Logs => {
                if self.pages.logs.is_none() {
                    let flag = self.pages.flag_for(page);
                    self.pages.logs = Some(cx.new(|cx| LogsView::new(flag, cx)));
                }
            }
            SecmPage::Settings => {
                if self.pages.settings.is_none() {
                    self.pages.settings = Some(cx.new(SettingsView::new));
                }
            }
            SecmPage::Services => {
                if self.pages.services.is_none() {
                    self.pages.services = Some(cx.new(ServicesView::new));
                }
            }
            SecmPage::Cleanup => {
                if self.pages.cleanup.is_none() {
                    self.pages.cleanup = Some(cx.new(CleanupView::new));
                }
            }
            SecmPage::Network => {
                if self.pages.network.is_none() {
                    self.pages.network = Some(cx.new(NetworkView::new));
                }
            }
            SecmPage::NetConfig => {
                if self.pages.net_config.is_none() {
                    self.pages.net_config = Some(cx.new(NetConfigView::new));
                }
            }
            SecmPage::Hardware => {
                if self.pages.hardware.is_none() {
                    self.pages.hardware = Some(cx.new(HardwareView::new));
                }
            }
            SecmPage::Environment => {
                if self.pages.environment.is_none() {
                    self.pages.environment = Some(cx.new(EnvironmentView::new));
                }
            }
            SecmPage::AiEnvironment => {
                if self.pages.ai_environment.is_none() {
                    self.pages.ai_environment = Some(cx.new(AiEnvironmentView::new));
                }
            }
            SecmPage::About => {
                if self.pages.about.is_none() {
                    self.pages.about = Some(cx.new(|_| AboutView::new()));
                }
            }
        }
    }

    fn current_page_view(&self) -> impl IntoElement {
        match self.current_page {
            SecmPage::Dashboard => self
                .pages
                .dashboard
                .as_ref()
                .map(|e| e.clone().into_any_element()),
            SecmPage::Logs => self
                .pages
                .logs
                .as_ref()
                .map(|e| e.clone().into_any_element()),
            SecmPage::Settings => self
                .pages
                .settings
                .as_ref()
                .map(|e| e.clone().into_any_element()),
            SecmPage::Services => self
                .pages
                .services
                .as_ref()
                .map(|e| e.clone().into_any_element()),
            SecmPage::Cleanup => self
                .pages
                .cleanup
                .as_ref()
                .map(|e| e.clone().into_any_element()),
            SecmPage::Network => self
                .pages
                .network
                .as_ref()
                .map(|e| e.clone().into_any_element()),
            SecmPage::NetConfig => self
                .pages
                .net_config
                .as_ref()
                .map(|e| e.clone().into_any_element()),
            SecmPage::Hardware => self
                .pages
                .hardware
                .as_ref()
                .map(|e| e.clone().into_any_element()),
            SecmPage::Environment => self
                .pages
                .environment
                .as_ref()
                .map(|e| e.clone().into_any_element()),
            SecmPage::AiEnvironment => self
                .pages
                .ai_environment
                .as_ref()
                .map(|e| e.clone().into_any_element()),
            SecmPage::About => self
                .pages
                .about
                .as_ref()
                .map(|e| e.clone().into_any_element()),
        }
        .unwrap_or_else(|| div().into_any_element())
    }
}

/// 每帧向目标收拢（指数缓动，~0.2s 视觉节奏）
const SETTLE: f32 = 0.30;

impl PiShell {
    fn settle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut dirty = false;

        if self.sidebar_open && !self.sidebar_panel.resizing {
            let target = self.sidebar_panel.width;
            let diff = target - self.sidebar_display_w;
            if diff.abs() > 0.4 {
                self.sidebar_display_w += diff * SETTLE;
                dirty = true;
            } else {
                self.sidebar_display_w = target;
            }
        } else if !self.sidebar_open
            && !self.sidebar_panel.resizing
            && self.sidebar_display_w > 0.0
        {
            let diff = -self.sidebar_display_w;
            if diff.abs() > 0.4 {
                self.sidebar_display_w += diff * SETTLE;
                dirty = true;
            } else {
                self.sidebar_display_w = 0.0;
            }
        }

        if self.right_open && !self.right_panel.resizing {
            let target = self.right_panel.width;
            let diff = target - self.right_display_w;
            if diff.abs() > 0.4 {
                self.right_display_w += diff * SETTLE;
                dirty = true;
            } else {
                self.right_display_w = target;
            }
        } else if !self.right_open && !self.right_panel.resizing && self.right_display_w > 0.0 {
            let diff = -self.right_display_w;
            if diff.abs() > 0.4 {
                self.right_display_w += diff * SETTLE;
                dirty = true;
            } else {
                self.right_display_w = 0.0;
            }
        }

        if dirty {
            window.request_animation_frame();
            cx.notify();
        }
    }
}

impl PiShell {
    pub fn render_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_page(self.current_page, cx);
        self.settle(window, cx);

        let pal = self.palette();
        let vw = self.viewport_w(window);
        let mobile = layout::is_mobile(vw);
        let split = layout::is_split_panel(vw);

        let sb_max = layout::sidebar_max_width(vw, self.right_open, self.right_panel.width);
        self.sidebar_panel
            .set_bounds(layout::SIDEBAR_MIN_WIDTH, sb_max);
        let rp_max = layout::right_panel_max_width(vw, self.sidebar_open, self.sidebar_panel.width);
        self.right_panel
            .set_bounds(layout::RIGHT_PANEL_MIN_WIDTH, rp_max);

        let sb_open = self.sidebar_open;
        let sb_width = if mobile { 292.0 } else { self.sidebar_display_w };

        let mut shell = div()
            .id("pi-app-shell")
            .flex()
            .size_full()
            .overflow_hidden()
            .bg(pal.bg);

        shell = shell.child(self.render_sidebar_column(sb_open, sb_width, mobile, window, cx));
        if !mobile && sb_open && self.sidebar_display_w > 1.0 {
            shell = shell.child(self.render_sidebar_divider(window, cx));
        }
        shell = shell.child(self.render_main(window, cx));
        if split && self.right_open && self.right_display_w > 1.0 {
            shell = shell.child(self.render_right_divider(window, cx));
        }
        if mobile {
            if self.right_open {
                shell = shell.child(self.render_right_panel(true, window, cx));
            }
        } else if self.right_open {
            shell = shell.child(self.render_right_panel(false, window, cx));
        }

        shell
    }

    fn render_sidebar_column(
        &self,
        open: bool,
        width: f32,
        mobile: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pal = self.palette();
        let inner_w = if mobile { 292.0 } else { self.sidebar_panel.width };

        // 内层内容仅 header + 导航；底部工具条(footer)由本函数统一置底渲染一份，
        // 避免与导航尾部（关于分组下方）重复出现。pb(40) 为 footer 预留空间。
        let content = div()
            .w(px(inner_w))
            .h_full()
            .pb(px(40.0))
            .child(self.render_sidebar_inner(window, cx));

        // Footer 钉在侧栏最底（absolute 于容器底部），桌面/移动抽屉共用同一份
        let footer = div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .w(px(inner_w))
            .child(self.sidebar_footer(&pal, cx));

        if mobile {
            return div()
                .id("pi-sidebar-drawer")
                .absolute()
                .top_0()
                .left_0()
                .bottom_0()
                .w(px(292.0))
                .flex_shrink_0()
                .relative()
                .overflow_hidden()
                .bg(pal.bg_panel)
                .border_r_1()
                .border_color(pal.separator)
                .child(content)
                .child(footer)
                .when(!open, |s| s.hidden())
                .into_any_element();
        }

        div()
            .id("pi-sidebar-col")
            .flex_shrink_0()
            .h_full()
            .overflow_hidden()
            .relative()
            .w(px(width))
            .when(!open, |s| s.w(px(0.0)))
            .bg(pal.bg_panel)
            .border_r_1()
            .border_color(pal.separator)
            .child(content)
            .child(footer)
            .into_any_element()
    }

    fn render_sidebar_divider(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        self.divider(
            "pi-sidebar-divider",
            self.sidebar_panel.resizing,
            {
                let this = this.clone();
                move |x: f32, _w, cx| {
                    let _ = this.update(cx, |t, _| t.sidebar_drag_down(x));
                }
            },
            {
                let this = this.clone();
                move |x: f32, _w, cx| {
                    let _ = this.update(cx, |t, _| t.sidebar_drag_move(x, GrowDirection::Right));
                }
            },
            {
                let this = this.clone();
                move |_w, cx| {
                    let _ = this.update(cx, |t, _| t.sidebar_drag_up());
                }
            },
        )
    }

    fn render_right_divider(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        self.divider(
            "pi-right-divider",
            self.right_panel.resizing,
            {
                let this = this.clone();
                move |x: f32, _w, cx| {
                    let _ = this.update(cx, |t, _| t.right_drag_down(x));
                }
            },
            {
                let this = this.clone();
                move |x: f32, _w, cx| {
                    let _ = this.update(cx, |t, _| t.right_drag_move(x, GrowDirection::Left));
                }
            },
            {
                let this = this.clone();
                move |_w, cx| {
                    let _ = this.update(cx, |t, _| t.right_drag_up());
                }
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn divider(
        &self,
        id: &'static str,
        resizing: bool,
        on_down: impl Fn(f32, &mut Window, &mut gpui::App) + 'static,
        on_move: impl Fn(f32, &mut Window, &mut gpui::App) + 'static,
        on_up: impl Fn(&mut Window, &mut gpui::App) + 'static,
    ) -> impl IntoElement {
        use gpui::CursorStyle;
        let on_up: std::rc::Rc<dyn Fn(&mut Window, &mut gpui::App)> = std::rc::Rc::new(on_up);
        let up_a = on_up.clone();
        let up_b = on_up.clone();
        let line = if resizing {
            gpui::rgb(0x6e6e73).into()
        } else {
            gpui::transparent_black()
        };
        div()
            .id(id)
            .w(px(12.0))
            .mx(px(-6.0))
            .flex_shrink_0()
            .relative()
            .cursor(CursorStyle::ResizeColumn)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(5.0))
                    .w(px(1.0))
                    .bg(line)
                    .hover(|s| s.bg(gpui::rgb(0x8e8e93))),
            )
            .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, window, cx| {
                on_down(event.position.x.into(), window, cx);
            })
            .on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                on_move(event.position.x.into(), window, cx);
            })
            .on_mouse_up(MouseButton::Left, move |_ev, window, cx| (up_a)(window, cx))
            .on_mouse_up_out(MouseButton::Left, move |_ev, window, cx| (up_b)(window, cx))
    }

    pub fn sidebar_drag_down(&mut self, pointer_x: f32) {
        self.sidebar_panel.begin_drag(pointer_x);
    }
    pub fn sidebar_drag_move(&mut self, pointer_x: f32, dir: GrowDirection) {
        if self.sidebar_panel.resizing {
            self.sidebar_panel.drag_to(pointer_x, dir);
            if self.sidebar_open {
                self.sidebar_display_w = self.sidebar_panel.width;
            }
        }
    }
    pub fn sidebar_drag_up(&mut self) {
        if self.sidebar_panel.resizing {
            self.sidebar_panel.end_drag();
            if self.sidebar_open {
                self.sidebar_display_w = self.sidebar_panel.width;
            }
        }
    }
    pub fn right_drag_down(&mut self, pointer_x: f32) {
        self.right_panel.begin_drag(pointer_x);
    }
    pub fn right_drag_move(&mut self, pointer_x: f32, dir: GrowDirection) {
        if self.right_panel.resizing {
            self.right_panel.drag_to(pointer_x, dir);
            if self.right_open {
                self.right_display_w = self.right_panel.width;
            }
        }
    }
    pub fn right_drag_up(&mut self) {
        if self.right_panel.resizing {
            self.right_panel.end_drag();
            if self.right_open {
                self.right_display_w = self.right_panel.width;
            }
        }
    }

    fn render_main(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_1()
            .flex_col()
            .min_w(px(0.0))
            .h_full()
            .overflow_hidden()
            .child(self.render_top_bar(window, cx))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(self.current_page_view()),
            )
    }

    fn render_top_bar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.palette();
        let mut bar = div()
            .id("pi-topbar")
            .flex()
            .items_center()
            .flex_shrink_0()
            .h(px(layout::TOP_BAR_HEIGHT))
            .px(px(9.0))
            .gap(px(4.0))
            .border_b_1()
            .border_color(pal.separator)
            .bg(pal.bg_panel);

        if !self.sidebar_open {
            bar = bar.child(self.sidebar_reopen_button(&pal, cx));
        }

        bar = bar
            // 标题区：无标题栏窗口由此拖动
            .child(
                div()
                    .id("pi-topbar-title")
                    .flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .h_full()
                    .items_center()
                    .window_control_area(gpui::WindowControlArea::Drag)
                    .child(
                        div()
                            .flex_col()
                            .justify_center()
                            .min_w(px(72.0))
                            .ml(px(4.0))
                            .px(px(7.0))
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(pal.text)
                                    .truncate()
                                    .child(SharedString::from("SECM — 系统环境管理器")),
                            )
                            .child(
                                div()
                                    .mt(px(2.0))
                                    .text_size(px(10.5))
                                    .text_color(pal.text_muted)
                                    .child(SharedString::from(self.current_page.label())),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .h_full()
                    .gap(px(1.0))
                    // 无标题栏：自绘窗口控制（最小化 / 最大化 / 关闭）
                    .child(self.window_control_min())
                    .child(self.window_control_max())
                    .child(self.window_control_close())
                    // 右栏开关放在最右（关闭按钮右侧）
                    .child(self.right_toggle_button(&pal, cx)),
            );

        bar
    }

    // ── 自绘窗口控制按钮（右上角；点击走原生窗口动作）──
    fn window_control_min(&self) -> impl IntoElement {
        let pal = self.palette();
        div()
            .id("pi-win-min")
            .flex()
            .items_center()
            .justify_center()
            .w(px(46.0))
            .h_full()
            .text_color(pal.text_muted)
            .hover(|s| s.bg(pal.bg_hover).text_color(pal.text))
            .on_mouse_down(gpui::MouseButton::Left, |_ev, window, _cx| {
                if let Some(hwnd) = crate::win32::hwnd_from_window(window) {
                    crate::win32::minimize_window(hwnd);
                }
            })
            .child(icons::icon(Icon::WinMin, 12.0).text_color(pal.text_muted))
    }

    fn window_control_max(&self) -> impl IntoElement {
        let pal = self.palette();
        div()
            .id("pi-win-max")
            .flex()
            .items_center()
            .justify_center()
            .w(px(46.0))
            .h_full()
            .text_color(pal.text_muted)
            .hover(|s| s.bg(pal.bg_hover).text_color(pal.text))
            .on_mouse_down(gpui::MouseButton::Left, |_ev, window, _cx| {
                window.zoom_window();
            })
            .child(icons::icon(Icon::WinMax, 12.0).text_color(pal.text_muted))
    }

    fn window_control_close(&self) -> impl IntoElement {
        let pal = self.palette();
        div()
            .id("pi-win-close")
            .flex()
            .items_center()
            .justify_center()
            .w(px(46.0))
            .h_full()
            .text_color(pal.text_muted)
            .hover(|s| s.bg(gpui::rgb(0xe81123)).text_color(gpui::rgb(0xffffff)))
            .on_mouse_down(gpui::MouseButton::Left, |_ev, window, _cx| {
                if let Some(hwnd) = crate::win32::hwnd_from_window(window) {
                    crate::win32::close_window(hwnd);
                }
            })
            .child(icons::icon(Icon::WinClose, 12.0).text_color(pal.text_muted))
    }

    fn sidebar_reopen_button(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        div()
            .id("pi-sidebar-reopen")
            .flex()
            .items_center()
            .justify_center()
            .size(px(layout::TOP_BAR_ICON_BUTTON_SIZE))
            .flex_shrink_0()
            .border_r_1()
            .border_color(pal.separator)
            .cursor_pointer()
            .text_color(pal.text_muted)
            .hover(|s| s.text_color(pal.text))
            .on_click(move |_ev, _w, cx| {
                let _ = this.update(cx, |t, cx| t.toggle_sidebar(cx));
            })
            .child(icons::icon(Icon::Sidebar, 16.0).text_color(pal.text_muted))
    }

    fn right_toggle_button(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        div()
            .id("pi-right-toggle")
            .flex()
            .items_center()
            .justify_center()
            .w(px(layout::RIGHT_PANEL_TOGGLE_W))
            .h(px(layout::RIGHT_PANEL_TOGGLE_H))
            .ml(px(4.0))
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

}

impl Render for PiShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_shell(window, cx)
    }
}













