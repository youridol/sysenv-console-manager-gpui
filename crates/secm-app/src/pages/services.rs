// secm-app::pages::services — 服务管理页
// 枚举 Windows 服务 + 搜索 + 启停/启动类型。
//
// 并发模型：服务枚举（数百服务，慢）后台线程执行；启停/启动类型为系统 API
// 调用，后台执行 + 完成后延迟后台刷新状态。主线程仅渲染。
//
// 呈现层：统一接入 crate::ui::page 布局框架，色板取自 pi_clone::theme::Palette
// （明暗双主题，随壳 set_appearance 联动），禁止硬编码业务色。

use gpui::{div, px, Entity, SharedString, Window, Context, Render, WeakEntity};
use gpui::prelude::*;
use secm_core::settings::{self, ServiceInfo};

use crate::pi_clone::theme::{Appearance, Palette};
use crate::ui::page::{
    banner, button_sm, card, page_header, page_root, status_pill, table_head, table_row,
    BannerKind, ButtonKind, ColWidth,
};
use crate::ui::text_input::{ChangeText, TextField};

pub struct ServicesView {
    services: Vec<ServiceInfo>,
    /// 搜索关键词（匹配 name/display_name）
    keyword: SharedString,
    status: SharedString,
    /// 列表加载中
    loading: bool,
    /// 操作进行中（互斥）
    op_busy: bool,
    /// 搜索输入框（P2：历史为静态占位文案，搜索从未接线）
    search_input: Entity<TextField>,
    /// 页面外观，随壳主题联动
    appearance: Appearance,
    /// 页面滚动状态（GPUI 0.2 滚轮需 track_scroll 手动驱动，见 ui::page::page_root）
    page_scroll: gpui::ScrollHandle,
}

impl ServicesView {
    pub fn new(appearance: Appearance, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| TextField::new("", "搜索服务名/显示名", cx));
        cx.subscribe(
            &search_input,
            |this, field: Entity<TextField>, _ev: &ChangeText, cx| {
                this.set_keyword(field.read(cx).value(), cx);
            },
        )
        .detach();
        let mut v = Self {
            services: Vec::new(),
            keyword: SharedString::from(""),
            status: SharedString::from("正在加载服务列表…"),
            loading: false,
            op_busy: false,
            search_input,
            appearance,
            page_scroll: gpui::ScrollHandle::new(),
        };
        log::info!("服务管理 · 页面已打开");
        v.start_load(cx);
        v
    }

    /// 当前页面色板（明暗随壳联动）
    fn pal(&self) -> Palette {
        Palette::for_appearance(self.appearance)
    }

    /// 壳切换主题时同步外观（PiShell::toggle_theme 联动调用）
    pub(crate) fn set_appearance(&mut self, appearance: Appearance, cx: &mut Context<Self>) {
        self.appearance = appearance;
        cx.notify();
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
            // UI 侧日志：服务枚举完成（记录枚举到的数量）
            log::info!("服务管理 · 服务枚举完成，共 {} 个服务", services.len());
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

    /// 搜索关键词更新（由搜索输入框 ChangeText 订阅驱动）
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
        let name_log = name_c.clone();
        let op_c = op;
        let op_label = op.label().to_string();
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
            // UI 侧日志：服务启停/启动类型操作结果
            match &result {
                Ok(msg) => log::info!("服务管理 · 已{}服务 {}：{}", op_label, name_log, msg),
                Err(e) => log::warn!("服务管理 · {}服务 {} 失败: {}", op_label, name_log, e),
            }
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

    /// 状态点颜色（Running=成功绿 / Stopped=弱化文本 / 其他=警示黄）
    fn status_color(status: &str, pal: &Palette) -> gpui::Rgba {
        match status {
            "Running" => pal.success,
            "Stopped" => pal.text_muted,
            _ => pal.warning,
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
        let pal = self.pal();
        let services = self.filtered();

        page_root(&pal, "services-page-root", &self.page_scroll, &cx.entity())
            // 页头（标题+副标题）+ 右侧搜索框
            .child(
                page_header(
                    &pal,
                    "服务管理",
                    format!("{} 个服务 · Windows 服务枚举", self.services.len()),
                )
                .child(self.search_box(&pal)),
            )
            // 状态消息（非空才显示）
            .when(!self.status.is_empty(), |s| {
                let msg = self.status.clone();
                s.child(banner(&pal, BannerKind::Info, msg))
            })
            // 服务表（卡片内滚动容器）
            .child(
                card(&pal).child(
                    div()
                        .id("svc-scroll")
                        .flex_col()
                        .h(px(520.0))
                        .overflow_scroll()
                        .child(table_head(
                            &pal,
                            &[
                                ("状态", ColWidth::Px(80.0)),
                                ("服务名", ColWidth::Flex),
                                ("显示名", ColWidth::Px(260.0)),
                                ("启动类型", ColWidth::Px(90.0)),
                                ("操作", ColWidth::Px(120.0)),
                            ],
                        ))
                        .children(services.iter().map(|s| self.service_row(&pal, s, cx))),
                ),
            )
    }
}

impl ServicesView {
    /// 页头右侧搜索框（保留 search_input 实体与 🔍 结构）
    fn search_box(&self, pal: &Palette) -> impl IntoElement {
        div()
            .id("svc-search")
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1p5()
            .rounded(px(8.0))
            .border_1()
            .border_color(pal.border)
            .bg(pal.bg_hover)
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(pal.text_muted)
                    .child("🔍"),
            )
            // 真实输入框（P2：搜索功能接线）
            .child(
                div()
                    .flex_1()
                    .child(self.search_input.clone()),
            )
    }

    /// 单行服务数据（单元格列宽与表头完全一致：80/Flex/260/90/120）
    fn service_row(
        &self,
        pal: &Palette,
        s: &ServiceInfo,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let name = s.name.clone();
        let status = s.status.clone();
        let display = s.display_name.clone();
        let start_type = s.start_type.clone();
        let color = Self::status_color(&status, pal);

        table_row(pal)
            .id(SharedString::from(format!("svc-{}", name.clone())))
            // 状态（点色 + 彩色文本）
            .child(
                div()
                    .flex_none()
                    .w(px(80.0))
                    .child(status_pill(pal, status.clone(), color)),
            )
            // 服务名（Flex 自适应）
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(12.5))
                    .text_color(pal.text)
                    .child(name.clone()),
            )
            // 显示名（固定宽）
            .child(
                div()
                    .flex_none()
                    .w(px(260.0))
                    .text_size(px(12.0))
                    .text_color(pal.text_muted)
                    .child(display),
            )
            // 启动类型
            .child(
                div()
                    .flex_none()
                    .w(px(90.0))
                    .text_size(px(11.5))
                    .text_color(pal.text_muted)
                    .child(start_type),
            )
            // 操作按钮
            .child(
                div()
                    .flex_none()
                    .w(px(120.0))
                    .flex()
                    .gap_1()
                    .child(service_action_button(pal, "启动", name.clone(), ServiceOp::Start, cx))
                    .child(service_action_button(pal, "停止", name.clone(), ServiceOp::Stop, cx)),
            )
    }
}

/// 行内操作按钮（框架 button_sm：启动=次操作 / 停止=危险；id 组合规则保持原样）
fn service_action_button(
    pal: &Palette,
    label: &str,
    svc: String,
    op: ServiceOp,
    cx: &mut Context<ServicesView>,
) -> impl IntoElement {
    // 启动类操作用次操作样式，停止类操作用危险样式（分组与原实现一致）
    let kind = match op {
        ServiceOp::Start | ServiceOp::ToggleAuto | ServiceOp::ToggleManual => ButtonKind::Secondary,
        _ => ButtonKind::Danger,
    };
    let label_owned = label.to_string();
    button_sm(pal, kind)
        .id(SharedString::from(label_owned.clone() + &svc))
        .child(label_owned)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.op_service(&svc, op, cx);
        }))
}
