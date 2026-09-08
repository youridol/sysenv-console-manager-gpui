// secm-app::pages::services — 服务管理页
// 枚举 Windows 服务 + 搜索 + 启停（停止二次确认）+ 启动类型（自动/手动/禁用）。
//
// 并发模型：服务枚举（数百服务，慢）后台线程执行；启停/启动类型为系统 API
// 调用，后台执行 + 完成后延迟后台刷新状态。写操作全局互斥（busy）；
// 主线程仅渲染。UI 反馈：全局 Toast + 状态横幅 + 日志流。
//
// 呈现层：统一接入 crate::ui::page 布局框架，色板取自 pi_clone::theme::Palette
// （明暗双主题，随壳 set_appearance 联动），禁止硬编码业务色。

use gpui::prelude::*;
use gpui::{div, px, Context, Entity, Render, SharedString, WeakEntity, Window};
use secm_core::settings::{self, ServiceInfo};

use crate::pi_clone::theme::{Appearance, Palette};
use crate::ui::page::{
    banner, button_sm, card, page_header, page_root, status_pill, table_empty, table_head,
    table_row, BannerKind, ButtonKind, ColWidth,
};
use crate::ui::text_input::{ChangeText, TextField};
use crate::ui::toast;

/// 启动类型三档（value 与后端 set_service_start_type 参数契约一致）
const START_TYPE_CHOICES: &[(&str, &str)] =
    &[("auto", "自动"), ("manual", "手动"), ("disabled", "禁用")];

pub struct ServicesView {
    services: Vec<ServiceInfo>,
    /// 搜索关键词（匹配 name/display_name）
    keyword: SharedString,
    status: SharedString,
    /// 列表加载中
    loading: bool,
    /// 当前写操作（互斥；定位到具体服务行做 loading 视觉）
    busy_svc: Option<(ServiceOp, String)>,
    /// 搜索输入框（ChangeText 订阅驱动搜索）
    search_input: Entity<TextField>,
    /// 待确认停止的服务（Some = 显示停止确认弹层）
    confirm_stop: Option<ServiceInfo>,
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
            busy_svc: None,
            search_input,
            confirm_stop: None,
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
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
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
            },
        )
        .detach();
    }

    /// 延迟 800ms 后台刷新（启停/启动类型异步生效）
    fn reload_later(&mut self, cx: &mut Context<Self>) {
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
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
            },
        )
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
                s.name.to_lowercase().contains(&kw) || s.display_name.to_lowercase().contains(&kw)
            })
            .collect()
    }

    /// 搜索关键词更新（由搜索输入框 ChangeText 订阅驱动）
    fn set_keyword(&mut self, kw: SharedString, cx: &mut Context<Self>) {
        self.keyword = kw;
        cx.notify();
    }

    /// 该服务行是否操作中（对应行按钮 loading 禁点视觉）
    fn row_busy(&self, name: &str) -> bool {
        match &self.busy_svc {
            Some((_, n)) => n == name,
            None => false,
        }
    }

    /// 任意写操作进行中（全局互斥：同时只允许一个系统写操作）
    fn any_busy(&self) -> bool {
        self.busy_svc.is_some()
    }

    /// 启停/启动类型（后台系统 API + 延迟刷新）
    fn op_service(&mut self, name: &str, op: ServiceOp, cx: &mut Context<Self>) {
        if self.any_busy() {
            return;
        }
        self.busy_svc = Some((op, name.to_string()));
        self.status = SharedString::from(format!("正在{} {}…", op.label(), name));
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        let name_c = name.to_string();
        let name_log = name_c.clone();
        let op_c = op;
        let op_label = op.label().to_string();
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
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
                        this.busy_svc = None;
                        // 全局泡泡提示：服务操作结果随屏可见
                        match &result {
                            Ok(msg) => {
                                toast::success(format!("服务 {}{}", name_log, op_label), cx);
                                this.status = SharedString::from(msg.clone());
                            }
                            Err(e) => {
                                toast::error(
                                    format!("{}服务 {} 失败：{}", op_label, name_log, e),
                                    cx,
                                );
                                this.status = SharedString::from(e.clone());
                            }
                        }
                        cx.notify();
                        // 延迟后台刷新（服务状态异步变化）
                        this.reload_later(cx);
                    })
                    .ok();
                }
            },
        )
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

/// 服务操作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// 启动类型显示文本 → 契约值（无法识别 → None：三档均不高亮，仅可选）
fn start_type_value(start_type: &str) -> Option<&'static str> {
    match start_type {
        "自动" => Some("auto"),
        "手动" => Some("manual"),
        "已禁用" | "禁用" => Some("disabled"),
        _ => None,
    }
}

impl Render for ServicesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.pal();
        let services = self.filtered();

        let content = page_root(&pal, "services-page-root", &self.page_scroll, &cx.entity())
            // 页头（标题+副标题）+ 右侧搜索框
            .child(
                page_header(
                    &pal,
                    "服务管理",
                    format!(
                        "{} 个服务 · 启动类型可设自动/手动/禁用（需管理员权限）",
                        self.services.len()
                    ),
                )
                .child(self.search_box()),
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
                                ("状态", ColWidth::Px(76.0)),
                                ("服务名", ColWidth::Flex),
                                ("显示名", ColWidth::Px(240.0)),
                                ("启动类型", ColWidth::Px(190.0)),
                                ("操作", ColWidth::Px(104.0)),
                            ],
                        ))
                        .when(!self.loading && services.is_empty(), |s| {
                            s.child(table_empty(
                                &pal,
                                "未枚举到服务（可能权限不足或服务控制台不可用）",
                            ))
                        })
                        .children(services.iter().map(|s| self.service_row(&pal, s, cx))),
                ),
            );

        // 停止确认弹层（模态；对齐上游 AlertDialog 语义）
        if let Some(svc) = self.confirm_stop.clone() {
            let modal = confirm_modal(
                &pal,
                "svc-stop-backdrop",
                format!("停止服务 {}", svc.name),
                format!(
                    "将停止系统服务「{}」。部分服务停止后可能导致相关功能不可用，需管理员权限，请确认后继续。",
                    svc.display_name
                ),
                "确认停止",
                ButtonKind::Danger,
                cx.listener(move |this, _, _, cx| {
                    let name = svc.name.clone();
                    this.confirm_stop = None;
                    this.op_service(&name, ServiceOp::Stop, cx);
                }),
                cx.listener(|this, _, _, cx| {
                    this.confirm_stop = None;
                    cx.notify();
                }),
            );
            div()
                .relative()
                .size_full()
                .child(content)
                .child(gpui::deferred(modal))
                .into_any_element()
        } else {
            div()
                .relative()
                .size_full()
                .child(content)
                .into_any_element()
        }
    }
}

use crate::ui::page::confirm_modal;

impl ServicesView {
    /// 页头右侧搜索框（视觉外壳由 TextField 统一提供，页面不再包一层边框底色）
    fn search_box(&self) -> impl IntoElement {
        div()
            .id("svc-search")
            .flex()
            .items_center()
            .w(px(220.0))
            .max_w(px(320.0))
            .min_w(px(140.0))
            .flex_shrink_0()
            // 真实输入框（统一视觉外壳；ChangeText 订阅驱动搜索）
            .child(self.search_input.clone())
    }

    /// 单行服务数据（单元格列宽与表头完全一致：76/Flex/240/190/104）
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
        let running = status == "Running";
        let row_busy = self.row_busy(&name);
        let any_busy = self.any_busy();
        // 停止/启动按行互斥：本行操作中禁点；其他行在全局互斥期间同样禁点
        let action_disabled = row_busy || any_busy;
        let current_type = start_type_value(&start_type);
        // 启动类型操作中：本行三档禁点（其余行按全局互斥禁点）
        let type_busy = self
            .busy_svc
            .as_ref()
            .map(|(op, n)| {
                n == name.as_str()
                    && matches!(
                        op,
                        ServiceOp::ToggleAuto | ServiceOp::ToggleManual | ServiceOp::ToggleDisable
                    )
            })
            .unwrap_or(false);

        table_row(pal)
            .id(SharedString::from(format!("svc-{}", name.clone())))
            // 状态（点色 + 彩色文本）
            .child(
                div()
                    .flex_none()
                    .w(px(76.0))
                    .child(status_pill(pal, status.clone(), color)),
            )
            // 服务名（Flex 自适应；单行省略防溢出）
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .whitespace_nowrap()
                    .truncate()
                    .text_size(px(12.5))
                    .text_color(pal.text)
                    .child(name.clone()),
            )
            // 显示名（固定宽；单行省略防溢出挤压后续列）
            .child(
                div()
                    .flex_none()
                    .w(px(240.0))
                    .whitespace_nowrap()
                    .truncate()
                    .text_size(px(12.0))
                    .text_color(pal.text_muted)
                    .child(display),
            )
            // 启动类型（三档按钮组：当前档 accent 高亮；点击设为新档）
            .child(div().flex_none().w(px(190.0)).flex().gap_1().children(
                START_TYPE_CHOICES.iter().map(|(value, label)| {
                    let is_cur = current_type == Some(*value);
                    let op = match *value {
                        "auto" => ServiceOp::ToggleAuto,
                        "manual" => ServiceOp::ToggleManual,
                        _ => ServiceOp::ToggleDisable,
                    };
                    let svc = name.clone();
                    let disabled = type_busy || any_busy;
                    div()
                        .id(SharedString::from(format!("svc-type-{}-{}", name, value)))
                        .px_2()
                        .py(px(2.0))
                        .rounded_md()
                        .text_size(px(10.5))
                        .when(is_cur, |s| s.bg(pal.accent).text_color(pal.accent_contrast))
                        .when(!is_cur, |s| {
                            s.bg(pal.bg_hover)
                                .hover(|s| s.bg(pal.bg_selected))
                                .text_color(pal.text)
                        })
                        .when(disabled, |s| s.opacity(0.55).cursor_default())
                        .when(!disabled, |s| s.cursor_pointer())
                        .when(!disabled && !is_cur, |s| {
                            s.on_click(cx.listener(move |this, _, _, cx| {
                                this.op_service(&svc, op, cx);
                            }))
                        })
                        .child(label.to_string())
                }),
            ))
            // 操作按钮：运行中 → 停止（危险；二次确认）；非运行 → 启动（次操作）
            .child(
                div()
                    .flex_none()
                    .w(px(104.0))
                    .flex()
                    .gap_1()
                    .child(if running {
                        let svc = name.clone();
                        button_sm(pal, ButtonKind::Danger)
                            .id(SharedString::from(format!("svc-stop-{}", name)))
                            .when(action_disabled, |s| s.opacity(0.55).cursor_default())
                            .when(!action_disabled, |s| {
                                s.on_click(cx.listener(move |this, _, _, cx| {
                                    // 打开停止确认弹层（破坏性操作二次确认）
                                    this.confirm_stop =
                                        this.services.iter().find(|s| s.name == svc).cloned();
                                    cx.notify();
                                }))
                            })
                            .child("停止")
                    } else {
                        let svc = name.clone();
                        button_sm(pal, ButtonKind::Secondary)
                            .id(SharedString::from(format!("svc-start-{}", name)))
                            .when(action_disabled, |s| s.opacity(0.55).cursor_default())
                            .when(!action_disabled, |s| {
                                s.on_click(cx.listener(move |this, _, _, cx| {
                                    this.op_service(&svc, ServiceOp::Start, cx);
                                }))
                            })
                            .child("启动")
                    }),
            )
    }
}
