// secm-app::pages::settings — 系统设置页
// 开关组（HAGS/游戏模式/窗口化优化/VRR/鼠标精准度）+ 电源计划列表 + 异类调度策略 + 卓越性能导入。
//
// 并发模型：初始状态（开关/电源计划/异类策略）后台线程一次加载；所有写操作
// （切换/激活计划/异类策略/导入卓越）在后台线程执行，完成后后台重读回填；
// 主线程仅渲染当前状态。写操作互斥防并发。

use gpui::{div, px, rgb, SharedString, Window, Context, Render, WeakEntity};
use gpui::prelude::*;
use secm_core::settings::{self, HeteroPolicies, PowerPlan, SettingState};

use crate::theme::Theme;

/// 可切换设置项（枚举明确区分调用函数）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToggleKind {
    Hags,
    GameMode,
    GameOptimization,
    Vrr,
    MousePrecision,
}

impl ToggleKind {
    fn label(self) -> &'static str {
        match self {
            Self::Hags => "GPU 硬件加速调度 (HAGS)",
            Self::GameMode => "游戏模式",
            Self::GameOptimization => "窗口化游戏优化",
            Self::Vrr => "可变刷新率 (VRR)",
            Self::MousePrecision => "鼠标精准度（增强指针精确度）",
        }
    }

    /// 读取当前状态（后台线程调用）
    fn get(self) -> SettingState {
        match self {
            Self::Hags => settings::get_hags_state(),
            Self::GameMode => settings::get_game_mode_state(),
            Self::GameOptimization => settings::get_game_optimization_state(),
            Self::Vrr => settings::get_vrr_state(),
            Self::MousePrecision => settings::get_mouse_precision_state(),
        }
    }

    fn set(self, enabled: bool) -> Result<SettingState, String> {
        match self {
            Self::Hags => settings::set_hags_state(enabled),
            Self::GameMode => settings::set_game_mode_state(enabled),
            Self::GameOptimization => settings::set_game_optimization(enabled),
            Self::Vrr => settings::set_vrr_state(enabled),
            Self::MousePrecision => settings::set_mouse_precision(enabled),
        }
    }
}

/// 异类调度策略取值（0-5 → 中文标签）
const HETERO_CHOICES: &[(u32, &str)] = &[
    (0, "所有处理器"),
    (1, "高性能处理器"),
    (2, "首选高性能处理器"),
    (3, "高效处理器"),
    (4, "首选高效处理器"),
    (5, "自动"),
];

/// 全部设置状态（后台一次加载）
struct AllSettings {
    toggles: Vec<(ToggleKind, SettingState)>,
    plans: Vec<PowerPlan>,
    hetero: Option<HeteroPolicies>,
}

pub struct SettingsView {
    /// 开关状态列表
    toggles: Vec<(ToggleKind, SettingState)>,
    /// 电源计划列表
    plans: Vec<PowerPlan>,
    /// 异类调度策略（无/不支持时 None）
    hetero: Option<HeteroPolicies>,
    /// 初始状态是否加载中
    loading: bool,
    /// 写操作进行中（互斥，防并发写注册表）
    op_busy: bool,
    /// 状态消息（操作反馈）
    status: SharedString,
    /// 卓越性能导入反馈
    ultimate_msg: SharedString,
}

impl SettingsView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        log::info!("系统设置 · 页面已打开");
        let mut v = Self {
            toggles: Vec::new(),
            plans: Vec::new(),
            hetero: None,
            loading: false,
            op_busy: false,
            status: SharedString::from("正在读取设置状态…"),
            ultimate_msg: SharedString::from(""),
        };
        v.start_load(cx);
        v
    }

    /// 后台加载全部状态（开关/电源计划/异类策略）
    fn start_load(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let data = exec
                .spawn(async move {
                    AllSettings {
                        toggles: [
                            ToggleKind::Hags,
                            ToggleKind::GameMode,
                            ToggleKind::GameOptimization,
                            ToggleKind::Vrr,
                            ToggleKind::MousePrecision,
                        ]
                        .into_iter()
                        .map(|k| (k, k.get()))
                        .collect(),
                        plans: settings::get_power_plans().unwrap_or_default(),
                        hetero: settings::get_hetero_policies().ok(),
                    }
                })
                .await;
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.loading = false;
                    this.toggles = data.toggles;
                    this.plans = data.plans;
                    this.hetero = data.hetero;
                    this.status = SharedString::from("");
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 切换开关（后台写 + 后台重读该开关真值）
    fn toggle(&mut self, kind: ToggleKind, cx: &mut Context<Self>) {
        if self.op_busy {
            return;
        }
        self.op_busy = true;
        // 乐观 UI：立即反相显示当前开关
        if let Some((_, st)) = self.toggles.iter_mut().find(|(k, _)| *k == kind) {
            st.enabled = !st.enabled;
        }
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        let kind_c = kind;
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            // 后台：读当前真值 → 反相写入
            let result = exec
                .spawn(async move {
                    let cur = kind_c.get();
                    kind_c.set(!cur.enabled)
                })
                .await;
            // UI 侧日志：记录用户触发的开关切换（成功用 info，失败用 warn）
            match &result {
                Ok(s) => log::info!("系统设置 · 已切换「{}」→ {}", kind_c.label(), s.message),
                Err(e) => log::warn!("系统设置 · 切换「{}」失败: {}", kind_c.label(), e),
            }
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.op_busy = false;
                    match &result {
                        Ok(s) => this.status = SharedString::from(s.message.clone()),
                        Err(e) => this.status = SharedString::from(format!("操作失败: {}", e)),
                    }
                    cx.notify();
                })
                .ok();
                // 后台重读该开关，回填真实状态（写可能被系统拒绝）
                let exec = cx.background_executor().clone();
                let k2 = kind_c;
                let new_state = exec.spawn(async move { k2.get() }).await;
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        if let Some((_, st)) = this.toggles.iter_mut().find(|(tk, _)| *tk == k2) {
                            *st = new_state;
                        }
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    /// 切换电源计划（后台执行）
    fn activate_plan(&mut self, guid: &str, cx: &mut Context<Self>) {
        if self.op_busy {
            return;
        }
        self.op_busy = true;
        self.status = SharedString::from("正在切换电源计划…");
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        let guid_c = guid.to_string();
        let name_c = self
            .plans
            .iter()
            .find(|p| p.guid == guid)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| format!("计划 {}", &guid[..8.min(guid.len())]));
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let result = exec
                .spawn(async move { settings::set_power_plan(&guid_c) })
                .await;
            // UI 侧日志：电源计划激活结果
            match &result {
                Ok(()) => log::info!("系统设置 · 已激活电源计划 {}", name_c),
                Err(e) => log::warn!("系统设置 · 激活电源计划 {} 失败: {}", name_c, e),
            }
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.op_busy = false;
                    this.status = match result {
                        Ok(()) => SharedString::from("电源计划已切换"),
                        Err(e) => SharedString::from(format!("切换失败: {}", e)),
                    };
                    cx.notify();
                })
                .ok();
                view.update(cx, |this, cx| {
                    // 后台重读计划列表与当前激活
                    this.start_reload_plans(cx);
                })
                .ok();
            }
        })
        .detach();
    }

    /// 后台仅重读电源计划列表
    fn start_reload_plans(&mut self, cx: &mut Context<Self>) {
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let plans = exec
                .spawn(async move { settings::get_power_plans().unwrap_or_default() })
                .await;
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.plans = plans;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 设置异类调度策略（后台 AC/DC 写 + 重读）
    fn set_hetero(&mut self, kind: &str, value: u32, cx: &mut Context<Self>) {
        if self.op_busy {
            return;
        }
        self.op_busy = true;
        self.status = SharedString::from("正在设置调度策略…");
        cx.notify();

        let kind_label = if kind == "short" {
            "短运行线程调度策略"
        } else {
            "线程调度策略"
        };
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        let kind_c = kind.to_string();
        let kind_label_c = kind_label.to_string();
        let value_c = value;
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let result = exec
                .spawn(async move { settings::set_hetero_policy(&kind_c, value_c) })
                .await;
            // UI 侧日志：异类调度策略设置结果
            let value_label = Self::hetero_label(value_c);
            match &result {
                Ok(()) => log::info!(
                    "系统设置 · {}已设为「{}」",
                    kind_label_c,
                    value_label
                ),
                Err(e) => log::warn!("系统设置 · 设置{}失败: {}", kind_label_c, e),
            }
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.op_busy = false;
                    this.status = match result {
                        Ok(()) => SharedString::from(format!(
                            "{}已设为「{}」",
                            kind_label_c,
                            Self::hetero_label(value_c)
                        )),
                        Err(e) => SharedString::from(format!("设置失败: {}", e)),
                    };
                    cx.notify();
                })
                .ok();
                view.update(cx, |this, cx| {
                    this.start_load(cx);
                })
                .ok();
            }
        })
        .detach();
    }

    /// 导入并激活卓越性能电源计划（后台执行）
    fn import_ultimate(&mut self, cx: &mut Context<Self>) {
        if self.op_busy {
            return;
        }
        self.op_busy = true;
        self.status = SharedString::from("正在导入卓越性能计划…");
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let result = exec
                .spawn(async move { settings::enable_ultimate_performance() })
                .await;
            // UI 侧日志：导入卓越性能计划结果
            match &result {
                Ok(msg) => log::info!("系统设置 · 导入卓越性能计划成功: {}", msg),
                Err(e) => log::warn!("系统设置 · 导入卓越性能计划失败: {}", e),
            }
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.op_busy = false;
                    match result {
                        Ok(msg) => {
                            this.ultimate_msg = SharedString::from(msg);
                            this.status = SharedString::from("卓越性能电源计划已导入并激活");
                        }
                        Err(e) => {
                            this.status = SharedString::from(format!("导入失败: {}", e));
                        }
                    }
                    cx.notify();
                })
                .ok();
                view.update(cx, |this, cx| {
                    this.start_reload_plans(cx);
                })
                .ok();
            }
        })
        .detach();
    }

    fn hetero_label(value: u32) -> &'static str {
        HETERO_CHOICES
            .iter()
            .find(|(v, _)| *v == value)
            .map(|(_, l)| *l)
            .unwrap_or("自动")
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::dark();
        let status = self.status.clone();

        div()
            .flex_col()
            .size_full()
            .p_6()
            .gap_4()
            // 页头
            .child(
                div()
                    .flex()
                    .items_baseline()
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
                                    .child("系统设置"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme.text_muted)
                                    .child("系统优化开关 · 电源计划 · 异类调度策略"),
                            ),
                    ),
            )
            // 开关组
            .child(
                div()
                    .flex_col()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.panel)
                    .children(
                        self.toggles
                            .iter()
                            .enumerate()
                            .map(|(i, (kind, state))| {
                                let k = *kind;
                                let enabled = state.enabled;
                                let msg = state.message.clone();
                                let label = k.label().to_string();
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .px_5()
                                    .py_3p5()
                                    .when(i + 1 < self.toggles.len(), |s| {
                                        s.border_b_1().border_color(theme.border)
                                    })
                                    .child(
                                        div()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_color(theme.text)
                                                    .text_size(px(14.0))
                                                    .child(label),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(11.5))
                                                    .text_color(theme.text_muted)
                                                    .child(msg),
                                            ),
                                    )
                                    .child(self.toggle_switch(&theme, k, enabled, cx))
                            }),
                    ),
            )
            // 状态消息
            .when(!status.is_empty(), |s| {
                s.child(
                    div()
                        .px_4()
                        .py_2()
                        .rounded_md()
                        .bg(theme.panel_hover)
                        .text_size(px(12.0))
                        .text_color(theme.info)
                        .child(status),
                )
            })
            // 电源计划
            .child(
                div()
                    .flex_col()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.panel)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_5()
                            .py_3()
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child("电源计划"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id("import-ultimate")
                                            .px_3()
                                            .py_1()
                                            .rounded_md()
                                            .cursor_pointer()
                                            .bg(theme.panel_hover)
                                            .hover(|s| s.bg(theme.border))
                                            .text_color(theme.text)
                                            .text_size(px(11.5))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.import_ultimate(cx);
                                            }))
                                            .child("导入卓越性能计划"),
                                    )
                                    .when(!self.ultimate_msg.is_empty(), |s| {
                                        let m = self.ultimate_msg.clone();
                                        s.child(
                                            div()
                                                .text_size(px(10.5))
                                                .text_color(theme.text_muted)
                                                .child(m),
                                        )
                                    }),
                            ),
                    )
                    .children(self.plans.iter().map(|p| {
                        let guid = p.guid.clone();
                        let guid8 = SharedString::from(guid.chars().take(8).collect::<String>());
                        let name = p.name.clone();
                        let active = p.is_active;
                        div()
                            .id(guid8.clone())
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_5()
                            .py_2p5()
                            .cursor_pointer()
                            .when(!active, |s| s.hover(|s| s.bg(theme.panel_hover)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !active {
                                    this.activate_plan(&guid, cx);
                                }
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .when(active, |s| {
                                        s.child(
                                            div()
                                                .size(px(7.0))
                                                .rounded_full()
                                                .bg(theme.success),
                                        )
                                    })
                                    .child(
                                        div()
                                            .text_color(if active {
                                                theme.text
                                            } else {
                                                theme.text_muted
                                            })
                                            .text_size(px(13.0))
                                            .child(name),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(if active {
                                        theme.success
                                    } else {
                                        theme.text_muted
                                    })
                                    .child(if active {
                                        "当前 · 点击其余计划可切换".to_string()
                                    } else {
                                        "点击激活".to_string()
                                    }),
                            )
                    })),
            )
            // 异类调度策略（仅混合架构 CPU 支持时显示）
            .when_some(self.hetero.as_ref(), |s, h| {
                let supported = h.supported;
                let thread_ac = h.thread_ac;
                let short_ac = h.short_ac;
                s.child(
                    div()
                        .flex_col()
                        .rounded_md()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.panel)
                        .child(
                            div()
                                .px_5()
                                .py_3()
                                .text_size(px(14.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child("异类线程调度策略"),
                        )
                        .when(!supported, |s| {
                            s.child(
                                div()
                                    .px_5()
                                    .py_3()
                                    .text_size(px(12.0))
                                    .text_color(theme.text_muted)
                                    .child("当前 CPU 不支持（非混合架构）"),
                            )
                        })
                        .when(supported, |s| {
                            s.child(hetero_section_row(
                                &theme,
                                "线程调度策略",
                                thread_ac,
                                "thread",
                                cx,
                            ))
                            .child(hetero_section_row(
                                &theme,
                                "短运行线程调度策略",
                                short_ac,
                                "short",
                                cx,
                            ))
                        }),
                )
            })
    }
}

/// 异类策略选择行（6 档按钮组，当前值高亮）
fn hetero_section_row(
    theme: &Theme,
    title: &str,
    current: Option<u32>,
    kind: &'static str,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let title = title.to_string();
    div()
        .flex_col()
        .gap_2()
        .px_5()
        .py_3()
        .border_t_1()
        .border_color(theme.border)
        .child(
            div()
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(title),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_1p5()
                .children(HETERO_CHOICES.iter().map(|(v, label)| {
                    let value = *v;
                    let is_cur = current == Some(value);
                    let label_owned = label.to_string();
                    let kind_c = kind;
                    div()
                        .id(SharedString::from(format!("hetero-{}-{}", kind_c, value)))
                        .px_2p5()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .when(is_cur, |s| {
                            s.bg(theme.brand).text_color(rgb(0xffffff))
                        })
                        .when(!is_cur, |s| {
                            s.bg(theme.panel_hover)
                                .hover(|s| s.bg(theme.border))
                                .text_color(theme.text)
                        })
                        .text_size(px(11.5))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !is_cur {
                                this.set_hetero(kind_c, value, cx);
                            }
                        }))
                        .child(label_owned)
                })),
        )
}

impl SettingsView {
    /// 开关控件（圆钮式：开=品牌色，关=灰；点击切换）
    fn toggle_switch(
        &self,
        theme: &Theme,
        kind: ToggleKind,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = match kind {
            ToggleKind::Hags => "toggle-hags",
            ToggleKind::GameMode => "toggle-gamemode",
            ToggleKind::GameOptimization => "toggle-gameopt",
            ToggleKind::Vrr => "toggle-vrr",
            ToggleKind::MousePrecision => "toggle-mouseprec",
        };
        div()
            .id(id)
            .w(px(44.0))
            .h(px(24.0))
            .rounded_full()
            .cursor_pointer()
            .when(enabled, |s| s.bg(theme.brand))
            .when(!enabled, |s| s.bg(theme.panel_hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle(kind, cx);
            }))
            .child(
                div()
                    .absolute()
                    .top(px(2.0))
                    .size(px(20.0))
                    .rounded_full()
                    .bg(rgb(0xffffff))
                    .when(enabled, |s| s.right(px(2.0)))
                    .when(!enabled, |s| s.left(px(2.0))),
            )
    }
}
