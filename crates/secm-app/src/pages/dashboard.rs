// secm-app::pages::dashboard — 硬件信息页（Dashboard，Phase 1）
// 订阅 secm-core SensorService 快照，1s 周期刷新展示 CPU/内存/磁盘摘要卡。

use gpui::{
    div, px, SharedString, Window, Context, Render, Timer,
};
use gpui::prelude::*;
use secm_core::sensor::SensorSnapshot;
use secm_core::sensor_service::SensorService;
use std::time::Duration;

pub struct DashboardView {
    /// 最新快照摘要文本（Phase 1 以文本卡展示；后续替换为卡片网格）
    summary: SharedString,
}

impl DashboardView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // 启动传感器服务（幂等：仅首个调用真正启动后台线程）
        SensorService::start_once();
        let mut view = Self {
            summary: SharedString::from("传感器服务启动中…"),
        };
        view.schedule_refresh(cx);
        view
    }

    /// 1s 周期刷新快照（GPUI Timer + Context::spawn；WeakEntity 回 UI 更新）
    fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        cx.spawn(
            async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                loop {
                    Timer::after(Duration::from_millis(1000)).await;
                    let snap = SensorService::snapshot();
                    if let Some(view) = this.upgrade() {
                        view.update(cx, |view, cx| {
                            view.summary = snapshot_summary(&snap);
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

/// 把快照格式化为展示文本（Phase 1 文本卡；后续替换卡片网格）
fn snapshot_summary(s: &SensorSnapshot) -> SharedString {
    let cpu = &s.cpu;
    let mem = &s.memory;
    let mut parts = Vec::new();
    parts.push(format!("CPU: {:.0}% · {:.1} GHz ({})", cpu.usage, cpu.clock_mhz / 1000.0, cpu.name));
    if cpu.temperature > 0.0 {
        parts.push(format!("温度 {:.0}°C ({})", cpu.temperature, cpu.temp_source));
    } else {
        parts.push("温度 n/a（LHM 链后续接入）".into());
    }
    parts.push(format!(
        "内存: {:.0}% ({:.1}/{:.1} GB)",
        mem.usage_percent,
        bytes_gb(mem.used),
        bytes_gb(mem.total)
    ));
    let disk_top = s.disks.first();
    if let Some(d) = disk_top {
        parts.push(format!(
            "磁盘 {}: {:.0}% ({:.1}/{:.1} GB)",
            d.name,
            d.usage_percent,
            bytes_gb(d.used_space),
            bytes_gb(d.total_space)
        ));
    }
    let mut out = parts.join("\n");
    if !s.diag.is_empty() {
        out.push_str(&format!("\n\ndiag: {}", s.diag));
    }
    out.into()
}

fn bytes_gb(b: u64) -> f64 {
    b as f64 / (1024.0 * 1024.0 * 1024.0)
}

impl Render for DashboardView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = crate::theme::Theme::dark();
        div()
            .flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(24.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("硬件信息"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text_muted)
                            .child("每秒刷新 · GPUI 版"),
                    ),
            )
            .child(
                div()
                    .mt_2()
                    .p_5()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.panel)
                    .text_color(theme.text)
                    .text_size(px(14.0))
                    .whitespace_nowrap()
                    .child(self.summary.clone()),
            )
    }
}
