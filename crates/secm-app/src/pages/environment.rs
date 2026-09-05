// secm-app::pages::environment — 环境检测页（Phase 5）
// 系统信息 8 字段 + 游戏环境预设对比 + DirectX 诊断 + VC++ 运行库 + AI 工具检测。
// 检测含外部命令/网络（npm/registry），进入页面后台执行一次，可手动重新检测。

use gpui::prelude::*;
use gpui::{div, px, rgb, SharedString, Window, Context, Render, WeakEntity};
use secm_core::environment::{
    self, AiToolsInfo, CheckStatus, DirectXInfo, DxCheck, VcRuntimeInfo,
};
use secm_core::game_env::{self, GamePreset, GameSetting};
use secm_core::sysinfo::{self, SystemInfo};

use crate::theme::Theme;

pub struct EnvironmentView {
    /// 系统信息（同步快，进入即载）
    system: SystemInfo,
    /// DirectX / VC++（注册表为主，快）
    dx: DirectXInfo,
    vc: VcRuntimeInfo,
    /// 游戏预设对比（同步读当前设置）
    presets: Vec<GamePreset>,
    /// AI 工具（npm/where 慢 → 后台检测）
    ai: Option<AiToolsInfo>,
    /// 检测中标记
    loading_ai: bool,
    /// 页面错误/状态
    status: String,
}

impl EnvironmentView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // 同步项：系统信息 / DirectX / VC++ / 游戏预设（注册表与命令，秒级）
        let system = sysinfo::get_system_info();
        let dx = environment::check_directx();
        let vc = environment::check_vc_runtimes();
        let presets = game_env::get_game_presets();

        let mut v = Self {
            system,
            dx,
            vc,
            presets,
            ai: None,
            loading_ai: false,
            status: String::new(),
        };
        v.run_ai_check(cx);
        v
    }

    /// 后台执行 AI 工具检测（npm 查询，耗时数秒）
    fn run_ai_check(&mut self, cx: &mut Context<Self>) {
        if self.loading_ai {
            return;
        }
        self.loading_ai = true;
        self.status = "AI 工具检测中…（npm 查询可能需要几秒）".to_string();
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let result = exec
                .spawn(async move { environment::check_ai_tools() })
                .await;
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.loading_ai = false;
                    this.ai = Some(result);
                    this.status = String::new();
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 全页重新检测
    fn rescan(&mut self, cx: &mut Context<Self>) {
        self.system = sysinfo::get_system_info();
        self.dx = environment::check_directx();
        self.vc = environment::check_vc_runtimes();
        self.presets = game_env::get_game_presets();
        self.ai = None;
        self.run_ai_check(cx);
        cx.notify();
    }

    /// 一键套用游戏预设（联动系统设置开关；key 为可联动项）
    fn apply_preset(&mut self, preset: &GamePreset, cx: &mut Context<Self>) {
        use secm_core::settings;
        let mut applied = 0u32;
        for s in &preset.settings {
            if s.key.is_empty() {
                continue; // 纯检测项不可切换
            }
            // 仅处理"推荐开启"的联动项（ok=false 时切换）；当前已达标跳过
            if s.ok {
                continue;
            }
            let recommended_on = s.recommended.contains("开启")
                || s.recommended.contains("高性能")
                || s.recommended.contains("卓越性能");
            match s.key.as_str() {
                "hags" => {
                    let _ = settings::set_hags_state(recommended_on);
                    applied += 1;
                }
                "game_mode" => {
                    let _ = settings::set_game_mode_state(recommended_on);
                    applied += 1;
                }
                "vrr" => {
                    let _ = settings::set_vrr_state(recommended_on);
                    applied += 1;
                }
                "mouse_precision" => {
                    // 推荐"关闭"即禁用增强指针精确度
                    let _ = settings::set_mouse_precision(!recommended_on);
                    applied += 1;
                }
                "power_plan" => {
                    // 按推荐名激活对应电源计划
                    if let Ok(plans) = settings::get_power_plans() {
                        let target = plans
                            .iter()
                            .find(|p| {
                                s.recommended.contains("高性能")
                                    && p.name.contains("高性能")
                            })
                            .or_else(|| {
                                plans.iter().find(|p| {
                                    s.recommended.contains("卓越")
                                        && p.name.contains("卓越")
                                })
                            });
                        if let Some(plan) = target {
                            let _ = settings::set_power_plan(&plan.guid);
                            applied += 1;
                        }
                    }
                }
                _ => {}
            }
        }
        if applied > 0 {
            self.status = format!("已应用「{}」预设（{} 项联动）", preset.name, applied);
        } else {
            self.status = format!("「{}」全部达标，无需调整", preset.name);
        }
        // 重读预设（当前值变化）
        self.presets = game_env::get_game_presets();
        cx.notify();
    }

    fn check_color(status: &CheckStatus, theme: &Theme) -> gpui::Rgba {
        match status {
            CheckStatus::Pass => theme.success,
            CheckStatus::Warn => theme.warn,
            CheckStatus::Fail => theme.danger,
            CheckStatus::Info => theme.text_muted,
        }
    }

    fn check_icon(status: &CheckStatus) -> &'static str {
        match status {
            CheckStatus::Pass => "✓",
            CheckStatus::Warn => "⚠",
            CheckStatus::Fail => "✗",
            CheckStatus::Info => "ℹ",
        }
    }

    /// 检测条目行（DX/VC++ 通用：状态图标 + 名 + 详情）
    fn check_row(&self, theme: &Theme, c: &DxCheck) -> impl IntoElement {
        let name = c.name.clone();
        let detail = c.detail.clone();
        let color = Self::check_color(&c.status, theme);
        let icon = Self::check_icon(&c.status).to_string();
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_5()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_size(px(12.5))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(color)
                    .child(icon),
            )
            .child(
                div()
                    .w(px(150.0))
                    .flex_none()
                    .text_size(px(12.5))
                    .text_color(theme.text)
                    .child(name),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child(detail),
            )
    }
}

impl Render for EnvironmentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::dark();
        let status = self.status.clone();
        let loading_ai = self.loading_ai;
        let ai = self.ai.clone();
        let sys = self.system.clone();
        let dx = self.dx.clone();
        let vc = self.vc.clone();
        let presets: Vec<GamePreset> = self.presets.clone();

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
                            .text_size(px(24.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme.text)
                            .child("环境检测"),
                    )
                    .child(
                        div()
                            .id("env-rescan")
                            .px_4()
                            .py_1p5()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(theme.panel_hover)
                            .hover(|s| s.bg(theme.border))
                            .text_color(theme.text)
                            .text_size(px(12.5))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.rescan(cx);
                            }))
                            .child("重新检测"),
                    ),
            )
            // 状态
            .when(!status.is_empty(), |s| {
                let msg = status.clone();
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
            // 系统信息卡
            .child(self.system_card(&theme, &sys))
            // 游戏环境预设
            .child(self.presets_section(&theme, &presets, cx))
            // DirectX + VC++ 双列
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_4()
                    .child(self.dx_card(&theme, &dx))
                    .child(self.vc_card(&theme, &vc)),
            )
            // AI 工具卡
            .child(self.ai_card(&theme, &ai, loading_ai, cx))
    }
}

impl EnvironmentView {
    /// 系统信息 8 字段卡
    fn system_card(&self, theme: &Theme, s: &SystemInfo) -> impl IntoElement {
        let rows: Vec<(&str, String)> = vec![
            ("系统版本", s.edition.clone()),
            ("内部版本", format!("{} · UBR {}", s.build_number, s.ubr)),
            ("系统架构", s.arch.clone()),
            ("安装日期", s.install_date.clone()),
            ("激活状态", format!("{}（{}）", s.activation.label, s.activation.status_raw)),
            ("最新补丁", format!("{} · {}", s.latest_patch.kb, s.latest_patch.title_cn)),
            ("启动模式", s.boot_mode.clone()),
        ];
        crate::ui::table_container(theme)
            .child(
                div()
                    .px_5()
                    .py_3()
                    .text_size(px(14.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child("系统信息"),
            )
            .children(rows.into_iter().map(|(k, v)| {
                let k = k.to_string();
                div()
                    .flex()
                    .items_center()
                    .px_5()
                    .py_1p5()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .w(px(100.0))
                            .flex_none()
                            .text_size(px(12.0))
                            .text_color(theme.text_muted)
                            .child(k),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(theme.text)
                            .child(v),
                    )
            }))
    }

    /// 游戏环境预设对比（含一键套用）
    fn presets_section(
        &self,
        theme: &Theme,
        presets: &[GamePreset],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        crate::ui::table_container(theme)
            .child(
                div()
                    .px_5()
                    .py_3()
                    .text_size(px(14.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child("游戏环境预设（推荐设置对比当前状态）"),
            )
            .children(presets.iter().map(|p| {
                let name = p.name.clone();
                let engine = p.engine.clone();
                let settings: Vec<GameSetting> = p.settings.clone();
                let ok_all = settings.iter().all(|s| s.ok || s.key.is_empty());
                let preset_for_click = p.clone();
                let applied_id = SharedString::from(format!("preset-{}", p.id));
                div()
                    .flex_col()
                    .px_5()
                    .py_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_size(px(13.5))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(theme.text)
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(theme.text_muted)
                                            .child(engine),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(if ok_all { theme.success } else { theme.warn })
                                            .child(if ok_all { "全部达标" } else { "有未达标项" }),
                                    ),
                            )
                            .child(
                                div()
                                    .id(applied_id)
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .when(!ok_all, |s| {
                                        s.bg(theme.brand)
                                            .hover(|s| s.bg(rgb(0x3d66e6)))
                                            .text_color(rgb(0xffffff))
                                    })
                                    .when(ok_all, |s| {
                                        s.bg(theme.panel_hover).text_color(theme.text_muted)
                                    })
                                    .text_size(px(11.5))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !ok_all {
                                            this.apply_preset(&preset_for_click, cx);
                                        }
                                    }))
                                    .child(if ok_all { "已达标" } else { "一键套用" }),
                            ),
                    )
                    .children(settings.iter().map(|s| {
                        let label = s.label.clone();
                        let rec = s.recommended.clone();
                        let cur = s.current.clone();
                        let desc = s.description.clone();
                        let ok = s.ok;
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_1()
                            .py_1()
                            .child(
                                div()
                                    .size(px(6.0))
                                    .rounded_full()
                                    .bg(if ok { theme.success } else { theme.warn }),
                            )
                            .child(
                                div()
                                    .w(px(150.0))
                                    .flex_none()
                                    .text_size(px(12.0))
                                    .text_color(theme.text)
                                    .child(label),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child(desc),
                            )
                            .child(
                                div()
                                    .w(px(80.0))
                                    .flex_none()
                                    .text_size(px(11.5))
                                    .text_color(theme.text_muted)
                                    .child(format!("推荐 {}", rec)),
                            )
                            .child(
                                div()
                                    .w(px(80.0))
                                    .flex_none()
                                    .text_size(px(11.5))
                                    .text_color(if ok { theme.success } else { theme.danger })
                                    .child(format!("当前 {}", cur)),
                            )
                    }))
            }))
    }

    /// DirectX 诊断卡
    fn dx_card(&self, theme: &Theme, dx: &DirectXInfo) -> impl IntoElement {
        let checks: Vec<DxCheck> = dx.checks.clone();
        let version = dx.version.clone();
        crate::ui::table_container(theme)
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
                            .child("DirectX 诊断"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.brand)
                            .child(SharedString::from(format!("DirectX {}", version))),
                    ),
            )
            .children(checks.iter().map(|c| self.check_row(theme, c)))
    }

    /// VC++ 运行库卡
    fn vc_card(&self, theme: &Theme, vc: &VcRuntimeInfo) -> impl IntoElement {
        let runtimes: Vec<_> = vc.runtimes.clone();
        let checks: Vec<DxCheck> = vc.checks.clone();
        crate::ui::table_container(theme)
            .child(
                div()
                    .px_5()
                    .py_3()
                    .text_size(px(14.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child("VC++ 运行库"),
            )
            .children(checks.iter().map(|c| self.check_row(theme, c)))
            .child(
                div()
                    .flex_col()
                    .px_5()
                    .py_2()
                    .children(runtimes.iter().map(|r| {
                        let name = r.name.clone();
                        let arch = r.arch.clone();
                        let ver = r.version.clone();
                        let installed = r.installed;
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .py_0p5()
                            .child(
                                div()
                                    .size(px(6.0))
                                    .rounded_full()
                                    .bg(if installed { theme.success } else { theme.danger }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(12.0))
                                    .text_color(theme.text)
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.text_muted)
                                    .child(arch),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(if installed { theme.text_muted } else { theme.danger })
                                    .child(if installed { ver } else { "未安装".to_string() }),
                            )
                    })),
            )
    }

    /// AI 工具卡（10 项并行检测结果）
    fn ai_card(
        &self,
        theme: &Theme,
        ai: &Option<AiToolsInfo>,
        loading: bool,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        crate::ui::table_container(theme)
            .child(
                div()
                    .px_5()
                    .py_3()
                    .text_size(px(14.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child("AI 开发工具"),
            )
            .when(loading, |s| {
                s.child(crate::ui::table_empty(theme, "检测中…（npm 查询，请稍候）"))
            })
            .when(ai.is_none() && !loading, |s| {
                s.child(crate::ui::table_empty(theme, "点击「重新检测」运行 AI 工具检测"))
            })
            .when_some(ai.clone(), |s, info| {
                let tools = info.tools;
                let checks: Vec<DxCheck> = info.checks;
                s.children(checks.iter().map(|c| self.check_row(theme, c)))
                    .child(
                        div()
                            .flex_col()
                            .children(tools.into_iter().map(|t| {
                                let name = t.display_name;
                                let cmd = t.name;
                                let installed = t.installed;
                                let version = t.version;
                                let upgradable = t.upgradable;
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_5()
                                    .py_1p5()
                                    .border_b_1()
                                    .border_color(theme.border)
                                    .child(
                                        div()
                                            .size(px(6.0))
                                            .rounded_full()
                                            .bg(if installed { theme.success } else { theme.text_muted }),
                                    )
                                    .child(
                                        div()
                                            .w(px(130.0))
                                            .flex_none()
                                            .text_size(px(12.5))
                                            .text_color(theme.text)
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .w(px(90.0))
                                            .flex_none()
                                            .text_size(px(11.0))
                                            .text_color(theme.text_muted)
                                            .child(cmd),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(12.0))
                                            .text_color(if installed { theme.text_muted } else { theme.text_muted })
                                            .child(if installed { version } else { "未安装".to_string() }),
                                    )
                                    .when(installed && upgradable, |r| {
                                        r.child(
                                            div()
                                                .text_size(px(11.0))
                                                .text_color(theme.warn)
                                                .child("可升级"),
                                        )
                                    })
                            })),
                    )
            })
    }
}
