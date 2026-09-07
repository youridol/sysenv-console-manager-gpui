// secm-app::pages::cleanup — 清理优化页
// 缓存清理（临时/着色器/NVIDIA/AMD/DirectX/Steam）+ 进程管理（Top 200 + 优先级）+ DNS 刷新。
// 清理为文件 IO，放后台线程执行避免卡 UI。
// 呈现层统一由 crate::ui::page 装配，色板取自 Palette（明暗随壳主题联动）。

use gpui::prelude::*;
use gpui::{div, px, Context, Entity, Render, SharedString, WeakEntity, Window};
use secm_core::cleanup::{self, CleanupResult, ProcessInfo};

use crate::pi_clone::theme::{Appearance, Palette};
use crate::ui::page::{
    badge, banner, button, button_sm, card, card_body, card_divider, card_header_accent,
    page_header, page_root, section_title, table_head, table_row, BannerKind, ButtonKind, ColWidth,
    CARD_PADDING,
};
use crate::ui::text_input::{ChangeText, TextField};

/// 清理操作类型（按钮 → 后台执行函数映射）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanOp {
    Temp,
    Nvidia,
    Amd,
    DirectX,
    Steam,
    AllShaders,
    TrimWorkingSet,
}

impl CleanOp {
    fn label(self) -> &'static str {
        match self {
            Self::Temp => "清理临时文件",
            Self::Nvidia => "NVIDIA 着色器缓存",
            Self::Amd => "AMD 着色器缓存",
            Self::DirectX => "DirectX 缓存",
            Self::Steam => "Steam 着色器缓存",
            Self::AllShaders => "一键清理全部着色器缓存",
            Self::TrimWorkingSet => "修剪工作集",
        }
    }

    fn run(self) -> CleanupResult {
        match self {
            Self::Temp => cleanup::clean_temp_files(),
            Self::Nvidia => cleanup::clean_nvidia_cache(),
            Self::Amd => cleanup::clean_amd_cache(),
            Self::DirectX => cleanup::clean_directx_cache(),
            Self::Steam => cleanup::clean_steam_cache(),
            Self::AllShaders => cleanup::clean_shader_cache(),
            Self::TrimWorkingSet => cleanup::trim_process_working_set(),
        }
    }
}

pub struct CleanupView {
    procs: Vec<ProcessInfo>,
    /// 最近一次清理结果（可追溯展示）
    last_result: Option<CleanupResult>,
    /// DNS 刷新等操作结果反馈
    status: SharedString,
    /// 清理是否执行中（防并发点击）
    cleaning: bool,
    /// 进程列表加载中
    loading_procs: bool,
    /// 搜索关键词
    keyword: SharedString,
    /// 进程搜索输入框（P2：keyword 历史无写入源，过滤从未接线）
    search_input: Entity<TextField>,
    /// 页面外观，随壳主题联动
    appearance: Appearance,
    /// 页面滚动状态（GPUI 0.2 滚轮需 track_scroll 手动驱动，见 ui::page::page_root）
    page_scroll: gpui::ScrollHandle,
}

/// 快捷操作语义（P2：历史 op_button 靠 label.contains("DNS") 字符串嗅探分发）
#[derive(Clone, Copy)]
enum QuickOp {
    FlushDns,
    RefreshProcs,
}

impl CleanupView {
    pub fn new(appearance: Appearance, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| TextField::new("", "搜索进程名 / PID", cx));
        cx.subscribe(
            &search_input,
            |this, field: Entity<TextField>, _ev: &ChangeText, cx| {
                this.set_keyword(field.read(cx).value(), cx);
            },
        )
        .detach();
        let mut v = Self {
            procs: Vec::new(),
            last_result: None,
            status: SharedString::from("正在加载进程列表…"),
            cleaning: false,
            loading_procs: false,
            keyword: SharedString::from(""),
            search_input,
            appearance,
            page_scroll: gpui::ScrollHandle::new(),
        };
        log::info!("清理优化 · 页面已打开");
        v.refresh_procs(cx);
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

    /// 进程搜索关键词更新（由搜索输入框 ChangeText 订阅驱动）
    fn set_keyword(&mut self, kw: SharedString, cx: &mut Context<Self>) {
        self.keyword = kw;
        cx.notify();
    }

    /// 后台枚举进程（Top 200，快照耗时；结果回填）
    fn refresh_procs(&mut self, cx: &mut Context<Self>) {
        if self.loading_procs {
            return;
        }
        self.loading_procs = true;
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let procs = exec
                .spawn(async move { cleanup::list_processes() })
                .await;
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.loading_procs = false;
                    this.procs = procs;
                    this.status = SharedString::from("");
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// DNS 刷新（系统 API，后台执行）
    fn flush_dns(&mut self, cx: &mut Context<Self>) {
        log::info!("清理优化 · 触发 DNS 刷新");
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let r = exec.spawn(async move { cleanup::flush_dns() }).await;
            // UI 侧日志：DNS 刷新完成/失败（CleanupResult 无 Err，按 success 判）
            if r.success {
                log::info!("清理优化 · DNS 刷新完成");
            } else {
                log::warn!("清理优化 · DNS 刷新失败: {}", r.message);
            }
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.status = SharedString::from(r.message.clone());
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 后台执行清理（文件 IO 可能耗时数秒）
    fn run_clean(&mut self, op: CleanOp, cx: &mut Context<Self>) {
        if self.cleaning {
            return;
        }
        self.cleaning = true;
        self.status = SharedString::from(format!("{}执行中…", op.label()));
        // UI 侧日志：用户触发清理动作（触发点）
        log::info!("清理优化 · 触发{}", op.label());
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let result = exec.spawn(async move { op.run() }).await;
            // UI 侧日志：清理结果（CleanupResult 非 Result，按 success/bytes 判）
            if result.success {
                log::info!("清理优化 · {}完成，释放 {} 字节", op.label(), result.bytes_freed);
            } else {
                log::warn!("清理优化 · {}未完全成功: {}", op.label(), result.message);
            }
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.cleaning = false;
                    this.last_result = Some(result);
                    this.status = SharedString::from("");
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 设置进程优先级（系统 API，后台执行）
    fn set_prio(&mut self, pid: u32, prio: &str, cx: &mut Context<Self>) {
        log::info!("清理优化 · 设置进程优先级 {} → {}", pid, prio);
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        let prio_c = prio.to_string();
        let prio_log = prio_c.clone();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let r = exec
                .spawn(async move { cleanup::set_process_priority(pid, &prio_c) })
                .await;
            // UI 侧日志：优先级设置结果
            if r.success {
                log::info!("清理优化 · 设置进程 {} 优先级为 {} 成功", pid, prio_log);
            } else {
                log::warn!("清理优化 · 设置进程 {} 优先级失败: {}", pid, r.message);
            }
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.status = SharedString::from(r.message.clone());
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn filtered(&self) -> Vec<&ProcessInfo> {
        if self.keyword.is_empty() {
            return self.procs.iter().take(50).collect();
        }
        let kw = self.keyword.to_lowercase();
        self.procs
            .iter()
            .filter(|p| p.name.to_lowercase().contains(&kw))
            .take(50)
            .collect()
    }

    fn fmt_bytes(b: u64) -> String {
        if b >= 1024 * 1024 * 1024 {
            format!("{:.2} GB", b as f64 / (1024.0 * 1024.0 * 1024.0))
        } else if b >= 1024 * 1024 {
            format!("{:.1} MB", b as f64 / (1024.0 * 1024.0))
        } else if b >= 1024 {
            format!("{:.0} KB", b as f64 / 1024.0)
        } else {
            format!("{} B", b)
        }
    }
}

impl Render for CleanupView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.pal();
        let procs = self.filtered();
        let status = self.status.clone();
        let cleaning = self.cleaning;
        let last_result = self.last_result.clone();
        // 响应式：主内容区过窄时左右两栏改为上下堆叠（自适应）
        let vw = f32::from(window.viewport_size().width);
        let side_by_side = vw >= 900.0;

        // 统一根容器：内边距/纵向节奏/超高滚动/页面底色（随主题联动）
        page_root(&pal, "cleanup-page-root", &self.page_scroll, &cx.entity())
            // 页头
            .child(page_header(
                &pal,
                "清理优化",
                "缓存清理 · 进程管理 · DNS 刷新",
            ))
            // 状态消息
            .when(!status.is_empty(), |s| {
                let msg = status.clone();
                s.child(banner(&pal, BannerKind::Info, msg))
            })
            // 主体：窗口宽时左右两栏（左=缓存清理+结果；右=快捷+进程）；
            // 窄窗（<900）时上下堆叠（响应式自适应）
            .child(
                div()
                    .flex()
                    .when(!side_by_side, |s| s.flex_col())
                    .items_start()
                    .gap_4()
                    // 左列（缓存清理）
                    .child(
                        div()
                            .flex_col()
                            .when(side_by_side, |s| s.flex_1().min_w(px(0.0)))
                            .gap_4()
                            .child(self.clean_card(&pal, cleaning, cx))
                            // 清理结果面板（追溯）
                            .when_some(last_result.clone(), |s, r| {
                                s.child(self.result_panel(&pal, &r))
                            }),
                    )
                    // 右列（快捷操作 + 进程管理）
                    .child(
                        div()
                            .flex_col()
                            .when(side_by_side, |s| s.flex_1().min_w(px(0.0)))
                            .gap_4()
                            .child(self.quick_card(&pal, cx))
                            .child(self.proc_card(&pal, &procs, cx)),
                    ),
            )
    }
}

impl CleanupView {
    /// 缓存清理卡片（统一卡框 + 强调头 + 分组子区 + 语义按钮）
    fn clean_card(&self, pal: &Palette, cleaning: bool, cx: &mut Context<Self>) -> impl IntoElement {
        card(pal)
            // 卡片头：accent 圆点 + 标题 + 右侧安全提示徽标
            .child(
                card_header_accent(pal, "缓存清理", pal.accent)
                    .child(badge(pal, "安全清理 · 重启后删占用文件", pal.success)),
            )
            .child(card_divider(pal))
            // 说明行
            .child(
                div()
                    .px(px(CARD_PADDING))
                    .pt_3()
                    .text_size(px(11.5))
                    .text_color(pal.text_muted)
                    .child("清理系统临时文件与显卡厂商着色器缓存，释放磁盘空间。"),
            )
            // 分组：系统临时
            .child(
                div()
                    .px(px(CARD_PADDING))
                    .pt_4()
                    .child(section_title(pal, "系统临时")),
            )
            .child(
                div()
                    .px(px(CARD_PADDING))
                    .pt_2p5()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.clean_button(pal, CleanOp::Temp, cleaning, cx)),
            )
            // 分组：显卡着色器缓存
            .child(
                div()
                    .px(px(CARD_PADDING))
                    .pt_4()
                    .child(section_title(pal, "显卡着色器缓存")),
            )
            .child(
                div()
                    .px(px(CARD_PADDING))
                    .pt_2p5()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.clean_button(pal, CleanOp::Nvidia, cleaning, cx))
                    .child(self.clean_button(pal, CleanOp::Amd, cleaning, cx))
                    .child(self.clean_button(pal, CleanOp::DirectX, cleaning, cx))
                    .child(self.clean_button(pal, CleanOp::Steam, cleaning, cx)),
            )
            // 底部操作条：一键清理（主）+ 修剪工作集（危险）
            .child(
                div()
                    .mt_4()
                    .px(px(CARD_PADDING))
                    .py_4()
                    .border_t_1()
                    .border_color(pal.border)
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.clean_button(pal, CleanOp::AllShaders, cleaning, cx))
                    .child(div().flex_1())
                    .child(self.clean_button(pal, CleanOp::TrimWorkingSet, cleaning, cx)),
            )
    }

    /// 清理按钮（语义映射：一键=Primary、修剪工作集=Danger、其余=Secondary；
    /// 执行中统一 Ghost 弱化，禁点逻辑保留）
    fn clean_button(
        &self,
        pal: &Palette,
        op: CleanOp,
        cleaning: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = op.label().to_string();
        let kind = if cleaning {
            ButtonKind::Ghost
        } else if matches!(op, CleanOp::AllShaders) {
            ButtonKind::Primary
        } else if matches!(op, CleanOp::TrimWorkingSet) {
            ButtonKind::Danger
        } else {
            ButtonKind::Secondary
        };
        button(pal, kind)
            .id(SharedString::from(format!("clean-{:?}", op)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !cleaning {
                    this.run_clean(op, cx);
                }
            }))
            .child(label)
    }

    /// 结果面板（可追溯：操作名/是否成功/释放字节/消息明细）
    fn result_panel(&self, pal: &Palette, r: &CleanupResult) -> impl IntoElement {
        let op = r.operation.clone();
        let ok = r.success;
        let bytes = r.bytes_freed;
        let msg = r.message.clone();
        // 成功=语义绿、部分完成=警示黄（圆点与摘要文本同色）
        let state_color = if ok { pal.success } else { pal.warning };
        card(pal)
            // 卡片头：状态色圆点 + 操作名 + 右侧结果摘要
            .child(
                card_header_accent(pal, op.clone(), state_color).child(
                    div()
                        .text_size(px(12.0))
                        .text_color(state_color)
                        .child(SharedString::from(format!(
                            "{} · 释放 {}",
                            if ok { "成功" } else { "部分完成" },
                            Self::fmt_bytes(bytes)
                        ))),
                ),
            )
            // 消息明细区（顶部分隔线保留）
            .when(!msg.is_empty(), |s| {
                s.child(
                    div()
                        .px(px(CARD_PADDING))
                        .py_2()
                        .border_t_1()
                        .border_color(pal.border)
                        .text_size(px(11.5))
                        .text_color(pal.text_muted)
                        .child(msg),
                )
            })
    }

    /// 快捷操作卡（DNS 刷新 / 进程列表刷新 / 进程搜索输入）
    fn quick_card(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        card(pal)
            .child(card_header_accent(pal, "快捷操作", pal.success))
            .child(card_divider(pal))
            .child(
                card_body(pal).child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(self.op_button(pal, "刷新 DNS 缓存", QuickOp::FlushDns, cx))
                        .child(self.op_button(pal, "刷新进程列表", QuickOp::RefreshProcs, cx))
                        // 搜索输入容器（宽度固定，实体与订阅不变）
                        .child(div().w(px(220.0)).child(self.search_input.clone())),
                ),
            )
    }

    /// 快捷操作按钮（统一 Primary；id 沿用操作文案）
    fn op_button(
        &self,
        pal: &Palette,
        label: &str,
        op: QuickOp,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label_owned = label.to_string();
        button(pal, ButtonKind::Primary)
            .id(SharedString::from(label_owned.clone()))
            .on_click(cx.listener(move |this, _, _, cx| match op {
                QuickOp::FlushDns => this.flush_dns(cx),
                QuickOp::RefreshProcs => this.refresh_procs(cx),
            }))
            .child(label_owned)
    }

    /// 进程管理卡（滚动列表 + 规格化表头/行）
    fn proc_card(
        &self,
        pal: &Palette,
        procs: &[&ProcessInfo],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        card(pal)
            // 卡片头：accent 圆点 + 标题 + 右侧进程计数
            .child(
                card_header_accent(pal, "进程管理", pal.accent).child(
                    div()
                        .text_size(px(11.5))
                        .text_color(pal.text_muted)
                        .child(SharedString::from(format!("{} 个进程", procs.len()))),
                ),
            )
            .child(card_divider(pal))
            // 滚动容器（高度与 id 保留）
            .child(
                div()
                    .id("proc-scroll")
                    .flex_col()
                    .h(px(380.0))
                    .overflow_scroll()
                    .child(table_head(
                        pal,
                        &[
                            ("PID", ColWidth::Px(70.0)),
                            ("进程名", ColWidth::Flex),
                            ("内存", ColWidth::Px(90.0)),
                            ("优先级", ColWidth::Px(200.0)),
                        ],
                    ))
                    .children(procs.iter().map(|p| self.proc_row(pal, p, cx))),
            )
    }

    /// 进程行（列宽与表头规格一致：PID 70 / 名称自适应 / 内存 90 / 优先级 200）
    fn proc_row(&self, pal: &Palette, p: &ProcessInfo, cx: &mut Context<Self>) -> impl IntoElement {
        let pid = p.pid;
        let name = p.name.clone();
        let mem = p.memory_mb;
        let mem_disp = if mem >= 1024.0 {
            format!("{:.1} GB", mem / 1024.0)
        } else {
            format!("{:.0} MB", mem)
        };

        table_row(pal)
            .id(SharedString::from(format!("proc-{}", pid)))
            // PID 列
            .child(
                div()
                    .flex_none()
                    .w(px(70.0))
                    .text_size(px(12.0))
                    .text_color(pal.text_muted)
                    .child(pid.to_string()),
            )
            // 进程名列
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(12.5))
                    .text_color(pal.text)
                    .child(name.clone()),
            )
            // 内存列
            .child(
                div()
                    .flex_none()
                    .w(px(90.0))
                    .text_size(px(12.0))
                    .text_color(pal.text_muted)
                    .child(mem_disp),
            )
            // 优先级列
            .child(
                div()
                    .flex_none()
                    .w(px(200.0))
                    .flex()
                    .gap_1()
                    .child(prio_button(pal, "低", "idle", pid, cx))
                    .child(prio_button(pal, "标准", "normal", pid, cx))
                    .child(prio_button(pal, "高", "high", pid, cx)),
            )
    }
}

/// 优先级按钮（行内小按钮，统一 Secondary；id 保留）
fn prio_button(
    pal: &Palette,
    label: &str,
    prio: &'static str,
    pid: u32,
    cx: &mut Context<CleanupView>,
) -> impl IntoElement {
    let label_owned = label.to_string();
    let id = SharedString::from(format!("prio-{}-{}", pid, prio));
    button_sm(pal, ButtonKind::Secondary)
        .id(id)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_prio(pid, prio, cx);
        }))
        .child(label_owned)
}
