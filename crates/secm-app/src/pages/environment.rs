// secm-app::pages::environment — 环境检测页
// 系统信息 8 字段 + 游戏环境预设对比 + DirectX 诊断 + VC++ 运行库 + AI 工具检测。
//
// 呈现层：统一接入 crate::ui::page 页面布局框架（页头/卡片/横幅/按钮/键值行），
// 色板取自 pi_clone::theme::Palette，明暗随壳 Appearance 联动，禁止硬编码业务色。
//
// 并发模型：全部检测（注册表/命令/npm）在后台线程执行；View 构造仅占位并立即
// 启动后台任务，完成后经 WeakEntity 回 UI。主线程绝不执行检测或注册表写。

use gpui::prelude::*;
use gpui::{div, px, Context, Render, SharedString, WeakEntity, Window};
use secm_core::environment::{self, AiToolsInfo, CheckStatus, DirectXInfo, DxCheck, VcRuntimeInfo};
use secm_core::game_env::{self, GamePreset, GameSetting};
use secm_core::sysinfo::{self, SystemInfo};

use crate::pi_clone::theme::{Appearance, Palette};
use crate::ui::page::{
    banner, button, button_sm, card, card_body, card_divider, card_header, kv_row_w, page_header,
    page_root, table_empty, BannerKind, ButtonKind,
};

/// 静态检测结果包（后台一次算齐，回 UI 赋值）
struct StaticEnvData {
    system: SystemInfo,
    dx: DirectXInfo,
    vc: VcRuntimeInfo,
    presets: Vec<GamePreset>,
}

pub struct EnvironmentView {
    /// 系统信息 / DirectX / VC++ / 游戏预设（后台加载，加载完成前为 None）
    system: Option<SystemInfo>,
    dx: Option<DirectXInfo>,
    vc: Option<VcRuntimeInfo>,
    presets: Vec<GamePreset>,
    /// 静态检测是否进行中
    static_loading: bool,
    /// AI 工具（npm/where 慢 → 后台检测）
    ai: Option<AiToolsInfo>,
    ai_loading: bool,
    /// 一键套用预设是否进行中（注册表写后台执行）
    applying: bool,
    /// 页面错误/状态
    status: String,
    /// 页面外观，随壳主题联动
    appearance: Appearance,
    /// 页面滚动状态（GPUI 0.2 滚轮需 track_scroll 手动驱动，见 ui::page::page_root）
    page_scroll: gpui::ScrollHandle,
}

impl EnvironmentView {
    pub fn new(appearance: Appearance, cx: &mut Context<Self>) -> Self {
        log::info!("环境检测 · 页面已打开");
        let mut v = Self {
            system: None,
            dx: None,
            vc: None,
            presets: Vec::new(),
            static_loading: false,
            ai: None,
            ai_loading: false,
            applying: false,
            status: String::from("正在检测环境…"),
            appearance,
            page_scroll: gpui::ScrollHandle::new(),
        };
        v.start_static_load(cx);
        v.run_ai_check(cx);
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

    /// 后台加载静态检测（系统信息/DX/VC++/游戏预设 —— 注册表多线程 + PS 回退，秒级）
    fn start_static_load(&mut self, cx: &mut Context<Self>) {
        if self.static_loading {
            return;
        }
        self.static_loading = true;
        self.status = "正在检测环境（系统信息/DirectX/VC++）…".to_string();
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                let data = exec
                    .spawn(async move {
                        StaticEnvData {
                            system: sysinfo::get_system_info(),
                            dx: environment::check_directx(),
                            vc: environment::check_vc_runtimes(),
                            presets: game_env::get_game_presets(),
                        }
                    })
                    .await;
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.static_loading = false;
                        this.system = Some(data.system);
                        this.dx = Some(data.dx);
                        this.vc = Some(data.vc);
                        this.presets = data.presets;
                        this.status = String::new();
                        cx.notify();
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    /// 后台执行 AI 工具检测（npm 查询，耗时数秒）
    fn run_ai_check(&mut self, cx: &mut Context<Self>) {
        if self.ai_loading {
            return;
        }
        self.ai_loading = true;
        self.status = "AI 工具检测中…（npm 查询可能需要几秒）".to_string();
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                let result = exec
                    .spawn(async move { environment::check_ai_tools() })
                    .await;
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.ai_loading = false;
                        this.ai = Some(result);
                        this.status = String::new();
                        cx.notify();
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    /// 全页重新检测（两组各自后台并发）
    fn rescan(&mut self, cx: &mut Context<Self>) {
        // UI 侧日志：用户点击重新检测
        log::info!("环境检测 · 触发重新检测");
        self.start_static_load(cx);
        self.run_ai_check(cx);
        cx.notify();
    }

    /// 一键套用游戏预设（后台线程执行注册表写，完成后后台重读预设回填）
    fn apply_preset(&mut self, preset: &GamePreset, cx: &mut Context<Self>) {
        if self.applying {
            return;
        }
        self.applying = true;
        let preset_name = preset.name.clone();
        let preset_clone = preset.clone();
        self.status = format!("正在应用「{}」预设…", preset.name);
        // UI 侧日志：用户点击一键套用预设
        log::info!("环境检测 · 触发套用「{}」游戏预设", preset.name);
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                // 注册表写全部在后台线程
                let outcome = exec
                    .spawn(async move { apply_preset_settings(&preset_clone) })
                    .await;

                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.applying = false;
                        // P1-14：如实回显成功项数与失败明细
                        let mut msg = if outcome.applied > 0 {
                            format!(
                                "已应用「{}」预设（{} 项联动）",
                                preset_name, outcome.applied
                            )
                        } else if outcome.failures.is_empty() {
                            format!("「{}」全部达标，无需调整", preset_name)
                        } else {
                            format!("「{}」预设未应用任何变更", preset_name)
                        };
                        if !outcome.failures.is_empty() {
                            msg.push_str(&format!(
                                "；失败 {} 项: {}",
                                outcome.failures.len(),
                                outcome.failures.join("；")
                            ));
                        }
                        // UI 侧日志：预设套用结果（有失败项 → warn）
                        if outcome.failures.is_empty() {
                            log::info!("环境检测 · 套用「{}」预设完成", preset_name);
                        } else {
                            log::warn!(
                                "环境检测 · 套用「{}」预设部分失败（{} 项）: {}",
                                preset_name,
                                outcome.failures.len(),
                                outcome.failures.join("；")
                            );
                        }
                        this.status = msg;
                        cx.notify();
                    })
                    .ok();
                    // 后台重读预设（当前值变化），不在主线程跑
                    view.update(cx, |this, cx| {
                        this.start_static_load(cx);
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    fn check_color(status: &CheckStatus, pal: &Palette) -> gpui::Rgba {
        match status {
            CheckStatus::Pass => pal.success,
            CheckStatus::Warn => pal.warning,
            CheckStatus::Fail => pal.danger,
            CheckStatus::Info => pal.text_muted,
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
    fn check_row(&self, pal: &Palette, c: &DxCheck) -> impl IntoElement {
        let name = c.name.clone();
        let detail = c.detail.clone();
        let color = Self::check_color(&c.status, pal);
        let icon = Self::check_icon(&c.status).to_string();
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_5()
            .py_2()
            .border_b_1()
            .border_color(pal.border)
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
                    .text_color(pal.text)
                    .child(name),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(pal.text_muted)
                    .child(detail),
            )
    }
}

impl Render for EnvironmentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.pal();
        let status = self.status.clone();
        let ai_loading = self.ai_loading;
        let static_loading = self.static_loading;
        let ai = self.ai.clone();
        let sys = self.system.clone();
        let dx = self.dx.clone();
        let vc = self.vc.clone();
        let presets: Vec<GamePreset> = self.presets.clone();
        let applying = self.applying;

        // 统一页面骨架：根容器（统一内边距/纵向节奏/超高纵向滚动）
        page_root(
            &pal,
            "environment-page-root",
            &self.page_scroll,
            &cx.entity(),
        )
        // 页头：标题/副标题居左，右侧动作区挂「重新检测」（id/回调/文案逻辑保持）
        .child(
            page_header(
                &pal,
                "环境检测",
                "系统信息 · 游戏环境预设 · DirectX / VC++ · AI 工具",
            )
            .child(
                button(&pal, ButtonKind::Secondary)
                    .id("env-rescan")
                    .child(if static_loading || ai_loading {
                        "检测中…"
                    } else {
                        "重新检测"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.rescan(cx);
                    })),
            ),
        )
        // 状态
        .when(!status.is_empty(), |s| {
            let msg = status.clone();
            s.child(banner(&pal, BannerKind::Info, msg))
        })
        // 系统信息卡
        .child(self.system_card(&pal, &sys))
        // 游戏环境预设
        .child(self.presets_section(&pal, &presets, applying, cx))
        // DirectX + VC++ 双列（flex 等宽两列；禁 grid —— taffy grid 滚动容器内不渲染）
        .child(
            div()
                .flex()
                .gap_4()
                .child(div().flex_1().min_w(px(0.0)).child(self.dx_card(&pal, &dx)))
                .child(div().flex_1().min_w(px(0.0)).child(self.vc_card(&pal, &vc))),
        )
        // AI 工具卡
        .child(self.ai_card(&pal, &ai, ai_loading, cx))
    }
}

impl EnvironmentView {
    /// 系统信息 8 字段卡（None=后台加载中 → 占位）
    fn system_card(&self, pal: &Palette, s: &Option<SystemInfo>) -> impl IntoElement {
        let Some(s) = s else {
            return card(pal)
                .child(card_header(pal, "系统信息"))
                .child(card_divider(pal))
                .child(table_empty(pal, "检测中…"))
                .into_any_element();
        };
        let rows: Vec<(&str, String)> = vec![
            ("系统版本", s.edition.clone()),
            ("内部版本", format!("{} · UBR {}", s.build_number, s.ubr)),
            ("系统架构", s.arch.clone()),
            ("安装日期", s.install_date.clone()),
            (
                "激活状态",
                format!("{}（{}）", s.activation.label, s.activation.status_raw),
            ),
            (
                "最新补丁",
                format!("{} · {}", s.latest_patch.kb, s.latest_patch.title_cn),
            ),
            ("启动模式", s.boot_mode.clone()),
        ];
        card(pal)
            .child(card_header(pal, "系统信息"))
            .child(card_divider(pal))
            // 键值行统一走 kv_row 节奏（原行分隔线去掉，标签定宽 100 对齐）
            .child(
                card_body(pal).children(rows.into_iter().map(|(k, v)| kv_row_w(pal, 100.0, k, v))),
            )
            .into_any_element()
    }

    /// 游戏环境预设对比（含一键套用；applying=true 时禁用按钮防重复提交）
    fn presets_section(
        &self,
        pal: &Palette,
        presets: &[GamePreset],
        applying: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        card(pal)
            .child(card_header(pal, "游戏环境预设（推荐设置对比当前状态）"))
            .child(card_divider(pal))
            .children(presets.iter().map(|p| {
                let name = p.name.clone();
                let engine = p.engine.clone();
                let settings: Vec<GameSetting> = p.settings.clone();
                let ok_all = settings.iter().all(|s| s.ok || s.key.is_empty());
                let preset_for_click = p.clone();
                let applied_id = SharedString::from(format!("preset-{}", p.id));
                let disabled = applying;
                // 套用按钮：可套用=主操作钮，已达标=弱化幽灵钮；文案/互斥逻辑保持
                let btn_label = if ok_all {
                    "已达标"
                } else if disabled {
                    "应用中…"
                } else {
                    "一键套用"
                };
                let apply_btn = if ok_all {
                    button_sm(pal, ButtonKind::Ghost)
                } else {
                    button_sm(pal, ButtonKind::Primary)
                }
                .id(applied_id)
                .child(btn_label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !ok_all && !disabled {
                        this.apply_preset(&preset_for_click, cx);
                    }
                }));
                div()
                    .flex_col()
                    .px_5()
                    .py_3()
                    .border_b_1()
                    .border_color(pal.border)
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
                                            .text_color(pal.text)
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(pal.text_muted)
                                            .child(engine),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(if ok_all {
                                                pal.success
                                            } else {
                                                pal.warning
                                            })
                                            .child(if ok_all {
                                                "全部达标"
                                            } else {
                                                "有未达标项"
                                            }),
                                    ),
                            )
                            .child(apply_btn),
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
                            .child(div().size(px(6.0)).rounded_full().bg(if ok {
                                pal.success
                            } else {
                                pal.warning
                            }))
                            .child(
                                div()
                                    .w(px(150.0))
                                    .flex_none()
                                    .text_size(px(12.0))
                                    .text_color(pal.text)
                                    .child(label),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(11.0))
                                    .text_color(pal.text_muted)
                                    .child(desc),
                            )
                            .child(
                                div()
                                    .w(px(80.0))
                                    .flex_none()
                                    .text_size(px(11.5))
                                    .text_color(pal.text_muted)
                                    .child(format!("推荐 {}", rec)),
                            )
                            .child(
                                div()
                                    .w(px(80.0))
                                    .flex_none()
                                    .text_size(px(11.5))
                                    .text_color(if ok { pal.success } else { pal.danger })
                                    .child(format!("当前 {}", cur)),
                            )
                    }))
            }))
    }

    /// DirectX 诊断卡（None=加载中）
    fn dx_card(&self, pal: &Palette, dx: &Option<DirectXInfo>) -> impl IntoElement {
        let Some(dx) = dx else {
            return card(pal)
                .child(card_header(pal, "DirectX 诊断"))
                .child(card_divider(pal))
                .child(table_empty(pal, "检测中…"))
                .into_any_element();
        };
        let checks: Vec<DxCheck> = dx.checks.clone();
        let version = dx.version.clone();
        card(pal)
            // 卡片头标题 flex_1，版本号自动靠右（accent 强调）
            .child(
                card_header(pal, "DirectX 诊断").child(
                    div()
                        .text_size(px(12.0))
                        .text_color(pal.accent)
                        .child(SharedString::from(format!("DirectX {}", version))),
                ),
            )
            .child(card_divider(pal))
            .children(checks.iter().map(|c| self.check_row(pal, c)))
            .into_any_element()
    }

    /// VC++ 运行库卡（None=加载中）
    fn vc_card(&self, pal: &Palette, vc: &Option<VcRuntimeInfo>) -> impl IntoElement {
        let Some(vc) = vc else {
            return card(pal)
                .child(card_header(pal, "VC++ 运行库"))
                .child(card_divider(pal))
                .child(table_empty(pal, "检测中…"))
                .into_any_element();
        };
        let runtimes: Vec<_> = vc.runtimes.clone();
        let checks: Vec<DxCheck> = vc.checks.clone();
        card(pal)
            .child(card_header(pal, "VC++ 运行库"))
            .child(card_divider(pal))
            .children(checks.iter().map(|c| self.check_row(pal, c)))
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
                            .child(div().size(px(6.0)).rounded_full().bg(if installed {
                                pal.success
                            } else {
                                pal.danger
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(12.0))
                                    .text_color(pal.text)
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(pal.text_muted)
                                    .child(arch),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(if installed {
                                        pal.text_muted
                                    } else {
                                        pal.danger
                                    })
                                    .child(if installed {
                                        ver
                                    } else {
                                        "未安装".to_string()
                                    }),
                            )
                    })),
            )
            .into_any_element()
    }

    /// AI 工具卡（10 项并行检测结果）
    fn ai_card(
        &self,
        pal: &Palette,
        ai: &Option<AiToolsInfo>,
        loading: bool,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        card(pal)
            .child(card_header(pal, "AI 开发工具"))
            .child(card_divider(pal))
            .when(loading, |s| {
                s.child(table_empty(pal, "检测中…（npm 查询，请稍候）"))
            })
            .when(ai.is_none() && !loading, |s| {
                s.child(table_empty(pal, "点击「重新检测」运行 AI 工具检测"))
            })
            .when_some(ai.clone(), |s, info| {
                let tools = info.tools;
                let checks: Vec<DxCheck> = info.checks;
                s.children(checks.iter().map(|c| self.check_row(pal, c)))
                    .child(div().flex_col().children(tools.into_iter().map(|t| {
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
                            .border_color(pal.border)
                            .child(div().size(px(6.0)).rounded_full().bg(if installed {
                                pal.success
                            } else {
                                pal.text_muted
                            }))
                            .child(
                                div()
                                    .w(px(130.0))
                                    .flex_none()
                                    .text_size(px(12.5))
                                    .text_color(pal.text)
                                    .child(name),
                            )
                            .child(
                                div()
                                    .w(px(90.0))
                                    .flex_none()
                                    .text_size(px(11.0))
                                    .text_color(pal.text_muted)
                                    .child(cmd),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(12.0))
                                    .text_color(pal.text_muted)
                                    .child(if installed {
                                        version
                                    } else {
                                        "未安装".to_string()
                                    }),
                            )
                            .when(installed && upgradable, |r| {
                                r.child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(pal.warning)
                                        .child("可升级"),
                                )
                            })
                    })))
            })
    }
}

/// 预设套用结果（P1-14：历史实现 `let _ = settings::set_*` 吞错并按次数谎报
/// "已应用 N 项"，现逐项回传成败，失败项明细回显给用户）
struct PresetApplyOutcome {
    applied: u32,
    failures: Vec<String>,
}

/// 后台线程执行的预设套用（注册表写，逐项回传结果；勿在主线程调用）
fn apply_preset_settings(preset: &GamePreset) -> PresetApplyOutcome {
    use secm_core::settings;
    let mut outcome = PresetApplyOutcome {
        applied: 0,
        failures: Vec::new(),
    };
    for s in &preset.settings {
        if s.key.is_empty() {
            continue; // 纯检测项不可切换
        }
        if s.ok {
            continue; // 已达标跳过
        }
        let recommended_on = s.recommended.contains("开启")
            || s.recommended.contains("高性能")
            || s.recommended.contains("卓越性能");
        let result: Result<(), String> = match s.key.as_str() {
            "hags" => settings::set_hags_state(recommended_on).map(|_| ()),
            "game_mode" => settings::set_game_mode_state(recommended_on).map(|_| ()),
            "vrr" => settings::set_vrr_state(recommended_on).map(|_| ()),
            "mouse_precision" => {
                // 推荐"关闭"（recommended_on=false）→ 禁用增强指针精确度：
                // set_mouse_precision(false) 即 SPI_SETMOUSE 写入 [0,0,0]。
                // 历史 bug：曾写 !recommended_on，推荐"关闭"时反而启用（P1-1 修复）
                settings::set_mouse_precision(recommended_on).map(|_| ())
            }
            "power_plan" => {
                // 按推荐名激活对应电源计划
                match settings::get_power_plans() {
                    Err(e) => Err(format!("枚举电源计划失败: {}", e)),
                    Ok(plans) => {
                        let target = plans
                            .iter()
                            .find(|p| s.recommended.contains("高性能") && p.name.contains("高性能"))
                            .or_else(|| {
                                plans.iter().find(|p| {
                                    s.recommended.contains("卓越") && p.name.contains("卓越")
                                })
                            });
                        match target {
                            Some(plan) => settings::set_power_plan(&plan.guid).map(|_| ()),
                            None => Err(format!("未找到推荐电源计划（推荐: {}）", s.recommended)),
                        }
                    }
                }
            }
            _ => Ok(()),
        };
        match result {
            Ok(()) => outcome.applied += 1,
            Err(e) => outcome.failures.push(format!("{}: {}", s.label, e)),
        }
    }
    outcome
}
