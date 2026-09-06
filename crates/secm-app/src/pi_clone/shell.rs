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
    /// 侧栏底部「本机 IP」卡数据（首个 Up 网卡 IPv4；None = 未取到/未连接）
    pub local_ip: Option<String>,
    /// IP 卡读取中（后台枚举未回）
    pub ip_loading: bool,
    /// 侧栏网络信息卡数据（6 字段 + 上下行速率）
    pub net_info: Option<secm_core::net_info::NetInfo>,
    /// 网络信息卡读取中（后台采集未回）
    pub net_info_loading: bool,
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
            local_ip: None,
            ip_loading: false,
            net_info: None,
            net_info_loading: false,

        };
        this.ensure_page(SecmPage::Dashboard, cx);
        this.refresh_local_ip(cx);
        this.refresh_net_info(cx);
        this.start_net_rate_poll(cx);
        this
    }

    /// 启动侧栏网络速率周期刷新（1s；PDH 需要 ≥1s 间隔累积样本）。
    /// 仅刷新本地链路字段（无网络请求），驱动上下行速率滚动。
    fn start_net_rate_poll(&self, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let mut tick = gpui::Timer::after(std::time::Duration::from_secs(1));
            loop {
                tick.await;
                if let Some(shell) = weak.upgrade() {
                    shell
                        .update(cx, |t, cx| t.refresh_net_rate(cx))
                        .ok();
                }
                tick = gpui::Timer::after(std::time::Duration::from_secs(1));
            }
        })
        .detach();
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

    /// 后台读取本机首个 Up 网卡 IPv4（GetAdaptersAddresses，不阻塞主线程）。
    /// 供侧栏底部 IP 卡展示；点击卡片可再次调用刷新。
    pub fn refresh_local_ip(&mut self, cx: &mut Context<Self>) {
        if self.ip_loading {
            return;
        }
        self.ip_loading = true;
        cx.notify();

        let weak: gpui::WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let adapters = exec
                .spawn(async move { secm_core::netif::list_adapters().unwrap_or_default() })
                .await;
            // 首个 Up 适配器的第一个 IPv4；无 Up 或空则显示「未连接」
            let ip = adapters
                .iter()
                .find(|a| a.status == "Up")
                .and_then(|a| a.ipv4.first())
                .cloned();
            if let Some(shell) = weak.upgrade() {
                shell.update(cx, |this, cx| {
                    this.ip_loading = false;
                    this.local_ip = ip;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 后台采集侧栏「网络信息」全量数据（速率/本地 v4/公网 4 槽，见 net_info）。
    /// PDH 速率需 ≥1s 间隔自然累积：首次调用建立基线，此后每次刷新拿到最近 ~1s 均值。
    pub fn refresh_net_info(&mut self, cx: &mut Context<Self>) {
        if self.net_info_loading {
            return;
        }
        self.net_info_loading = true;
        cx.notify();

        let weak: gpui::WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let info = exec
                .spawn(async move { secm_core::net_info::collect_net_info() })
                .await;
            if let Some(shell) = weak.upgrade() {
                shell.update(cx, |this, cx| {
                    this.net_info_loading = false;
                    this.net_info = Some(info);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 后台轻量刷新本地链路字段（协商速率/本地 IPv4/实时上下行速率，无网络请求）。
    /// 由侧栏周期任务每秒调用，驱动 PDH 速率滚动；保留既有公网槽。
    pub fn refresh_net_rate(&mut self, cx: &mut Context<Self>) {
        if self.net_info_loading {
            return;
        }
        // 首次尚未建立数据 → 走全量（含公网）
        let Some(current) = self.net_info.clone() else {
            return self.refresh_net_info(cx);
        };
        self.net_info_loading = true;
        cx.notify();

        let weak: gpui::WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            // 后台克隆一份做本地刷新（不动主线程共享数据）
            let mut next = current;
            let updated = exec
                .spawn(async move {
                    secm_core::net_info::refresh_local_rate(&mut next);
                    next
                })
                .await;
            if let Some(shell) = weak.upgrade() {
                shell.update(cx, |this, cx| {
                    this.net_info_loading = false;
                    this.net_info = Some(updated);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
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

        // 侧栏固定底部区（absolute 钉底）：本机 IP 卡 + 底部工具条(footer)，
        // 均不随导航滚动；与导航列表尾部不重复。
        // 预留高度：IP 卡 ~58 + footer ~40。
        let fixed = div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .w(px(inner_w))
            .flex_col()
            .child(self.local_ip_card(&pal, cx))
            .child(self.sidebar_footer(&pal, cx));

        // 内层内容仅 header + 导航；pb 预留给底部固定区（IP 卡 + footer）
        let content = div()
            .w(px(inner_w))
            .h_full()
            .pb(px(210.0))
            .child(self.render_sidebar_inner(window, cx));

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
                .child(fixed)
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
            .child(fixed)
            .into_any_element()
    }

    /// 侧栏底部「网络信息」卡（固定于 footer 上方；点击重新读取）。
    ///
    /// 复原旧版侧栏网络信息检测（用户指令 2026-09-06）：已连接网卡上下行速率 +
    /// 协商速率 + 本地 IPv4 + 公网 4 槽（国内/国外 × IPv4/IPv6）。数据来自
    /// secm_core::net_info（后台采集，见 refresh_net_info）。
    fn local_ip_card(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let loading = self.net_info_loading;
        let ni = self.net_info.clone().unwrap_or_default();
        // 降级标题：卡片下方展示本地 IPv4（兼容旧「本机 IP」语义）
        let local_v4 = if ni.local_ipv4.is_empty() {
            self.local_ip.clone().unwrap_or_default()
        } else {
            ni.local_ipv4.clone()
        };

        // 格式化函数：KB/s → 人类可读
        fn fmt_rate(kbps: f32) -> String {
            if kbps < 0.1 {
                "0 KB/s".to_string()
            } else if kbps < 1024.0 {
                format!("{:.0} KB/s", kbps)
            } else {
                format!("{:.2} MB/s", kbps / 1024.0)
            }
        }

        // 公网槽 label 值对（行标签含区域；值=IP 或「—」；后缀=失败标）
        let pub_rows: Vec<(String, String, String)> = vec![
            (
                "公网 v4 · 国内".into(),
                label_ip(&ni.pub_v4_domestic),
                fail_of(&ni.pub_v4_domestic),
            ),
            (
                "公网 v4 · 国外".into(),
                label_ip(&ni.pub_v4_abroad),
                fail_of(&ni.pub_v4_abroad),
            ),
            (
                "公网 v6 · 国内".into(),
                label_ip(&ni.pub_v6_domestic),
                fail_of(&ni.pub_v6_domestic),
            ),
            (
                "公网 v6 · 国外".into(),
                label_ip(&ni.pub_v6_abroad),
                fail_of(&ni.pub_v6_abroad),
            ),
        ];

        div()
            .id("pi-sidebar-net-card")
            .mx(px(8.0))
            .mb(px(4.0))
            .px(px(10.0))
            .py(px(8.0))
            .rounded(px(9.0))
            .border_1()
            .border_color(pal.separator)
            .bg(pal.surface_muted)
            .cursor_pointer()
            .on_click(move |_ev, _w, cx| {
                let _ = this.update(cx, |t, cx| t.refresh_net_info(cx));
            })
            // 头部：网卡名 + 协商速率 + 上下行
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(
                        icons::icon(Icon::Wifi, 12.0).text_color(if ni.adapter_name.is_empty() {
                            pal.text_dim
                        } else {
                            pal.success
                        }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .text_size(px(10.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal.text_muted)
                            .child(SharedString::from(if loading {
                                "读取中…".to_string()
                            } else if ni.adapter_name.is_empty() {
                                "未连接".to_string()
                            } else {
                                format!("{} · {}", ni.adapter_name, ni.link_speed)
                            })),
                    ),
            )
            // 上下行速率行
            .child(
                div()
                    .mt(px(4.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(self.rate_pill(pal, "↓ 下行".into(), fmt_rate(ni.rate.rx_kbps)))
                    .child(self.rate_pill(pal, "↑ 上行".into(), fmt_rate(ni.rate.tx_kbps))),
            )
            // 本地 IPv4
            .child(
                div()
                    .mt(px(5.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(pal.text_dim)
                            .child(SharedString::from("本地 IPv4")),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(pal.text)
                            .child(SharedString::from(if local_v4.is_empty() {
                                "—".to_string()
                            } else {
                                local_v4
                            })),
                    ),
            )
            // 公网 4 槽
            .children(pub_rows.into_iter().map(|(k, v, d)| {
                div()
                    .mt(px(2.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(9.5))
                            .text_color(pal.text_dim)
                            .child(SharedString::from(k)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .min_w(px(0.0))
                            .child(
                                div()
                                    .max_w(px(130.0))
                                    .truncate()
                                    .text_size(px(10.5))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(if v == "—" { pal.text_dim } else { pal.text })
                                    .child(SharedString::from(v.clone())),
                            )
                            .when(!d.is_empty(), |s| {
                                s.child(
                                    div()
                                        .text_size(px(9.0))
                                        .text_color(pal.danger)
                                        .child(SharedString::from(d.clone())),
                                )
                            }),
                    )
            }))
            .into_any_element()
    }

    /// 上下行速率小标签（等宽两列展示）
    fn rate_pill(&self, pal: &Palette, label: String, value: String) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(
                div()
                    .text_size(px(9.5))
                    .text_color(pal.text_dim)
                    .child(SharedString::from(label)),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(11.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(pal.text)
                    .child(SharedString::from(value)),
            )
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

// ── 侧栏网络信息卡：公网槽格式化辅助 ──

/// 公网槽展示值：槽内有 IP 显示 "IP"，槽空显示 "—"
fn label_ip(e: &secm_core::net_info::PublicIpEntry) -> String {
    if e.ip.is_empty() {
        "—".to_string()
    } else {
        e.ip.clone()
    }
}

/// 公网槽失败标：取数失败显示「失败」，成功为空
fn fail_of(e: &secm_core::net_info::PublicIpEntry) -> String {
    if !e.diag.is_empty() {
        "失败".to_string()
    } else {
        String::new()
    }
}













