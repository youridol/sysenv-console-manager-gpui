// secm-app::pages::settings — 系统设置页
// Phase 2：开关组（HAGS/游戏模式…）+ 电源计划列表。状态从 secm-core::settings 读取，
// 操作后回读刷新；GPUI 交互经 Context::listener + notify。

use gpui::{div, px, rgb, SharedString, Window, Context, Render};
use gpui::prelude::*;
use secm_core::settings::{self, PowerPlan, SettingState};

use crate::theme::Theme;

/// 可切换设置项（枚举明确区分调用函数）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToggleKind {
    Hags,
    GameMode,
}

pub struct SettingsView {
    /// 开关状态（HAGS/游戏模式…，加载后填充）
    toggles: Vec<(ToggleKind, SettingState)>,
    /// 电源计划列表
    plans: Vec<PowerPlan>,
    /// 状态消息（操作反馈）
    status: SharedString,
}

impl SettingsView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut v = Self {
            toggles: vec![
                (ToggleKind::Hags, settings::get_hags_state()),
                (ToggleKind::GameMode, settings::get_game_mode_state()),
            ],
            plans: settings::get_power_plans().unwrap_or_default(),
            status: SharedString::from(""),
        };
        v.refresh(cx);
        v
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.toggles = vec![
            (ToggleKind::Hags, settings::get_hags_state()),
            (ToggleKind::GameMode, settings::get_game_mode_state()),
        ];
        self.plans = settings::get_power_plans().unwrap_or_default();
        cx.notify();
    }

    /// 切换开关（读当前状态 → 反相 → set → 回读）
    fn toggle(&mut self, kind: ToggleKind, cx: &mut Context<Self>) {
        let result = match kind {
            ToggleKind::Hags => {
                let cur = settings::get_hags_state();
                settings::set_hags_state(!cur.enabled)
            }
            ToggleKind::GameMode => {
                let cur = settings::get_game_mode_state();
                settings::set_game_mode_state(!cur.enabled)
            }
        };
        match result {
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

    fn kind_label(kind: ToggleKind) -> &'static str {
        match kind {
            ToggleKind::Hags => "GPU 硬件加速调度 (HAGS)",
            ToggleKind::GameMode => "游戏模式",
        }
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
                                    .child("Phase 2 · 部分功能"),
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
                                let label = Self::kind_label(k).to_string();
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
                            .px_5()
                            .py_3()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child("电源计划"),
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
    }
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
        div()
            .id(if kind == ToggleKind::Hags {
                "toggle-hags"
            } else {
                "toggle-gamemode"
            })
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
