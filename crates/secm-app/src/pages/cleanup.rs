// secm-app::pages::cleanup — 清理优化页（对齐上游 /cleanup Cleanup.tsx 全能力面）
// 缓存清理（临时/着色器 4 厂商/一键全清）+ 进程管理（Top 200 uniform_list 增量渲染
// + 名称/PID 搜索 + 6 档优先级）+ DNS 刷新。
// 执行结果追溯（用户指令 v3.2.1）：页内追溯卡已拆除——全部清理/快捷操作的执行与
// 结果逐行流式记录到右侧日志流面板（record_result → log::），右侧栏为唯一追溯出口。
// 清理为文件 IO，放后台线程执行避免卡 UI。
// 呈现层统一由 crate::ui::page 装配，色板取自 Palette（明暗随壳主题联动）。

use gpui::prelude::*;
use gpui::{
    div, px, uniform_list, Context, Entity, Render, ScrollHandle, SharedString,
    UniformListScrollHandle, WeakEntity, Window,
};
use secm_core::cleanup::{self, CleanupResult, ProcessInfo};

use crate::pi_clone::theme::{Appearance, Palette};
use crate::ui::page::{
    badge, banner, button, button_sm, card, card_divider, card_header_accent, page_header,
    page_root, section_title, table_empty, table_head, table_mid_cell, table_row, BannerKind,
    ButtonKind, ColWidth, CARD_PADDING,
};
use crate::ui::text_input::{ChangeText, TextField};
use crate::ui::toast;

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
            Self::Nvidia => "清 NVIDIA 缓存",
            Self::Amd => "清 AMD 缓存",
            Self::DirectX => "清 DirectX",
            Self::Steam => "清 Steam 缓存",
            Self::AllShaders => "全清着色器",
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
    /// DNS 刷新等操作结果反馈
    status: SharedString,
    /// 清理是否执行中（防并发点击）
    cleaning: bool,
    /// 进程列表加载中
    loading_procs: bool,
    /// 搜索关键词（名称/PID 双通道过滤）
    keyword: SharedString,
    /// 进程搜索输入框
    search_input: Entity<TextField>,
    /// 进程列表虚拟滚动句柄（uniform_list 仅渲染可见行，Top 200 无全量布局开销）
    proc_list_scroll: UniformListScrollHandle,
    /// 页面外观，随壳主题联动
    appearance: Appearance,
    /// 页面滚动状态（GPUI 0.2 滚轮需 track_scroll 手动驱动，见 ui::page::page_root）
    page_scroll: gpui::ScrollHandle,
}

/// 快捷操作语义（按钮 → 后台执行函数映射；按钮分置缓存清理卡/进程管理卡）
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
            status: SharedString::from("正在加载进程列表…"),
            cleaning: false,
            loading_procs: false,
            keyword: SharedString::from(""),
            search_input,
            proc_list_scroll: UniformListScrollHandle::default(),
            appearance,
            page_scroll: ScrollHandle::new(),
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
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                let procs = exec.spawn(async move { cleanup::list_processes() }).await;
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.loading_procs = false;
                        this.procs = procs;
                        this.status = SharedString::from("");
                        cx.notify();
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    /// 记录执行结果（追溯唯一出口：全部逐行流式写入右侧日志流面板；
    /// 摘要行含操作/结论/释放量，明细行失败/重启删除以 Warn 呈现）
    fn record_result(&mut self, result: CleanupResult, cx: &mut Context<Self>) {
        let time = secm_core::logger::now_hms();
        let verdict = if result.success {
            "成功"
        } else {
            "部分完成"
        };
        // 全局泡泡提示：完成摘要随屏可见（日志流仅作追溯）
        let toast_msg = format!(
            "{}{} · 释放 {}",
            result.operation,
            if result.success {
                "完成"
            } else {
                "部分完成"
            },
            Self::fmt_bytes(result.bytes_freed)
        );
        if result.success {
            toast::success(toast_msg, cx);
        } else {
            toast::warning(toast_msg, cx);
        }
        // 完成摘要（日志流可检索的锚点行）
        log::info!(
            "清理优化 · {} · {} · 释放 {}",
            result.operation,
            verdict,
            Self::fmt_bytes(result.bytes_freed)
        );
        // 明细逐行（一行一条日志）
        for line in result.message.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let degraded = line.contains("失败")
                || line.contains("错误")
                || line.contains("无法")
                || line.contains("标记重启后删除");
            if degraded {
                log::warn!("清理优化 · [{}] {} · {}", time, result.operation, line);
            } else {
                log::info!("清理优化 · [{}] {} · {}", time, result.operation, line);
            }
        }
    }

    /// DNS 刷新（系统 API，后台执行）
    fn flush_dns(&mut self, cx: &mut Context<Self>) {
        log::info!("清理优化 · 触发 DNS 刷新");
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                let r = exec.spawn(async move { cleanup::flush_dns() }).await;
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.record_result(r, cx);
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    /// 后台执行清理（文件 IO 可能耗时数秒）
    fn run_clean(&mut self, op: CleanOp, cx: &mut Context<Self>) {
        if self.cleaning {
            return;
        }
        self.cleaning = true;
        self.status = SharedString::from(format!("{}执行中…", op.label()));
        // 触发点日志（右侧日志流可追溯用户动作）
        log::info!("清理优化 · 触发{}", op.label());
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                let result = exec.spawn(async move { op.run() }).await;
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.cleaning = false;
                        this.status = SharedString::from("");
                        this.record_result(result, cx);
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    /// 设置进程优先级（系统 API，后台执行）
    fn set_prio(&mut self, pid: u32, prio: &'static str, cx: &mut Context<Self>) {
        log::info!("清理优化 · 设置进程优先级 {} → {}", pid, prio);
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                let r = exec
                    .spawn(async move { cleanup::set_process_priority(pid, prio) })
                    .await;
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.record_result(r, cx);
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    /// 过滤后的进程（名称/PID 双通道匹配；后端已按内存 Top 200 排序）
    fn filtered(&self) -> Vec<ProcessInfo> {
        let kw = self.keyword.trim();
        self.procs
            .iter()
            .filter(|p| cleanup::process_matches(p, kw))
            .cloned()
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.pal();
        let filtered = self.filtered();
        let status = self.status.clone();
        let cleaning = self.cleaning;

        // 统一根容器：内边距/纵向节奏/超高滚动/页面底色（随主题联动）
        page_root(&pal, "cleanup-page-root", &self.page_scroll, &cx.entity())
            // 页头
            .child(page_header(
                &pal,
                "清理优化",
                "缓存清理 · 进程管理 · DNS 刷新 · 执行结果全量记录于右侧日志流",
            ))
            // 状态消息
            .when(!status.is_empty(), |s| {
                let msg = status.clone();
                s.child(banner(&pal, BannerKind::Info, msg))
            })
            // 上下两行全宽排版（用户指令：缓存清理在上、进程管理在下，不并排）；
            // 两卡各自全宽，纵向节奏由卡片自带 .mb(PAGE_GAP) 承担
            .child(self.clean_card(&pal, cleaning, cx))
            // 进程管理（刷新 + 搜索并入卡容器，表格全宽自适应进程名）
            .child(self.proc_card(&pal, &filtered, cx))
    }
}

impl CleanupView {
    /// 缓存清理卡片（统一卡框 + 强调头 + 分组子区 + 语义按钮）
    fn clean_card(
        &self,
        pal: &Palette,
        cleaning: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        card(pal)
            // 卡片头：accent 圆点 + 标题 + 右侧安全提示徽标
            .child(card_header_accent(pal, "缓存清理", pal.accent).child(badge(
                pal,
                "安全清理 · 重启后删占用文件",
                pal.success,
            )))
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
            // 分组：系统临时（清理临时文件 + DNS 缓存刷新，用户指令：快捷操作按钮并入此行）
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
                    .child(self.clean_button(pal, CleanOp::Temp, cleaning, cx))
                    .child(self.op_button(pal, "刷新 DNS", QuickOp::FlushDns, cx)),
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

    /// 快捷操作按钮（统一 Primary；id 沿用操作文案；现分置于缓存清理卡/进程管理卡）
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

    /// 进程管理卡（uniform_list 增量渲染 Top 200 + 名称/PID 搜索 + 6 档优先级行内设置；
    /// 刷新按钮与搜索输入并入卡容器，标题右侧小字描述：进程的 CPU 优先级）
    fn proc_card(
        &self,
        pal: &Palette,
        filtered: &[ProcessInfo],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let loading = self.procs.is_empty();
        card(pal)
            // 卡片头：accent 圆点 + 标题 + 右侧小字描述（进程的CPU优先级）与计数
            .child(
                card_header_accent(pal, "进程管理", pal.accent).child(
                    div()
                        .text_size(px(11.5))
                        .text_color(pal.text_muted)
                        .child(SharedString::from(format!(
                            "进程的CPU优先级 · 共 {} 个 · 显示 {} 个",
                            self.procs.len(),
                            filtered.len()
                        ))),
                ),
            )
            .child(card_divider(pal))
            // 工具栏（并入卡容器）：刷新进程列表 + 搜索输入（弹性占满）
            .child(
                div()
                    .px(px(CARD_PADDING))
                    .py(px(8.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.op_button(pal, "刷新进程", QuickOp::RefreshProcs, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(self.search_input.clone()),
                    ),
            )
            // 表头固定于滚动区外（Top 200 滚动时表头不随行滚走）；
            // PID/进程名/内存三列居中（Mid/FlexMid=水平居中），与数据行同规格居中对齐
            .child(table_head(
                pal,
                &[
                    ("PID", ColWidth::Mid(70.0)),
                    ("进程名", ColWidth::FlexMid),
                    ("内存", ColWidth::Mid(90.0)),
                    ("优先级", ColWidth::Px(252.0)),
                ],
            ))
            .when(filtered.is_empty(), |s| {
                s.child(table_empty(
                    pal,
                    if loading {
                        "正在加载进程列表…"
                    } else {
                        "无匹配进程，请调整搜索条件"
                    },
                ))
            })
            .when(!filtered.is_empty(), move |s| {
                // uniform_list：仅可见区间行进入元素树（Top 200 无全量布局开销）。
                // 行构建经 view.read 只读状态；点击处理器捕获 WeakEntity 直达，
                // 避免布局期对实体做 update。
                let weak: WeakEntity<Self> = cx.entity().downgrade();
                let items: Vec<ProcessInfo> = filtered.to_vec();
                let pal_cp = *pal;
                let proc_scroll = self.proc_list_scroll.clone();
                s.child(
                    uniform_list("proc-list", items.len(), move |range, _window, cx| {
                        let mut rows: Vec<gpui::AnyElement> =
                            Vec::with_capacity(range.end - range.start);
                        if let Some(view) = weak.upgrade() {
                            let this = view.read(cx);
                            for i in range.clone() {
                                let Some(p) = items.get(i) else {
                                    continue;
                                };
                                rows.push(
                                    this.proc_row(&pal_cp, p, weak.clone()).into_any_element(),
                                );
                            }
                        }
                        rows
                    })
                    .h(px(380.0))
                    .track_scroll(proc_scroll),
                )
            })
    }

    /// 进程行（列宽与表头规格一致：PID 70(居中) / 名称自适应(居中) / 内存 90(居中) /
    /// 优先级 6 档 252；固定行高 44 保证 uniform_list 等高语义）
    fn proc_row(&self, pal: &Palette, p: &ProcessInfo, weak: WeakEntity<Self>) -> impl IntoElement {
        let pid = p.pid;
        let name = p.name.clone();
        let mem = p.memory_mb;
        let mem_disp = if mem >= 1024.0 {
            format!("{:.1} GB", mem / 1024.0)
        } else {
            format!("{:.0} MB", mem)
        };

        // 6 档优先级行内按钮（档位来自 secm_core::cleanup::PRIORITY_LEVELS 单一真源；
        // 点击经 WeakEntity 直达 set_prio，无需布局期实体 update）
        let prio_cells: Vec<gpui::AnyElement> = cleanup::PRIORITY_LEVELS
            .iter()
            .map(|(prio_id, label)| {
                let prio_id = *prio_id;
                // 每个按钮的点击闭包独立持有句柄（Fn 多次构建需逐个克隆）
                let weak = weak.clone();
                button_sm(pal, ButtonKind::Secondary)
                    .id(SharedString::from(format!("prio-{}-{}", pid, prio_id)))
                    .on_click(move |_ev, _w, cx| {
                        let _ = weak.update(cx, |this, cx| this.set_prio(pid, prio_id, cx));
                    })
                    .child(*label)
                    .into_any_element()
            })
            .collect();

        table_row(pal)
            .h(px(44.0))
            .id(SharedString::from(format!("proc-{}", pid)))
            // PID 列（居中，与表头同一规格）
            .child(
                table_mid_cell(70.0)
                    .text_size(px(12.0))
                    .text_color(pal.text_muted)
                    .child(pid.to_string()),
            )
            // 进程名列（自适应剩余全宽；居中 + 单行省略防异常换行/溢出 —— 行高固定 44
            // 由 uniform_list 等高语义决定，名称过长时优雅省略不折行）
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .whitespace_nowrap()
                    .truncate()
                    .text_center()
                    .text_size(px(12.5))
                    .text_color(pal.text)
                    .child(name),
            )
            // 内存列（居中，与表头同一规格）
            .child(
                table_mid_cell(90.0)
                    .text_size(px(12.0))
                    .text_color(pal.text_muted)
                    .child(mem_disp),
            )
            // 优先级列（6 档）
            .child(
                div()
                    .flex_none()
                    .w(px(252.0))
                    .flex()
                    .gap_1()
                    .children(prio_cells),
            )
    }
}
