// secm-app::pages::cleanup — 清理优化页
// 缓存清理（临时/着色器/NVIDIA/AMD/DirectX/Steam）+ 进程管理（Top 200 + 优先级）+ DNS 刷新。
// 清理为文件 IO，放后台线程执行避免卡 UI。

use gpui::prelude::*;
use gpui::{div, px, rgb, Entity, SharedString, Window, Context, Render, WeakEntity};
use secm_core::cleanup::{self, CleanupResult, ProcessInfo};

use crate::theme::Theme;
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
}

/// 快捷操作语义（P2：历史 op_button 靠 label.contains("DNS") 字符串嗅探分发）
#[derive(Clone, Copy)]
enum QuickOp {
    FlushDns,
    RefreshProcs,
}

impl CleanupView {
    pub fn new(cx: &mut Context<Self>) -> Self {
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
        };
        log::info!("清理优化 · 页面已打开");
        v.refresh_procs(cx);
        v
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
        let theme = Theme::dark();
        let procs = self.filtered();
        let status = self.status.clone();
        let cleaning = self.cleaning;
        let last_result = self.last_result.clone();
        // 响应式：主内容区过窄时左右两栏改为上下堆叠（自适应）
        let vw = f32::from(window.viewport_size().width);
        let side_by_side = vw >= 900.0;

        div()
            .id("cleanup-page-root")
            .flex_col()
            .size_full()
            .p_6()
            .gap_4()
            // 内容超高时整页纵向滚动
            .overflow_y_scroll()
            // 页头
            .child(crate::ui::page_header(
                &theme,
                "清理优化",
                "缓存清理 · 进程管理 · DNS 刷新",
            ))
            // 状态消息
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
                            .child(self.clean_card(&theme, cleaning, cx))
                            // 清理结果面板（追溯）
                            .when_some(last_result.clone(), |s, r| {
                                s.child(self.result_panel(&theme, &r))
                            }),
                    )
                    // 右列（快捷操作 + 进程管理）
                    .child(
                        div()
                            .flex_col()
                            .when(side_by_side, |s| s.flex_1().min_w(px(0.0)))
                            .gap_4()
                            // 快捷操作（现代化卡片）
                            .child(
                                div()
                                    .flex_col()
                                    .rounded(px(12.0))
                                    .border_1()
                                    .border_color(theme.border)
                                    .bg(theme.panel)
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2p5()
                                            .px_5()
                                            .py_3()
                                            .child(div().size(px(6.0)).rounded_full().bg(theme.success))
                                            .child(
                                                div()
                                                    .text_size(px(15.0))
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .text_color(theme.text)
                                                    .child("快捷操作"),
                                            ),
                                    )
                                    .child(div().h(px(1.0)).w_full().bg(theme.border))
                                    .child(
                                        div()
                                            .px_5()
                                            .py_4()
                                            .flex()
                                            .flex_wrap()
                                            .gap_2()
                                            .child(self.op_button(&theme, "刷新 DNS 缓存", QuickOp::FlushDns, cx))
                                            .child(self.op_button(&theme, "刷新进程列表", QuickOp::RefreshProcs, cx))
                                            .child(
                                                div()
                                                    .w(px(220.0))
                                                    .child(self.search_input.clone()),
                                            ),
                                    ),
                            )
                            // 进程表（现代化容器 + 搜索过滤）
                            .child(
                                div()
                                    .flex_col()
                                    .rounded(px(12.0))
                                    .border_1()
                                    .border_color(theme.border)
                                    .bg(theme.panel)
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2p5()
                                            .px_5()
                                            .py_3()
                                            .child(div().size(px(6.0)).rounded_full().bg(theme.info))
                                            .child(
                                                div()
                                                    .text_size(px(15.0))
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .text_color(theme.text)
                                                    .child("进程管理"),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .flex()
                                                    .justify_end()
                                                    .child(
                                                        div()
                                                            .text_size(px(11.5))
                                                            .text_color(theme.text_muted)
                                                            .child(SharedString::from(format!(
                                                                "{} 个进程",
                                                                procs.len()
                                                            ))),
                                                    ),
                                            ),
                                    )
                                    .child(div().h(px(1.0)).w_full().bg(theme.border))
                                    .child(
                                        div()
                                            .id("proc-scroll")
                                            .flex_col()
                                            .h(px(380.0))
                                            .overflow_scroll()
                                            .child(crate::ui::table_head(&theme, &["PID", "进程名", "内存", "优先级"]))
                                            .children(procs.iter().map(|p| self.proc_row(&theme, p, cx))),
                                    ),
                            ),
                    ),
            )
    }
}

impl CleanupView {
    /// 缓存清理卡片（现代化：强调标题条 + 分组子区 + 语义色按钮）
    fn clean_card(&self, theme: &Theme, cleaning: bool, cx: &mut Context<Self>) -> impl IntoElement {
        // 标题行：强调色短条 + 标题 + 右侧清理说明
        div()
            .flex_col()
            .rounded(px(12.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.panel)
            .overflow_hidden()
            // 卡片头：渐变强调
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .px_5()
                    .py_3()
                    .child(div().size(px(6.0)).rounded_full().bg(theme.brand))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child("缓存清理"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .justify_end()
                            .flex()
                            .child(
                                div()
                                    .px_2p5()
                                    .py_1()
                                    .rounded_full()
                                    .text_size(px(11.0))
                                    .text_color(theme.success)
                                    .border_1()
                                    .border_color(theme.success)
                                    .child("安全清理 · 重启后删占用文件"),
                            ),
                    ),
            )
            .child(
                div()
                    .h(px(1.0))
                    .w_full()
                    .bg(theme.border),
            )
            // 说明行
            .child(
                div()
                    .px_5()
                    .pt_3()
                    .text_size(px(11.5))
                    .text_color(theme.text_muted)
                    .child("清理系统临时文件与显卡厂商着色器缓存，释放磁盘空间。"),
            )
            // 分组：系统缓存
            .child(
                div()
                    .px_5()
                    .pt_4()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.text_muted)
                            .child("系统临时"),
                    ),
            )
            .child(
                div()
                    .px_5()
                    .pt_2p5()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.clean_button(theme, CleanOp::Temp, cleaning, cx)),
            )
            // 分组：显卡着色器
            .child(
                div()
                    .px_5()
                    .pt_4()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.text_muted)
                            .child("显卡着色器缓存"),
                    ),
            )
            .child(
                div()
                    .px_5()
                    .pt_2p5()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.clean_button(theme, CleanOp::Nvidia, cleaning, cx))
                    .child(self.clean_button(theme, CleanOp::Amd, cleaning, cx))
                    .child(self.clean_button(theme, CleanOp::DirectX, cleaning, cx))
                    .child(self.clean_button(theme, CleanOp::Steam, cleaning, cx)),
            )
            // 底部操作条：一键清理（主）+ 修剪工作集（危险）
            .child(
                div()
                    .mt_4()
                    .px_5()
                    .py_4()
                    .border_t_1()
                    .border_color(theme.border)
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.clean_button(theme, CleanOp::AllShaders, cleaning, cx))
                    .child(
                        div()
                            .flex_1(),
                    )
                    .child(self.clean_button(theme, CleanOp::TrimWorkingSet, cleaning, cx)),
            )
    }

    /// 清理按钮（现代化：普通=中性、危险=红调、一键=品牌主色；执行中禁用）
    fn clean_button(
        &self,
        theme: &Theme,
        op: CleanOp,
        cleaning: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = op.label().to_string();
        let danger = matches!(op, CleanOp::TrimWorkingSet);
        let all = matches!(op, CleanOp::AllShaders);
        // 三态配色（禁用时统一弱化）
        let (bg, hover, fg) = if cleaning {
            (theme.panel_hover, theme.panel_hover, theme.text_muted)
        } else if all {
            (theme.brand, rgb(0x3d66e6), rgb(0xffffff))
        } else if danger {
            (rgb(0x7f1d1d), rgb(0x991b1b), rgb(0xffffff))
        } else {
            (theme.panel_hover, theme.border, theme.text)
        };
        div()
            .id(SharedString::from(format!("clean-{:?}", op)))
            .flex()
            .items_center()
            .gap_1p5()
            .px_3p5()
            .h(px(30.0))
            .rounded(px(8.0))
            .cursor_pointer()
            .text_size(px(12.5))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(fg)
            .bg(bg)
            .hover(|s| s.bg(hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !cleaning {
                    this.run_clean(op, cx);
                }
            }))
            .child(label)
    }

    /// 结果面板（可追溯：操作名/是否成功/释放字节/消息明细）
    fn result_panel(&self, theme: &Theme, r: &CleanupResult) -> impl IntoElement {
        let op = r.operation.clone();
        let ok = r.success;
        let bytes = r.bytes_freed;
        let msg = r.message.clone();
        div()
            .flex_col()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.panel)
            .child(
                div()
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(op),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(if ok { theme.success } else { theme.warn })
                            .child(SharedString::from(format!(
                                "{} · 释放 {}",
                                if ok { "成功" } else { "部分完成" },
                                Self::fmt_bytes(bytes)
                            ))),
                    ),
            )
            .when(!msg.is_empty(), |s| {
                s.child(
                    div()
                        .px_4()
                        .py_2()
                        .border_t_1()
                        .border_color(theme.border)
                        .text_size(px(11.5))
                        .text_color(theme.text_muted)
                        .child(msg),
                )
            })
    }

    fn op_button(
        &self,
        theme: &Theme,
        label: &str,
        op: QuickOp,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label_owned = label.to_string();
        div()
            .id(SharedString::from(label_owned.clone()))
            .px_4()
            .py_1p5()
            .rounded_md()
            .cursor_pointer()
            .bg(theme.brand)
            .hover(|s| s.bg(rgb(0x3d66e6)))
            .text_color(rgb(0xffffff))
            .text_size(px(12.5))
            .on_click(cx.listener(move |this, _, _, cx| match op {
                QuickOp::FlushDns => this.flush_dns(cx),
                QuickOp::RefreshProcs => this.refresh_procs(cx),
            }))
            .child(label_owned)
    }

    fn proc_row(
        &self,
        theme: &Theme,
        p: &ProcessInfo,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pid = p.pid;
        let name = p.name.clone();
        let mem = p.memory_mb;
        let mem_disp = if mem >= 1024.0 {
            format!("{:.1} GB", mem / 1024.0)
        } else {
            format!("{:.0} MB", mem)
        };

        div()
            .id(SharedString::from(format!("proc-{}", pid)))
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .py_1p5()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .w(px(60.0))
                    .flex_none()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child(pid.to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.5))
                    .text_color(theme.text)
                    .child(name.clone()),
            )
            .child(
                div()
                    .w(px(80.0))
                    .flex_none()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child(mem_disp),
            )
            .child(
                div()
                    .w(px(210.0))
                    .flex_none()
                    .flex()
                    .gap_1()
                    .child(prio_button(theme, "低", "idle", pid, cx))
                    .child(prio_button(theme, "标准", "normal", pid, cx))
                    .child(prio_button(theme, "高", "high", pid, cx)),
            )
    }
}

fn prio_button(
    theme: &Theme,
    label: &str,
    prio: &'static str,
    pid: u32,
    cx: &mut Context<CleanupView>,
) -> impl IntoElement {
    let label_owned = label.to_string();
    let id = SharedString::from(format!("prio-{}-{}", pid, prio));
    div()
        .id(id)
        .px_2()
        .py_0p5()
        .rounded_sm()
        .cursor_pointer()
        .bg(theme.panel_hover)
        .hover(|s| s.bg(theme.border))
        .text_size(px(11.0))
        .text_color(theme.text)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_prio(pid, prio, cx);
        }))
        .child(label_owned)
}
