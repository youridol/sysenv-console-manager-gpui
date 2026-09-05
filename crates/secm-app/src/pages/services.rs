// secm-app::pages::services — 服务管理页
// 枚举 Windows 服务 + 搜索 + 启停/启动类型。
//
// 并发模型：服务枚举（数百服务，慢）后台线程执行；启停/启动类型为系统 API
// 调用，后台执行 + 完成后延迟后台刷新状态。主线程仅渲染。

use gpui::{div, px, rgb, SharedString, Window, Context, Render, WeakEntity};
use gpui::prelude::*;
use secm_core::settings::{self, ServiceInfo};

use crate::theme::Theme;

pub struct ServicesView {
    services: Vec<ServiceInfo>,
    /// 搜索关键词（匹配 name/display_name）
    keyword: SharedString,
    status: SharedString,
    /// 列表加载中
    loading: bool,
    /// 操作进行中（互斥）
    op_busy: bool,
}

impl ServicesView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut v = Self {
            services: Vec::new(),
            keyword: SharedString::from(""),
            status: SharedString::from("正在加载服务列表…"),
            loading: false,
            op_busy: false,
        };
        v.start_load(cx);
        v
    }

    /// 后台枚举服务（数百项，慢；结果回填 UI）
    fn start_load(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let services = exec
                .spawn(async move { settings::list_all_services().unwrap_or_default() })
                .await;
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.loading = false;
                    this.services = services;
                    this.status = SharedString::from("");
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 延迟 800ms 后台刷新（启停/启动类型异步生效）
    fn reload_later(&mut self, cx: &mut Context<Self>) {
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            gpui::Timer::after(std::time::Duration::from_millis(800)).await;
            let exec = cx.background_executor().clone();
            let services = exec
                .spawn(async move { settings::list_all_services().unwrap_or_default() })
                .await;
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.services = services;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn filtered(&self) -> Vec<&ServiceInfo> {
        if self.keyword.is_empty() {
            return self.services.iter().collect();
        }
        let kw = self.keyword.to_lowercase();
        self.services
            .iter()
            .filter(|s| {
                s.name.to_lowercase().contains(&kw)
                    || s.display_name.to_lowercase().contains(&kw)
            })
            .collect()
    }

    /// 搜索关键词更新（GPUI TextInput 接入后使用）
    #[allow(dead_code)]
    fn set_keyword(&mut self, kw: SharedString, cx: &mut Context<Self>) {
        self.keyword = kw;
        cx.notify();
    }

    /// 启停/启动类型（后台系统 API + 延迟刷新）
    fn op_service(
        &mut self,
        name: &str,
        op: ServiceOp,
        cx: &mut Context<Self>,
    ) {
        if self.op_busy {
            return;
        }
        self.op_busy = true;
        self.status = SharedString::from(format!("正在{} {}…", op.label(), name));
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        let name_c = name.to_string();
        let op_c = op;
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let result = exec
                .spawn(async move {
                    match op_c {
                        ServiceOp::Start => settings::start_service(&name_c),
                        ServiceOp::Stop => settings::stop_service(&name_c),
                        ServiceOp::ToggleAuto => {
                            settings::set_service_start_type(&name_c, "auto")
                        }
                        ServiceOp::ToggleManual => {
                            settings::set_service_start_type(&name_c, "manual")
                        }
                        ServiceOp::ToggleDisable => {
                            settings::set_service_start_type(&name_c, "disabled")
                        }
                    }
                })
                .await;
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.op_busy = false;
                    this.status = match result {
                        Ok(msg) => SharedString::from(msg),
                        Err(e) => SharedString::from(e),
                    };
                    cx.notify();
                })
                .ok();
                view.update(cx, |this, cx| {
                    // 延迟后台刷新（服务状态异步变化）
                    this.reload_later(cx);
                })
                .ok();
            }
        })
        .detach();
    }

    fn status_color(status: &str, theme: &Theme) -> gpui::Rgba {
        match status {
            "Running" => theme.success,
            "Stopped" => theme.text_muted,
            _ => theme.warn,
        }
    }
}

#[derive(Clone, Copy)]
#[allow(dead_code)] // 启动类型切换按钮待接入 UI
enum ServiceOp {
    Start,
    Stop,
    ToggleAuto,
    ToggleManual,
    ToggleDisable,
}

impl ServiceOp {
    fn label(self) -> &'static str {
        match self {
            Self::Start => "启动",
            Self::Stop => "停止",
            Self::ToggleAuto => "设为自动",
            Self::ToggleManual => "设为手动",
            Self::ToggleDisable => "禁用",
        }
    }
}

impl Render for ServicesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::dark();
        let services = self.filtered();

        div()
            .flex_col()
            .size_full()
            .p_6()
            .gap_4()
            // 页头 + 搜索
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap_3()
                            .child(
                                div()
                                    .text_size(px(24.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme.text)
                                    .child("服务管理"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme.text_muted)
                                    .child(SharedString::from(format!(
                                        "{} 个服务",
                                        self.services.len()
                                    ))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.search_box(&theme, cx)),
                    ),
            )
            // 状态消息
            .when(!self.status.is_empty(), |s| {
                let msg = self.status.clone();
                s.child(
                    div()
                        .px_4()
                        .py_2()
                        .rounded_md()
                        .bg(theme.panel_hover)
                        .text_size(px(12.0))
                        .text_color(theme.info)
                        .child(msg),
                )
            })
            // 服务表
            .child(
                crate::ui::table_container(&theme).child(
                    div()
                        .id("svc-scroll")
                        .flex_col()
                        .h(px(520.0))
                        .overflow_scroll()
                        .child(crate::ui::table_head(&theme, &["状态", "服务名", "显示名", "启动类型", "操作"]))
                        .children(services.iter().map(|s| self.service_row(&theme, s, cx))),
                ),
            )
    }
}

impl ServicesView {
    fn search_box(&self, theme: &Theme, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("svc-search")
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1p5()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.panel)
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child("🔍"),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .text_color(theme.text)
                    .child(if self.keyword.is_empty() {
                        SharedString::from("搜索服务名/显示名…（待接入输入）")
                    } else {
                        self.keyword.clone()
                    }),
            )
    }

    fn service_row(
        &self,
        theme: &Theme,
        s: &ServiceInfo,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let name = s.name.clone();
        let status = s.status.clone();
        let display = s.display_name.clone();
        let start_type = s.start_type.clone();
        let color = Self::status_color(&status, theme);

        div()
            .id(SharedString::from(format!("svc-{}", name.clone())))
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            // 状态
            .child(
                div()
                    .w(px(70.0))
                    .flex_none()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .child(div().size(px(6.0)).rounded_full().bg(color))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(color)
                                    .child(status.clone()),
                            ),
                    ),
            )
            // 名称（flex_1）
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.5))
                    .text_color(theme.text)
                    .child(name.clone()),
            )
            // 显示名（固定宽）
            .child(
                div()
                    .w(px(260.0))
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child(display),
            )
            // 启动类型
            .child(
                div()
                    .w(px(70.0))
                    .flex_none()
                    .text_size(px(11.5))
                    .text_color(theme.text_muted)
                    .child(start_type),
            )
            // 操作按钮
            .child(
                div()
                    .w(px(150.0))
                    .flex_none()
                    .flex()
                    .gap_1()
                    .child(service_action_button(theme, "启动", name.clone(), ServiceOp::Start, cx))
                    .child(service_action_button(theme, "停止", name.clone(), ServiceOp::Stop, cx)),
            )
    }
}

/// 操作按钮（Primary/Danger 样式，行内小按钮）
fn service_action_button(
    theme: &Theme,
    label: &str,
    svc: String,
    op: ServiceOp,
    cx: &mut Context<ServicesView>,
) -> impl IntoElement {
    let (bg, hover_bg) = match op {
        ServiceOp::Start | ServiceOp::ToggleAuto | ServiceOp::ToggleManual => {
            (theme.panel_hover, theme.border)
        }
        _ => (theme.panel_hover, rgb(0x7f1d1d)),
    };
    let label_owned = label.to_string();
    div()
        .id(SharedString::from(label_owned.clone() + &svc))
        .px_2()
        .py_0p5()
        .rounded_sm()
        .cursor_pointer()
        .bg(bg)
        .hover(|s| s.bg(hover_bg))
        .text_size(px(11.0))
        .text_color(theme.text)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.op_service(&svc, op, cx);
        }))
        .child(label_owned)
}
