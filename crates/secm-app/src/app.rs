// secm-app 应用根：页面枚举 + 主布局（侧边栏导航）
// ADR-0003：单窗口 + Page 枚举导航；控件走 theme。

use gpui::{
    div, px, Context, Entity, Render, SharedString, Window,
};
use gpui::prelude::*;

use crate::pages::about::AboutView;
use crate::pages::cleanup::CleanupView;
use crate::pages::dashboard::DashboardView;
use crate::pages::hardware::HardwareView;
use crate::pages::logs::LogsView;
use crate::pages::net_config::NetConfigView;
use crate::pages::network::NetworkView;
use crate::pages::services::ServicesView;
use crate::pages::settings::SettingsView;
use crate::theme::Theme;

/// 页面枚举（对齐源 11 路由）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Cleanup,
    Network,
    NetConfig,
    Settings,
    Services,
    Environment,
    AiEnvironment,
    Hardware,
    Logs,
    About,
}

impl Page {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dashboard => "硬件信息",
            Self::Cleanup => "清理优化",
            Self::Network => "网络诊断",
            Self::NetConfig => "网络配置",
            Self::Settings => "系统设置",
            Self::Services => "服务管理",
            Self::Environment => "环境检测",
            Self::AiEnvironment => "AI 环境",
            Self::Hardware => "硬件检测",
            Self::Logs => "调试日志",
            Self::About => "关于",
        }
    }

    pub const ALL: &'static [Page] = &[
        Page::Dashboard,
        Page::Cleanup,
        Page::Network,
        Page::NetConfig,
        Page::Settings,
        Page::Services,
        Page::Environment,
        Page::AiEnvironment,
        Page::Hardware,
        Page::Logs,
        Page::About,
    ];
}

/// 应用根视图：侧边栏 + 当前页内容
pub struct AppRoot {
    theme: Theme,
    current: Page,
    dashboard: Entity<DashboardView>,
    logs: Entity<LogsView>,
    settings: Entity<SettingsView>,
    services: Entity<ServicesView>,
    cleanup: Entity<CleanupView>,
    network: Entity<NetworkView>,
    net_config: Entity<NetConfigView>,
    hardware: Entity<HardwareView>,
    about: Entity<AboutView>,
}

impl AppRoot {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let theme = Theme::dark();
        let dashboard = cx.new(|cx| DashboardView::new(cx));
        let logs = cx.new(|cx| LogsView::new(cx));
        let settings = cx.new(|cx| SettingsView::new(cx));
        let services = cx.new(|cx| ServicesView::new(cx));
        let cleanup = cx.new(|cx| CleanupView::new(cx));
        let network = cx.new(|cx| NetworkView::new(cx));
        let net_config = cx.new(|cx| NetConfigView::new(cx));
        let hardware = cx.new(|cx| HardwareView::new(cx));
        let about = cx.new(|_| AboutView::new());
        Self {
            theme,
            current: Page::Dashboard,
            dashboard,
            logs,
            settings,
            services,
            cleanup,
            network,
            net_config,
            hardware,
            about,
        }
    }

    fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        if page != self.current {
            self.current = page;
            cx.notify();
        }
    }

    /// 侧边栏导航项（cx.listener 绑定实体回调）
    fn nav_item(
        &self,
        page: Page,
        cx: &mut Context<Self>,
    ) -> impl gpui::IntoElement + '_ {
        let theme = self.theme;
        let active = page == self.current;
        let label = page.label().to_string();
        div()
            .id(page as usize)
            .flex()
            .items_center()
            .px_3()
            .py_1p5()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(13.0))
            .when(active, |s| {
                s.bg(theme.panel_hover)
                    .text_color(theme.brand)
                    .font_weight(gpui::FontWeight::MEDIUM)
            })
            .when(!active, |s| s.text_color(theme.text_muted))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.navigate(page, cx);
            }))
            .child(label)
    }
}

impl Render for AppRoot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        div()
            .flex()
            .size_full()
            .bg(theme.bg)
            // 侧边栏
            .child(
                div()
                    .flex_col()
                    .w(px(210.0))
                    .h_full()
                    .border_r_1()
                    .border_color(theme.border)
                    .bg(theme.panel)
                    .px_2()
                    .py_4()
                    .gap_0p5()
                    .child(
                        div()
                            .px_2()
                            .pb_3()
                            .child(
                                div()
                                    .text_size(px(17.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme.text)
                                    .child("SECM"),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(theme.text_muted)
                                    .child("系统环境管理器 v2.0.0"),
                            ),
                    )
                    .children(Page::ALL.iter().map(|&p| self.nav_item(p, cx))),
            )
            // 内容区
            .child(
                div()
                    .id("content")
                    .flex_1()
                    .h_full()
                    .overflow_scroll()
                    .child(self.content_view()),
            )
    }
}

impl AppRoot {
    fn content_view(&self) -> impl IntoElement {
        match self.current {
            Page::Dashboard => self.dashboard.clone().into_any_element(),
            Page::Logs => self.logs.clone().into_any_element(),
            Page::Settings => self.settings.clone().into_any_element(),
            Page::Services => self.services.clone().into_any_element(),
            Page::Cleanup => self.cleanup.clone().into_any_element(),
            Page::Network => self.network.clone().into_any_element(),
            Page::NetConfig => self.net_config.clone().into_any_element(),
            Page::Hardware => self.hardware.clone().into_any_element(),
            Page::About => self.about.clone().into_any_element(),
            other => {
                let theme = self.theme;
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_color(theme.text_muted)
                            .text_size(px(14.0))
                            .child(SharedString::from(format!(
                                "「{}」页面 — 后续 Phase 实现",
                                other.label()
                            ))),
                    )
                    .into_any_element()
            }
        }
    }
}
