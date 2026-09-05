// secm-app::pages::settings — 系统设置页
// 开关组（HAGS/游戏模式/窗口化优化/VRR/鼠标精准度）+ 电源计划列表 + 异类调度策略 + 卓越性能导入。
// 状态从 secm-core::settings 读取，操作后回读刷新。

use gpui::{div, px, rgb, SharedString, Window, Context, Render};
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

    /// 开关开启方向（鼠标精准度开启 = 启用增强精确度；其余为"优化开启"）
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

pub struct SettingsView {
    /// 开关状态列表
    toggles: Vec<(ToggleKind, SettingState)>,
    /// 电源计划列表
    plans: Vec<PowerPlan>,
    /// 异类调度策略（无/不支持时 None）
    hetero: Option<HeteroPolicies>,
    /// 状态消息（操作反馈）
    status: SharedString,
    /// 卓越性能导入反馈
    ultimate_msg: SharedString,
}

impl SettingsView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut v = Self {
            toggles: Self::load_toggles(),
            plans: settings::get_power_plans().unwrap_or_default(),
            hetero: settings::get_hetero_policies().ok(),
            status: SharedString::from(""),
            ultimate_msg: SharedString::from(""),
        };
        v.refresh(cx);
        v
    }

    fn load_toggles() -> Vec<(ToggleKind, SettingState)> {
        [
            ToggleKind::Hags,
            ToggleKind::GameMode,
            ToggleKind::GameOptimization,
            ToggleKind::Vrr,
            ToggleKind::MousePrecision,
        ]
        .into_iter()
        .map(|k| (k, k.get()))
        .collect()
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.toggles = Self::load_toggles();
        self.plans = settings::get_power_plans().unwrap_or_default();
        self.hetero = settings::get_hetero_policies().ok();
        cx.notify();
    }

    /// 切换开关（读当前状态 → 反相 → set → 回读）
    fn toggle(&mut self, kind: ToggleKind, cx: &mut Context<Self>) {
        let cur = kind.get();
        match kind.set(!cur.enabled) {
            Ok(s) => self.status = SharedString::from(s.message.clone()),
            Err(e) => self.status = SharedString::from(format!("操作失败: {}", e)),
        }
        self.refresh(cx);
    }

    /// 切换电源计划
    fn activate_plan(&mut self, guid: &str, cx: &mut Context<Self>) {
        match settings::set_power_plan(guid) {
            Ok(()) => self.status = SharedString::from("电源计划已切换"),
            Err(e) => self.status = SharedString::from(format!("切换失败: {}", e)),
        }
        self.refresh(cx);
    }

    /// 设置异类调度策略（kind=thread/short；AC/DC 同步写）
    fn set_hetero(&mut self, kind: &str, value: u32, cx: &mut Context<Self>) {
        let kind_label = if kind == "short" {
            "短运行线程调度策略"
        } else {
            "线程调度策略"
        };
        match settings::set_hetero_policy(kind, value) {
            Ok(()) => self.status = SharedString::from(format!("{}已设为「{}」", kind_label, Self::hetero_label(value))),
            Err(e) => self.status = SharedString::from(format!("设置失败: {}", e)),
        }
        self.refresh(cx);
    }

    /// 导入并激活卓越性能电源计划
    fn import_ultimate(&mut self, cx: &mut Context<Self>) {
        match settings::enable_ultimate_performance() {
            Ok(msg) => {
                self.ultimate_msg = SharedString::from(msg);
                self.status = SharedString::from("卓越性能电源计划已导入并激活");
            }
            Err(e) => self.status = SharedString::from(format!("导入失败: {}", e)),
        }
        self.refresh(cx);
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
                                                .bg(rgb(0x4ade80)),
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
                                        rgb(0x4ade80)
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
