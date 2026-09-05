// secm-app::pages::cleanup — 清理优化页
// 进程管理（Top 200 表 + 优先级操作）+ DNS 刷新；缓存清理目录扫描后续补齐。

use gpui::{div, px, rgb, SharedString, Window, Context, Render};
use gpui::prelude::*;
use secm_core::cleanup::{self, ProcessInfo};

use crate::theme::Theme;

pub struct CleanupView {
    procs: Vec<ProcessInfo>,
    /// DNS 刷新等操作结果反馈
    status: SharedString,
    /// 搜索关键词（进程名过滤；GPUI 输入接入前用按钮式筛选由 UI 简化）
    keyword: SharedString,
}

impl CleanupView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut v = Self {
            procs: cleanup::list_processes(),
            status: SharedString::from(""),
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
            format!("{}：{}", r.operation, r.message)
        } else {
            format!("{} 失败：{}", r.operation, r.message)
        });
        cx.notify();
    }

    fn set_prio(&mut self, pid: u32, prio: &str, cx: &mut Context<Self>) {
        let r = cleanup::set_process_priority(pid, prio);
        self.status = SharedString::from(if r.success {
            r.message.clone()
        } else {
            format!("{}：{}", r.operation, r.message)
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
}

impl Render for CleanupView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::dark();
        let procs = self.filtered();

        div()
            .flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .child(
                div()
                    .text_size(px(24.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.text)
                    .child("清理优化"),
            )
            // 状态消息
            .when(!self.status.is_empty(), |s| {
                let msg = self.status.clone();
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
            // 快捷操作行
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
                            .child("快捷操作"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.op_button(&theme, "刷新 DNS 缓存", cx))
                            .child(self.op_button(&theme, "刷新进程列表", cx)),
                    ),
            )
            // 进程表
            .child(
                crate::ui::table_container(&theme).child(
                    div()
                        .id("proc-scroll")
                        .flex_col()
                        .h(px(440.0))
                        .overflow_scroll()
                        .child(crate::ui::table_head(&theme, &["PID", "进程名", "内存", "优先级"]))
                        .children(procs.iter().map(|p| self.proc_row(&theme, p, cx))),
                ),
            )
    }
}

impl CleanupView {
    fn op_button(&self, theme: &Theme, label: &str, cx: &mut Context<Self>) -> impl IntoElement {
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
            // PID
            .child(
                div()
                    .w(px(60.0))
                    .flex_none()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child(pid.to_string()),
            )
            // 名称
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.5))
                    .text_color(theme.text)
                    .child(name.clone()),
            )
            // 内存
            .child(
                div()
                    .w(px(80.0))
                    .flex_none()
                    .text_size(px(12.0))
                    .text_color(theme.text_muted)
                    .child(mem_disp),
            )
            // 优先级按钮组（低/标准/高）
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
