// secm-app::pages::cleanup — 清理优化页
// 缓存清理（临时/着色器/NVIDIA/AMD/DirectX/Steam）+ 进程管理（Top 200 + 优先级）+ DNS 刷新。
// 清理为文件 IO，放后台线程执行避免卡 UI。

use gpui::prelude::*;
use gpui::{div, px, rgb, SharedString, Window, Context, Render, WeakEntity};
use secm_core::cleanup::{self, CleanupResult, ProcessInfo};

use crate::theme::Theme;

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
    /// 搜索关键词
    keyword: SharedString,
}

impl CleanupView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut v = Self {
            procs: cleanup::list_processes(),
            last_result: None,
            status: SharedString::from(""),
            cleaning: false,
            keyword: SharedString::from(""),
        };
        v.refresh_procs(cx);
        v
    }

    fn refresh_procs(&mut self, cx: &mut Context<Self>) {
        self.procs = cleanup::list_processes();
        cx.notify();
    }

    fn flush_dns(&mut self, cx: &mut Context<Self>) {
        let r = cleanup::flush_dns();
        self.status = SharedString::from(if r.success {
            r.message.clone()
        } else {
            r.message.clone()
        });
        cx.notify();
    }

    /// 后台执行清理（文件 IO 可能耗时数秒）
    fn run_clean(&mut self, op: CleanOp, cx: &mut Context<Self>) {
        if self.cleaning {
            return;
        }
        self.cleaning = true;
        self.status = SharedString::from(format!("{}执行中…", op.label()));
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let result = exec.spawn(async move { op.run() }).await;
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

    fn set_prio(&mut self, pid: u32, prio: &str, cx: &mut Context<Self>) {
        let r = cleanup::set_process_priority(pid, prio);
        self.status = SharedString::from(if r.success {
            r.message.clone()
        } else {
            r.message.clone()
        });
        cx.notify();
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::dark();
        let procs = self.filtered();
        let status = self.status.clone();
        let cleaning = self.cleaning;
        let last_result = self.last_result.clone();

        div()
            .flex_col()
            .size_full()
            .p_6()
            .gap_4()
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
            // 缓存清理区
            .child(self.clean_card(&theme, cleaning, cx))
            // 清理结果面板（追溯）
            .when_some(last_result, |s, r| s.child(self.result_panel(&theme, &r)))
            // 快捷操作
            .child(
                div()
                    .flex_col()
                    .p_4()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.panel)
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child("快捷操作"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.op_button(&theme, "刷新 DNS 缓存", false, cx))
                            .child(self.op_button(&theme, "刷新进程列表", false, cx)),
                    ),
            )
            // 进程表
            .child(
                crate::ui::table_container(&theme).child(
                    div()
                        .id("proc-scroll")
                        .flex_col()
                        .h(px(380.0))
                        .overflow_scroll()
                        .child(crate::ui::table_head(&theme, &["PID", "进程名", "内存", "优先级"]))
                        .children(procs.iter().map(|p| self.proc_row(&theme, p, cx))),
                ),
            )
    }
}

impl CleanupView {
    /// 缓存清理卡片（厂商/系统缓存按钮组）
    fn clean_card(&self, theme: &Theme, cleaning: bool, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_col()
            .p_4()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.panel)
            .gap_3()
            .child(
                div()
                    .text_size(px(14.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child("缓存清理"),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme.text_muted)
                    .child("清理临时文件与各厂商着色器缓存；被占用文件将标记为重启后自动删除。"),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.clean_button(theme, CleanOp::Temp, cleaning, cx))
                    .child(self.clean_button(theme, CleanOp::Nvidia, cleaning, cx))
                    .child(self.clean_button(theme, CleanOp::Amd, cleaning, cx))
                    .child(self.clean_button(theme, CleanOp::DirectX, cleaning, cx))
                    .child(self.clean_button(theme, CleanOp::Steam, cleaning, cx)),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(self.clean_button(theme, CleanOp::AllShaders, cleaning, cx))
                    .child(self.clean_button(theme, CleanOp::TrimWorkingSet, cleaning, cx)),
            )
    }

    /// 清理按钮（背景按操作区分；执行中禁用）
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
        div()
            .id(SharedString::from(format!("clean-{:?}", op)))
            .px_3()
            .py_1p5()
            .rounded_md()
            .cursor_pointer()
            .when(!cleaning, |s| {
                s.when(all, |s| s.bg(theme.brand).hover(|s| s.bg(rgb(0x3d66e6))).text_color(rgb(0xffffff)))
                    .when(!all && danger, |s| s.bg(theme.panel_hover).hover(|s| s.bg(rgb(0x7f1d1d))).text_color(theme.text))
                    .when(!all && !danger, |s| s.bg(theme.panel_hover).hover(|s| s.bg(theme.border)).text_color(theme.text))
            })
            .when(cleaning, |s| s.bg(theme.bg).text_color(theme.text_muted))
            .text_size(px(12.5))
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

    fn op_button(&self, theme: &Theme, label: &str, _danger: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let label_owned = label.to_string();
        let is_dns = label_owned.contains("DNS");
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
            .on_click(cx.listener(move |this, _, _, cx| {
                if is_dns {
                    this.flush_dns(cx);
                } else {
                    this.refresh_procs(cx);
                }
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
