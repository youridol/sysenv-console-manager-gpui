// secm-app::pages::settings — 系统设置页（全量对齐上游 Settings.tsx 能力面）
//
// 功能分组：
//   ① 系统优化开关（游戏类 5 项 + 系统类 1 项：HAGS/游戏模式/窗口化优化/鼠标精准度/VRR/关闭高精度计时器）
//   ② 电源计划（列表/滑动开关切换/删除+二次确认/卓越性能导入 + 首载自动导入激活）
//   ③ 核心线程调度策略（混合架构 CPU：线程/短运行 × AC/DC 独立六档，AMD/Intel 通用）
//   ④ NVIDIA 显卡电源管理（NVAPI DRS：自适应/最高性能优先/最佳功率，写后校验回读）
//
// 并发模型：初始状态（开关/电源计划/异类策略/NVIDIA 模式）后台线程一次加载；
// 所有写操作在后台线程执行，完成后后台重读回填真值；写操作全局互斥（busy）
// 防并发写注册表/DRS。主线程仅渲染当前状态。UI 反馈：全局 Toast + 状态横幅 + 日志流。

use gpui::prelude::*;
use gpui::{div, px, Context, Render, SharedString, WeakEntity, Window};
use secm_core::settings::{self, HeteroPolicies, NvidiaPowerMode, PowerPlan, SettingState};

use crate::pi_clone::theme::{Appearance, Palette};
use crate::ui::page::{
    badge, banner, button_sm, card, card_divider, card_grid_row, confirm_modal, grid_cell,
    page_header, page_root, section_title, soft, table_empty, BannerKind, ButtonKind,
};
use crate::ui::toast;

/// 可切换设置项（枚举明确区分调用函数）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToggleKind {
    Hags,
    GameMode,
    GameOptimization,
    Vrr,
    MousePrecision,
    Hpt,
}

impl ToggleKind {
    /// 全部开关（游戏类 5 项在前，系统类 HPT 收尾，与上游分组一致）
    const ALL: [ToggleKind; 6] = [
        ToggleKind::Hags,
        ToggleKind::GameMode,
        ToggleKind::GameOptimization,
        ToggleKind::Vrr,
        ToggleKind::MousePrecision,
        ToggleKind::Hpt,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Hags => "GPU 硬件加速调度 (HAGS)",
            Self::GameMode => "游戏模式",
            Self::GameOptimization => "窗口化游戏优化",
            Self::Vrr => "可变刷新率 (VRR)",
            Self::MousePrecision => "鼠标精准度（增强指针精确度）",
            Self::Hpt => "关闭高精度计时器",
        }
    }

    /// 静态能力描述（对齐上游 SettingRow description 文案）
    fn description(self) -> &'static str {
        match self {
            Self::Hags => "使用 GPU 调度器管理显存，降低渲染延迟",
            Self::GameMode => "通过在后台关闭内容来优化电脑玩游戏",
            Self::GameOptimization => "提升窗口化 / 无边框游戏流畅度",
            Self::Vrr => "显示器动态刷新率，消除画面撕裂",
            Self::MousePrecision => "提高指针精确度（鼠标加速），FPS 玩家通常关闭",
            Self::Hpt => "降低计时器中断频率，减少 CPU 占用（需重启生效）",
        }
    }

    /// 开关控件元素 id（滚动锚点/事件去重）
    fn dom_id(self) -> &'static str {
        match self {
            Self::Hags => "toggle-hags",
            Self::GameMode => "toggle-gamemode",
            Self::GameOptimization => "toggle-gameopt",
            Self::Vrr => "toggle-vrr",
            Self::MousePrecision => "toggle-mouseprec",
            Self::Hpt => "toggle-hpt",
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
            Self::Hpt => settings::get_hpt_state(),
        }
    }

    /// 写入目标状态（后台线程调用；写入后由调用方重读回填真值）
    fn set(self, enabled: bool) -> Result<SettingState, String> {
        match self {
            Self::Hags => settings::set_hags_state(enabled),
            Self::GameMode => settings::set_game_mode_state(enabled),
            Self::GameOptimization => settings::set_game_optimization(enabled),
            Self::Vrr => settings::set_vrr_state(enabled),
            Self::MousePrecision => settings::set_mouse_precision(enabled),
            Self::Hpt => settings::set_hpt_state(enabled),
        }
    }
}

/// 异类调度策略取值（0-5 → 中文标签，与电源选项面板一致）
const HETERO_CHOICES: &[(u32, &str)] = &[
    (0, "所有处理器"),
    (1, "高性能处理器"),
    (2, "首选高性能处理器"),
    (3, "高效处理器"),
    (4, "首选高效处理器"),
    (5, "自动"),
];

/// NVIDIA 电源模式三档（显示顺序对齐上游：最佳功率/最高性能优先/自适应）
const NVIDIA_CHOICES: &[(NvidiaPowerMode, &str)] = &[
    (NvidiaPowerMode::Optimal, "最佳功率"),
    (NvidiaPowerMode::MaxPerformance, "最高性能优先"),
    (NvidiaPowerMode::Adaptive, "自适应"),
];

/// 全部设置状态（后台一次加载）
struct AllSettings {
    toggles: Vec<(ToggleKind, SettingState)>,
    plans: Vec<PowerPlan>,
    hetero: Option<HeteroPolicies>,
    hetero_err: Option<String>,
    nvidia: Option<NvidiaPowerMode>,
    nvidia_err: Option<String>,
}

/// 写操作类别：全局互斥（同一时刻仅一个系统写操作）+ 对应控件 loading 视觉
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BusyOp {
    None,
    Toggle(ToggleKind),
    PlanActivate,
    PlanDelete,
    ImportUltimate,
    AutoUltimate,
    Hetero,
    Nvidia,
}

pub struct SettingsView {
    /// 页面外观，随壳主题联动
    appearance: Appearance,
    /// 页面滚动状态（GPUI 0.2 滚轮需 track_scroll 手动驱动，见 ui::page::page_root）
    page_scroll: gpui::ScrollHandle,
    /// 开关状态列表
    toggles: Vec<(ToggleKind, SettingState)>,
    /// 电源计划列表
    plans: Vec<PowerPlan>,
    /// 异类调度策略（读取成功才有值）
    hetero: Option<HeteroPolicies>,
    /// 异类策略读取失败原因（注册表异常等 → 错误态；设置项缺失 → present 标志 + 注入提示）
    hetero_err: Option<String>,
    /// NVIDIA 电源管理模式（读取成功才有值）
    nvidia: Option<NvidiaPowerMode>,
    /// NVIDIA 不可用/读取失败原因（无显卡/驱动/NVAPI 异常 → 错误态并隐藏选择组）
    nvidia_err: Option<String>,
    /// 初始状态是否加载中
    loading: bool,
    /// 当前写操作（全局互斥 + loading 定位）
    busy: BusyOp,
    /// 状态消息（操作反馈横幅）
    status: SharedString,
    /// 卓越性能导入反馈
    ultimate_msg: SharedString,
    /// 待删除确认的电源计划（Some = 显示确认弹层）
    confirm_delete: Option<PowerPlan>,
    /// 首载自动导入/激活卓越性能只执行一次（避免用户手动切换后被自动改回）
    auto_ultimate_done: bool,
}

impl SettingsView {
    pub fn new(appearance: Appearance, cx: &mut Context<Self>) -> Self {
        log::info!("系统设置 · 页面已打开");
        let mut v = Self {
            appearance,
            page_scroll: gpui::ScrollHandle::new(),
            toggles: Vec::new(),
            plans: Vec::new(),
            hetero: None,
            hetero_err: None,
            nvidia: None,
            nvidia_err: None,
            loading: false,
            busy: BusyOp::None,
            status: SharedString::from("正在读取设置状态…"),
            ultimate_msg: SharedString::from(""),
            confirm_delete: None,
            auto_ultimate_done: false,
        };
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

    /// 后台加载全部状态（开关/电源计划/异类策略/NVIDIA 模式）
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
                let data = exec
                    .spawn(async move {
                        // 异类策略读取失败不致命：记录原因供 UI 错误态展示
                        let (hetero, hetero_err) = match settings::get_hetero_policies() {
                            Ok(h) => (Some(h), None),
                            Err(e) => (None, Some(e)),
                        };
                        // NVIDIA 模式读取失败不致命：错误态展示（无 NVIDIA 卡/NVAPI 不可用时优雅降级）
                        let (nvidia, nvidia_err) = match settings::get_nvidia_power_mode() {
                            Ok(m) => (Some(m), None),
                            Err(e) => (None, Some(e)),
                        };
                        AllSettings {
                            toggles: ToggleKind::ALL.into_iter().map(|k| (k, k.get())).collect(),
                            plans: settings::get_power_plans().unwrap_or_default(),
                            hetero,
                            hetero_err,
                            nvidia,
                            nvidia_err,
                        }
                    })
                    .await;
                if let Some(view) = weak.upgrade() {
                    let load_plans = view.update(cx, |this, cx| {
                        this.loading = false;
                        this.toggles = data.toggles;
                        this.plans = data.plans;
                        this.hetero = data.hetero;
                        this.hetero_err = data.hetero_err;
                        this.nvidia = data.nvidia;
                        this.nvidia_err = data.nvidia_err;
                        this.status = SharedString::from("");
                        cx.notify();
                        // 首载完成后自动导入/激活卓越性能（对齐上游 useEffect 行为）
                        this.maybe_auto_ultimate(cx);
                    });
                    let _ = load_plans;
                }
            },
        )
        .detach();
    }

    /// 首载自动导入并激活卓越性能（只执行一次；失败仅记日志，不阻塞页面功能）
    fn maybe_auto_ultimate(&mut self, cx: &mut Context<Self>) {
        if self.auto_ultimate_done || self.plans.is_empty() {
            return;
        }
        self.auto_ultimate_done = true;
        let has_ultimate = self.plans.iter().any(|p| p.name == "卓越性能");
        if !has_ultimate {
            log::info!("系统设置 · 未检测到卓越性能计划，自动导入");
            self.busy = BusyOp::AutoUltimate;
            self.status = SharedString::from("正在自动导入卓越性能计划…");
            cx.notify();
            let weak: WeakEntity<Self> = cx.entity().downgrade();
            cx.spawn(async move |_this, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                // 后台：导入 → 重读列表 → 未激活则立即激活（对齐上游"导入后自动激活"完整链条）
                let result = exec
                    .spawn(async move {
                        let import = settings::enable_ultimate_performance();
                        match &import {
                            Ok(msg) => {
                                log::info!("系统设置 · 自动导入卓越性能计划成功: {}", msg)
                            }
                            Err(e) => log::warn!(
                                "系统设置 · 自动导入卓越性能计划失败（可手动导入）: {}",
                                e
                            ),
                        }
                        // 导入成功 → 自动激活（激活失败仅记日志，不阻塞页面）
                        if import.is_ok() {
                            if let Ok(plans) = settings::get_power_plans() {
                                if let Some(u) = plans.iter().find(|p| p.name == "卓越性能") {
                                    if !u.is_active {
                                        match settings::set_power_plan(&u.guid) {
                                            Ok(()) => {
                                                log::info!("系统设置 · 卓越性能计划已自动激活")
                                            }
                                            Err(e) => log::warn!(
                                                "系统设置 · 卓越性能计划自动激活失败: {}",
                                                e
                                            ),
                                        }
                                    }
                                }
                            }
                        }
                        import
                    })
                    .await;
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.busy = BusyOp::None;
                        this.ultimate_msg = match result {
                            Ok(msg) => SharedString::from(msg),
                            Err(ref e) => SharedString::from(format!("自动导入失败：{}", e)),
                        };
                        this.status = SharedString::from("");
                        cx.notify();
                        this.start_reload_plans(cx);
                    })
                    .ok();
                }
            })
            .detach();
        } else if let Some(plan) = self.plans.iter().find(|p| p.name == "卓越性能") {
            // 已存在：自动激活（对齐上游：存在但未激活 → handlePowerPlan）
            if !plan.is_active {
                log::info!("系统设置 · 检测到卓越性能计划未激活，自动激活");
                let guid = plan.guid.clone();
                self.activate_plan(&guid, cx);
            }
        }
    }

    /// 切换开关（后台写 + 后台重读该开关真值）
    fn toggle(&mut self, kind: ToggleKind, cx: &mut Context<Self>) {
        if self.busy != BusyOp::None {
            return;
        }
        self.busy = BusyOp::Toggle(kind);
        // 乐观 UI：立即反相显示当前开关
        if let Some((_, st)) = self.toggles.iter_mut().find(|(k, _)| *k == kind) {
            st.enabled = !st.enabled;
        }
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        let kind_c = kind;
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                // 后台：读当前真值 → 反相写入（写可能被系统拒绝，之后重读回填）
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
                        this.busy = BusyOp::None;
                        // 全局泡泡提示：开关切换结果随屏可见
                        match &result {
                            Ok(s) => {
                                toast::success(
                                    format!(
                                        "已{}「{}」",
                                        if s.enabled { "开启" } else { "关闭" },
                                        kind_c.label()
                                    ),
                                    cx,
                                );
                                this.status = SharedString::from(s.message.clone());
                            }
                            Err(e) => {
                                toast::error(format!("切换「{}」失败：{}", kind_c.label(), e), cx);
                                this.status = SharedString::from(format!("操作失败: {}", e));
                            }
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
                            if let Some((_, st)) = this.toggles.iter_mut().find(|(tk, _)| *tk == k2)
                            {
                                *st = new_state;
                            }
                            cx.notify();
                        })
                        .ok();
                    }
                }
            },
        )
        .detach();
    }

    /// 切换电源计划（后台执行）
    fn activate_plan(&mut self, guid: &str, cx: &mut Context<Self>) {
        if self.busy != BusyOp::None {
            return;
        }
        self.busy = BusyOp::PlanActivate;
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
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
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
                        this.busy = BusyOp::None;
                        // 全局泡泡提示：电源计划切换结果
                        match result {
                            Ok(()) => {
                                toast::success(format!("电源计划已切换：{}", name_c), cx);
                                this.status = SharedString::from("电源计划已切换");
                            }
                            Err(ref e) => {
                                toast::error(format!("切换「{}」失败：{}", name_c, e), cx);
                                this.status = SharedString::from(format!("切换失败: {}", e));
                            }
                        }
                        cx.notify();
                        // 后台重读计划列表与当前激活
                        this.start_reload_plans(cx);
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    /// 删除电源计划（确认弹层「删除」按钮触发；后台执行）
    fn delete_confirmed_plan(&mut self, cx: &mut Context<Self>) {
        let Some(plan) = self.confirm_delete.take() else {
            return;
        };
        if self.busy != BusyOp::None {
            cx.notify();
            return;
        }
        self.busy = BusyOp::PlanDelete;
        self.status = SharedString::from(format!("正在删除电源计划「{}」…", plan.name));
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        let guid_c = plan.guid.clone();
        let name_c = plan.name.clone();
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                let result = exec
                    .spawn(async move { settings::delete_power_plan(&guid_c) })
                    .await;
                // UI 侧日志：电源计划删除结果
                match &result {
                    Ok(()) => log::info!("系统设置 · 已删除电源计划 {}", name_c),
                    Err(e) => log::warn!("系统设置 · 删除电源计划 {} 失败: {}", name_c, e),
                }
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.busy = BusyOp::None;
                        match result {
                            Ok(()) => {
                                toast::success(format!("已删除电源计划「{}」", name_c), cx);
                                this.status = SharedString::from("电源计划已删除");
                            }
                            Err(ref e) => {
                                toast::error(format!("删除「{}」失败：{}", name_c, e), cx);
                                this.status = SharedString::from(format!("删除失败: {}", e));
                            }
                        }
                        cx.notify();
                        this.start_reload_plans(cx);
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    /// 后台仅重读电源计划列表
    fn start_reload_plans(&mut self, cx: &mut Context<Self>) {
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
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
            },
        )
        .detach();
    }

    /// 后台仅重读异类策略（策略写入后回填真值）
    fn start_reload_hetero(&mut self, cx: &mut Context<Self>) {
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                let outcome = exec
                    .spawn(async move { settings::get_hetero_policies() })
                    .await;
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        match outcome {
                            Ok(h) => {
                                this.hetero = Some(h);
                                this.hetero_err = None;
                            }
                            Err(e) => this.hetero_err = Some(e),
                        }
                        cx.notify();
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    /// 设置异类调度策略（scope: "ac"/"dc" 单路写入；后台执行 + 重读回填）
    fn set_hetero(&mut self, kind: &str, scope: &str, value: u32, cx: &mut Context<Self>) {
        if self.busy != BusyOp::None {
            return;
        }
        self.busy = BusyOp::Hetero;
        self.status = SharedString::from("正在设置调度策略…");
        cx.notify();

        let kind_label = match (kind, scope) {
            ("short", "ac") => "短运行线程调度策略（AC）",
            ("short", _) => "短运行线程调度策略（DC）",
            (_, "ac") => "线程调度策略（AC）",
            (_, _) => "线程调度策略（DC）",
        };
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        let kind_c = kind.to_string();
        let scope_c = scope.to_string();
        let kind_label_c = kind_label.to_string();
        let value_c = value;
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                let result = exec
                    .spawn(async move {
                        settings::set_hetero_policy_scoped(
                            &kind_c,
                            value_c,
                            scope_c == "ac",
                            scope_c == "dc",
                        )
                    })
                    .await;
                // UI 侧日志：异类调度策略设置结果
                let value_label = hetero_label(value_c);
                match &result {
                    Ok(()) => log::info!("系统设置 · {}已设为「{}」", kind_label_c, value_label),
                    Err(e) => log::warn!("系统设置 · 设置{}失败: {}", kind_label_c, e),
                }
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.busy = BusyOp::None;
                        // 全局泡泡提示：调度策略设置结果
                        match result {
                            Ok(()) => {
                                toast::success(
                                    format!("{}已设为「{}」", kind_label_c, hetero_label(value_c)),
                                    cx,
                                );
                                this.status = SharedString::from(format!(
                                    "{}已设为「{}」",
                                    kind_label_c,
                                    hetero_label(value_c)
                                ));
                            }
                            Err(ref e) => {
                                toast::error(format!("设置{}失败：{}", kind_label_c, e), cx);
                                this.status = SharedString::from(format!("设置失败: {}", e));
                            }
                        }
                        cx.notify();
                        // 后台重读异类策略，回填真实值
                        this.start_reload_hetero(cx);
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    /// 导入并激活卓越性能电源计划（手动按钮；后台执行）
    fn import_ultimate(&mut self, cx: &mut Context<Self>) {
        if self.busy != BusyOp::None {
            return;
        }
        self.busy = BusyOp::ImportUltimate;
        self.status = SharedString::from("正在导入卓越性能计划…");
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
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
                        this.busy = BusyOp::None;
                        // 全局泡泡提示：卓越性能导入结果
                        match result {
                            Ok(msg) => {
                                toast::success("卓越性能电源计划已导入并激活", cx);
                                this.ultimate_msg = SharedString::from(msg);
                                this.status = SharedString::from("卓越性能电源计划已导入并激活");
                            }
                            Err(ref e) => {
                                toast::error(format!("导入卓越性能计划失败：{}", e), cx);
                                this.status = SharedString::from(format!("导入失败: {}", e));
                            }
                        }
                        cx.notify();
                        this.start_reload_plans(cx);
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    /// 切换 NVIDIA 电源管理模式（后台写 + 后端写后校验 + 重读回填真值）
    fn set_nvidia(&mut self, mode: NvidiaPowerMode, cx: &mut Context<Self>) {
        if self.busy != BusyOp::None {
            return;
        }
        if self.nvidia == Some(mode) {
            return;
        }
        self.busy = BusyOp::Nvidia;
        self.nvidia_err = None;
        self.status = SharedString::from("正在切换 NVIDIA 电源管理模式…");
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        let mode_c = mode;
        let label_c = nvidia_label(mode);
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                // 后端 set_power_mode 内部含 SaveSettings 后重读校验（防驱动静默拒绝）
                let result = exec
                    .spawn(async move { settings::set_nvidia_power_mode(mode_c) })
                    .await;
                match &result {
                    Ok(()) => log::info!("系统设置 · NVIDIA 电源管理模式已切换为「{}」", label_c),
                    Err(e) => log::warn!("系统设置 · 切换 NVIDIA 电源管理模式失败: {}", e),
                }
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.busy = BusyOp::None;
                        match result {
                            Ok(()) => {
                                toast::success(
                                    format!("NVIDIA 电源管理模式已切换为「{}」", label_c),
                                    cx,
                                );
                                this.status = SharedString::from("NVIDIA 电源管理模式已切换");
                            }
                            Err(ref e) => {
                                toast::error(format!("切换 NVIDIA 电源模式失败：{}", e), cx);
                                this.status = SharedString::from(format!("切换失败: {}", e));
                            }
                        }
                        cx.notify();
                    })
                    .ok();
                    // 无论成败：后台重读真实模式回填 UI（失败回滚到真实值，防界面与实况不符）
                    let exec = cx.background_executor().clone();
                    let outcome = exec
                        .spawn(async move { settings::get_nvidia_power_mode() })
                        .await;
                    if let Some(view) = weak.upgrade() {
                        view.update(cx, |this, cx| {
                            match outcome {
                                Ok(m) => {
                                    this.nvidia = Some(m);
                                    this.nvidia_err = None;
                                }
                                Err(e) => this.nvidia_err = Some(e),
                            }
                            cx.notify();
                        })
                        .ok();
                    }
                }
            },
        )
        .detach();
    }
}

/// 异类策略取值 → 中文标签
fn hetero_label(value: u32) -> &'static str {
    HETERO_CHOICES
        .iter()
        .find(|(v, _)| *v == value)
        .map(|(_, l)| *l)
        .unwrap_or("自动")
}

/// NVIDIA 模式 → 中文标签
fn nvidia_label(mode: NvidiaPowerMode) -> &'static str {
    NVIDIA_CHOICES
        .iter()
        .find(|(m, _)| *m == mode)
        .map(|(_, l)| *l)
        .unwrap_or("—")
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.pal();
        let status = self.status.clone();

        // 页面主内容（page_root 为滚动容器；模态弹层须挂其外层避免随内容滚动）
        let content = page_root(&pal, "settings-page-root", &self.page_scroll, &cx.entity())
            // 页头：左标题 + 副标题
            .child(page_header(
                &pal,
                "系统设置",
                "系统优化开关 · 电源计划 · 核心策略 · NVIDIA 电源管理",
            ))
            // 状态消息（操作反馈，非空时以统一横幅展示）
            .when(!status.is_empty(), |s| {
                let msg = status.clone();
                s.child(banner(&pal, BannerKind::Info, msg))
            })
            // 第一行两列：系统优化开关 | 电源计划（容器级自适应，窄区自动堆叠）
            .child(
                card_grid_row()
                    .child(grid_cell().child(self.toggles_card(&pal, cx)))
                    .child(grid_cell().child(self.plans_card(&pal, cx))),
            )
            // 第二行全宽：核心线程调度策略（仅混合架构支持；不支持时降级说明）
            .child(self.hetero_card(&pal, cx))
            // 第三行全宽：NVIDIA 显卡电源管理（无 NVIDIA/NVAPI 不可用时降级说明）
            .child(self.nvidia_card(&pal, cx));

        // 相对容器承载模态确认弹层（deferred 置顶绘制 + 覆盖页面可视区）
        if self.confirm_delete.is_some() {
            let plan_name = self
                .confirm_delete
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_default();
            let modal = confirm_modal(
                &pal,
                "plan-delete-backdrop",
                "删除电源计划",
                format!(
                    "确定要删除电源计划「{}」吗？此操作不可恢复（需重新创建）。",
                    plan_name
                ),
                "删除",
                ButtonKind::Danger,
                cx.listener(|this, _, _, cx| {
                    this.delete_confirmed_plan(cx);
                }),
                cx.listener(|this, _, _, cx| {
                    this.confirm_delete = None;
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

impl SettingsView {
    /// 系统优化开关卡（游戏类 5 项 + 系统类 1 项分组；行：标题+管理员徽标+描述+状态+开关）
    fn toggles_card(&self, pal: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        let mut card = card(pal)
            // 卡头（标题 + 副标题，对齐上游"管理 Windows 游戏和性能相关设置"）
            .child(
                div()
                    .flex_col()
                    .gap_1()
                    .px(px(20.0))
                    .pt(px(12.0))
                    .pb(px(8.0))
                    .child(
                        div()
                            .text_size(px(14.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal.text)
                            .child("系统优化开关"),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(pal.text_muted)
                            .child("管理 Windows 游戏和性能相关设置"),
                    ),
            )
            .child(card_divider(pal))
            // 游戏类小节标题
            .child(
                div()
                    .px(px(20.0))
                    .pt(px(10.0))
                    .pb(px(4.0))
                    .child(section_title(pal, "游戏类")),
            );

        // 游戏类 5 项
        for (i, kind) in ToggleKind::ALL.iter().enumerate() {
            // 系统类（HPT）前插入分隔 + 小节标题
            if *kind == ToggleKind::Hpt {
                card = card.child(
                    div()
                        .px(px(20.0))
                        .pt(px(10.0))
                        .pb(px(4.0))
                        .mt_1()
                        .border_t_1()
                        .border_color(pal.border)
                        .child(section_title(pal, "系统类")),
                );
            }
            let state = self
                .toggles
                .iter()
                .find(|(k, _)| k == kind)
                .map(|(_, s)| s.clone());
            card =
                card.child(self.toggle_row(pal, *kind, state, i + 1 < ToggleKind::ALL.len(), cx));
        }
        card
    }

    /// 单个开关行：左（标题 + 管理员徽标 / 静态描述 / 当前状态消息）+ 右（滑动开关）
    fn toggle_row(
        &self,
        pal: &Palette,
        kind: ToggleKind,
        state: Option<SettingState>,
        with_border: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let enabled = state.as_ref().map(|s| s.enabled).unwrap_or(false);
        let admin_required = state.as_ref().map(|s| s.admin_required).unwrap_or(false);
        let message = state
            .as_ref()
            .map(|s| s.message.clone())
            .unwrap_or_else(|| "读取中…".to_string());
        // 该开关写操作进行中 → 开关置灰禁点（对应控件 loading 视觉）
        let disabled = self.busy == BusyOp::Toggle(kind);

        let mut row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px(px(20.0))
            .py(px(10.0));
        if with_border {
            row = row.border_b_1().border_color(pal.border);
        }
        row.child(
            div()
                .flex_col()
                .gap(px(3.0))
                .min_w(px(0.0))
                // 行 1：标题 + 管理员徽标（admin_required=true 时显示）
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
                                .child(kind.label()),
                        )
                        .when(admin_required, |s| {
                            s.child(badge(pal, "管理员", pal.warning))
                        }),
                )
                // 行 2：静态能力描述
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(pal.text_muted)
                        .child(kind.description()),
                )
                // 行 3：当前真实状态消息（注册表/API 读值回显）
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(pal.text_dim)
                        .whitespace_nowrap()
                        .truncate()
                        .child(message),
                ),
        )
        .child(self.switch(
            pal,
            kind.dom_id(),
            enabled,
            disabled,
            move |this, cx| {
                this.toggle(kind, cx);
            },
            cx,
        ))
    }

    /// 电源计划卡（卡头导入按钮 + 计划行[删除钮 + 滑动开关] + 卓越导入反馈）
    fn plans_card(&self, pal: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        let mut card = card(pal)
            .child(
                div()
                    .flex_col()
                    .gap_1()
                    .px(px(20.0))
                    .pt(px(12.0))
                    .pb(px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(14.5))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(pal.text)
                                    .child("电源计划"),
                            )
                            .child(
                                button_sm(pal, ButtonKind::Secondary)
                                    .id("import-ultimate")
                                    .when(self.busy == BusyOp::ImportUltimate, |s| {
                                        s.opacity(0.55).cursor_default()
                                    })
                                    .when(self.busy != BusyOp::ImportUltimate, |s| {
                                        s.on_click(cx.listener(|this, _, _, cx| {
                                            this.import_ultimate(cx);
                                        }))
                                    })
                                    .child("导入卓越计划"),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(pal.text_muted)
                            .child("性能电源计划 · 导入 / 删除 / 切换激活"),
                    ),
            )
            .when(!self.ultimate_msg.is_empty(), |s| {
                let m = self.ultimate_msg.clone();
                s.child(
                    div()
                        .px(px(20.0))
                        .pt(px(6.0))
                        .text_size(px(10.5))
                        .text_color(pal.text_muted)
                        .child(m),
                )
            })
            .child(card_divider(pal));

        if self.plans.is_empty() {
            // 读取失败/无计划：空态反馈（不静默留白）
            if !self.loading {
                card = card.child(table_empty(
                    pal,
                    "未读取到电源计划（可能权限不足或注册表异常）",
                ));
            }
            return card;
        }

        let count = self.plans.len();
        for (i, p) in self.plans.iter().enumerate() {
            card = card.child(self.plan_row(pal, p, i + 1 < count, cx));
        }
        card
    }

    /// 单个电源计划行：名称（激活高亮 + 使用中标记）+ 删除钮（仅非激活）+ 激活滑动开关
    fn plan_row(
        &self,
        pal: &Palette,
        plan: &PowerPlan,
        with_border: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let active = plan.is_active;
        // 开关回调要求 'static 闭包：先克隆 guid，避免捕获 &PowerPlan 引用
        let plan_guid = plan.guid.clone();
        let guid8 = SharedString::from(plan.guid.chars().take(8).collect::<String>());
        let switching = self.busy == BusyOp::PlanActivate;
        let deleting = self.busy == BusyOp::PlanDelete;

        let mut row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px(px(20.0))
            .py(px(9.0));
        if with_border {
            row = row.border_b_1().border_color(pal.border);
        }

        row.child(
            // 左：名称（激活=正文字色+「使用中」标记；未激活=弱化+悬停底提示可点）
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_w(px(0.0))
                .child(
                    div()
                        .min_w(px(0.0))
                        .whitespace_nowrap()
                        .truncate()
                        .text_size(px(13.0))
                        .font_weight(if active {
                            gpui::FontWeight::MEDIUM
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .text_color(if active { pal.text } else { pal.text_muted })
                        .child(plan.name.clone()),
                )
                .when(active, |s| {
                    s.child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(10.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(pal.success)
                            .child("使用中"),
                    )
                }),
        )
        .child(
            // 右：删除（仅非激活计划展示；激活计划不可删——先切换其他计划）+ 激活开关
            div()
                .flex()
                .items_center()
                .gap_2()
                .flex_shrink_0()
                .when(!active, |s| {
                    let guid = plan.guid.clone();
                    s.child(
                        button_sm(pal, ButtonKind::Danger)
                            .id(SharedString::from(format!("plan-del-{}", guid8)))
                            .when(deleting, |s| s.opacity(0.55).cursor_default())
                            .when(!deleting, |s| {
                                s.on_click(cx.listener(move |this, _, _, cx| {
                                    // 打开删除确认弹层（二次确认防误删）
                                    this.confirm_delete =
                                        this.plans.iter().find(|p| p.guid == guid).cloned();
                                    cx.notify();
                                }))
                            })
                            .child("删除"),
                    )
                })
                .child(self.switch(
                    pal,
                    SharedString::from(format!("plan-switch-{}", guid8)),
                    active,
                    // 激活计划开关置亮但禁点；其他计划点击切换；任一计划操作中全部禁点
                    active || switching || deleting,
                    move |this, cx| {
                        this.activate_plan(&plan_guid, cx);
                    },
                    cx,
                )),
        )
    }

    /// 核心线程调度策略卡（混合架构 CPU；线程/短运行 × AC/DC 独立六档）
    fn hetero_card(&self, pal: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        let mut card =
            card(pal)
                .child(
                    div()
                        .flex_col()
                        .gap_1()
                        .px(px(20.0))
                        .pt(px(12.0))
                        .pb(px(8.0))
                        .child(
                            div()
                                .text_size(px(14.5))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(pal.text)
                                .child("核心线程调度策略"),
                        )
                        .child(div().text_size(px(11.5)).text_color(pal.text_muted).child(
                            "混合架构 CPU（AMD / Intel 适用）· 异类线程调度 AC/DC 独立六档",
                        )),
                )
                .child(card_divider(pal));

        if let Some(err) = &self.hetero_err {
            // 注册表读取异常：明确错误反馈（非静默）
            card = card.child(
                div()
                    .px(px(20.0))
                    .py(px(12.0))
                    .text_size(px(12.0))
                    .text_color(pal.danger)
                    .child(format!("读取调度策略失败：{}", err)),
            );
            return card;
        }
        let Some(h) = &self.hetero else {
            card = card.child(
                div()
                    .px(px(20.0))
                    .py(px(12.0))
                    .text_size(px(12.0))
                    .text_color(pal.text_muted)
                    .child("读取中…"),
            );
            return card;
        };

        // 设置项缺失提示（不再硬阻断）：当前电源计划未包含策略设置项时，
        // 首次配置自动注入到当前电源计划（set_hetero_policy_scoped 内置注入），保证可配置成功
        let missing = !h.thread_present || !h.short_present;
        if missing {
            let absent = match (h.thread_present, h.short_present) {
                (false, true) => "异类线程调度策略",
                (true, false) => "异类短运行线程调度策略",
                _ => "异类线程/短运行线程调度策略",
            };
            card = card.child(
                div()
                    .mx(px(20.0))
                    .mt(px(10.0))
                    .px(px(12.0))
                    .py(px(8.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(soft(pal.warning, 0.32))
                    .bg(soft(pal.warning, 0.10))
                    .text_size(px(11.5))
                    .text_color(pal.warning)
                    .child(format!(
                        "当前电源计划未包含{}设置项，首次配置时将自动注入当前电源计划（需管理员权限）",
                        absent
                    )),
            );
        }

        // 四组六档选择：线程 AC/DC + 短运行 AC/DC（DC 值未显式设置时不高亮，可直接写入）
        let busy = self.busy == BusyOp::Hetero;
        card.child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(16.0))
                .px(px(20.0))
                .py(px(10.0))
                .child(self.hetero_group(
                    pal,
                    "异类线程调度策略 · AC（外接供电）",
                    "thread",
                    "ac",
                    h.thread_ac,
                    busy,
                    cx,
                ))
                .child(self.hetero_group(
                    pal,
                    "异类线程调度策略 · DC（电池）",
                    "thread",
                    "dc",
                    h.thread_dc,
                    busy,
                    cx,
                ))
                .child(self.hetero_group(
                    pal,
                    "异类短运行线程调度策略 · AC（外接供电）",
                    "short",
                    "ac",
                    h.short_ac,
                    busy,
                    cx,
                ))
                .child(self.hetero_group(
                    pal,
                    "异类短运行线程调度策略 · DC（电池）",
                    "short",
                    "dc",
                    h.short_dc,
                    busy,
                    cx,
                )),
        )
    }

    /// 异类策略单组（标题 + 六档按钮组，当前档 accent 实底高亮）
    // 参数含作用域/忙碌态等语义位，均为显式状态传递（无隐藏耦合），允许超参
    #[allow(clippy::too_many_arguments)]
    fn hetero_group(
        &self,
        pal: &Palette,
        title: &str,
        kind: &'static str,
        scope: &'static str,
        current: Option<u32>,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let title = title.to_string();
        div()
            .flex_col()
            .gap_2()
            // 弹性两列：宽容器两组并排，窄容器自动换行堆叠
            .flex_1()
            .min_w(px(380.0))
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(pal.text)
                    .child(title),
            )
            .child(
                div()
                    .mt_1()
                    .flex()
                    .flex_wrap()
                    .gap_1p5()
                    .children(HETERO_CHOICES.iter().map(|(v, label)| {
                        let value = *v;
                        let is_cur = current == Some(value);
                        let label_owned = label.to_string();
                        let group_id =
                            SharedString::from(format!("hetero-{}-{}-{}", kind, scope, value));
                        div()
                            .id(group_id)
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .text_size(px(11.5))
                            // 选中档：accent 实底 + 对比色字；未选档：hover 底 + 悬停加深
                            .when(is_cur, |s| s.bg(pal.accent).text_color(pal.accent_contrast))
                            .when(!is_cur, |s| {
                                s.bg(pal.bg_hover)
                                    .hover(|s| s.bg(pal.bg_selected))
                                    .text_color(pal.text)
                            })
                            // 写操作进行中：全部档位禁点（互斥）
                            .when(busy, |s| s.opacity(0.55).cursor_default())
                            .when(!busy, |s| s.cursor_pointer())
                            .when(!busy && !is_cur, |s| {
                                s.on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_hetero(kind, scope, value, cx);
                                }))
                            })
                            .child(label_owned)
                    })),
            )
    }

    /// NVIDIA 显卡电源管理卡（NVAPI DRS；无 NVIDIA/NVAPI 不可用时降级说明）
    fn nvidia_card(&self, pal: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        let mut card = card(pal)
            .child(
                div()
                    .flex_col()
                    .gap_1()
                    .px(px(20.0))
                    .pt(px(12.0))
                    .pb(px(8.0))
                    .child(
                        div()
                            .text_size(px(14.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal.text)
                            .child("NVIDIA 显卡电源管理"),
                    )
                    .child(div().text_size(px(11.5)).text_color(pal.text_muted).child(
                        "管理 3D 设置 → 电源管理模式（NVAPI DRS 实时读写，写入后自动校验）",
                    )),
            )
            .child(card_divider(pal));

        if let Some(err) = &self.nvidia_err {
            // 优雅降级：无 NVIDIA 显卡 / 驱动未装 / NVAPI 不可用 → 明确错误说明
            card = card.child(
                div()
                    .px(px(20.0))
                    .py(px(12.0))
                    .text_size(px(12.0))
                    .text_color(pal.danger)
                    .child(err.clone()),
            );
            return card;
        }
        let Some(current) = self.nvidia else {
            card = card.child(
                div()
                    .px(px(20.0))
                    .py(px(12.0))
                    .text_size(px(12.0))
                    .text_color(pal.text_muted)
                    .child("读取中…"),
            );
            return card;
        };

        let busy = self.busy == BusyOp::Nvidia;
        card.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap_3()
                .px(px(20.0))
                .py(px(10.0))
                .child(
                    div()
                        .flex_col()
                        .gap(px(3.0))
                        .min_w(px(0.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(pal.text)
                                .child("电源管理模式"),
                        )
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(pal.text_dim)
                                .child(format!(
                                    "当前：{}（DRS 全局 profile）",
                                    nvidia_label(current)
                                )),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1p5()
                        .children(NVIDIA_CHOICES.iter().map(|(mode, label)| {
                            let mode = *mode;
                            let is_cur = current == mode;
                            let label_owned = label.to_string();
                            let item_id =
                                SharedString::from(format!("nvidia-{:?}", mode).to_lowercase());
                            div()
                                .id(item_id)
                                .px_2p5()
                                .py_1()
                                .rounded_md()
                                .text_size(px(11.5))
                                .when(is_cur, |s| s.bg(pal.accent).text_color(pal.accent_contrast))
                                .when(!is_cur, |s| {
                                    s.bg(pal.bg_hover)
                                        .hover(|s| s.bg(pal.bg_selected))
                                        .text_color(pal.text)
                                })
                                .when(busy, |s| s.opacity(0.55).cursor_default())
                                .when(!busy, |s| s.cursor_pointer())
                                .when(!busy && !is_cur, |s| {
                                    s.on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_nvidia(mode, cx);
                                    }))
                                })
                                .child(label_owned)
                        })),
                ),
        )
    }

    /// 滑动开关控件（圆钮式：开=accent 底对比色钮，关=hover 底弱化钮）
    ///
    /// `on_toggle` 由调用方以闭包注入（开关切换的统一视觉，多处复用）；
    /// `disabled=true` 时置灰并移除点击（写操作互斥/激活计划/加载中）。
    fn switch(
        &self,
        pal: &Palette,
        id: impl Into<gpui::ElementId>,
        enabled: bool,
        disabled: bool,
        on_toggle: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut sw = div()
            .id(id)
            .w(px(44.0))
            .h(px(24.0))
            .rounded_full()
            .when(enabled, |s| s.bg(pal.accent))
            .when(!enabled, |s| s.bg(pal.bg_hover))
            // 禁用态：置灰 + 默认光标（无点击语义）
            .when(disabled, |s| s.opacity(0.55).cursor_default())
            .when(!disabled, |s| s.cursor_pointer())
            .child(
                div()
                    .absolute()
                    .top(px(2.0))
                    .size(px(20.0))
                    .rounded_full()
                    // 开：圆钮右移取对比色；关：圆钮左移取弱化色
                    .when(enabled, |s| s.bg(pal.accent_contrast).right(px(2.0)))
                    .when(!disabled && !enabled, |s| {
                        s.bg(pal.text_muted).left(px(2.0))
                    })
                    .when(disabled && !enabled, |s| s.bg(pal.text_muted).left(px(2.0))),
            );
        if !disabled {
            sw = sw.on_click(cx.listener(move |this, _, _, cx| {
                on_toggle(this, cx);
            }));
        }
        sw
    }
}
