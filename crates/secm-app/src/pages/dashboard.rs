// secm-app::pages::dashboard — 硬件信息页（Dashboard，Phase 1）
// 订阅 secm-core SensorService 快照，1s 刷新；卡片网格展示 CPU/内存/磁盘/网络占位。

use gpui::{
    div, px, rgb, Window, Context, Render, Timer, Rgba,
};
use gpui::prelude::*;
use secm_core::sensor::SensorSnapshot;
use secm_core::sensor_service::SensorService;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::theme::Theme;

pub struct DashboardView {
    snap: SensorSnapshot,
    /// 页面可见性门控（P1-12）：仅当前页激活时拉取快照并 notify，
    /// 避免页面不可见时 1s 空转（AppRoot::navigate 控制）
    active: Arc<AtomicBool>,
}

impl DashboardView {
    pub fn new(active: Arc<AtomicBool>, cx: &mut Context<Self>) -> Self {
        SensorService::start_once();
        let mut view = Self {
            snap: SensorService::snapshot(),
            active,
        };
        view.schedule_refresh(cx);
        view
    }

    fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        let active = self.active.clone();
        cx.spawn(
            async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                loop {
                    Timer::after(Duration::from_millis(1000)).await;
                    // 不可见时仅睡眠轮询，不取快照不 notify（P1-12）
                    if !active.load(Ordering::Relaxed) {
                        continue;
                    }
                    let snap = SensorService::snapshot();
                    if let Some(view) = this.upgrade() {
                        view.update(cx, |view, cx| {
                            view.snap = snap;
                            cx.notify();
                        })
                        .ok();
                    }
                }
            },
        )
        .detach();
    }

    /// GPU 摘要卡（数据来自 LHM；无 GPU 时占位）
    fn gpu_stat_card(&self, theme: &Theme, s: &SensorSnapshot) -> impl IntoElement {
        match s.gpu.first() {
            Some(g) => {
                let gpu_stats = vec![
                    (
                        theme.brand,
                        format!("占用 {:.0}%", g.usage),
                        None,
                    ),
                    (
                        theme.danger,
                        if g.temperature > 0.0 {
                            format!("温度 {:.0}°C", g.temperature)
                        } else {
                            "温度 —".to_string()
                        },
                        None,
                    ),
                    (
                        theme.info,
                        if g.memory_total > 0 {
                            format!(
                                "显存 {:.0} / {:.0} GB",
                                gb(g.memory_used),
                                gb(g.memory_total)
                            )
                        } else {
                            "显存 —".to_string()
                        },
                        None,
                    ),
                    (
                        theme.text_muted,
                        g.name.clone(),
                        None,
                    ),
                ];
                self.stat_card(theme, "GPU", rgb(0xa78bfa), format!("{:.0}%", g.usage), gpu_stats)
                    .into_any_element()
            }
            None => {
                // 无 GPU 数据：占位卡
                let stats = vec![(theme.text_muted, "未检测到 GPU（LHM 不可用或无独显）".to_string(), None)];
                self.stat_card(theme, "GPU", rgb(0xa78bfa), "—".into(), stats)
                    .into_any_element()
            }
        }
    }

    /// 摘要卡（标题 + 圆点图标色 + 主值 + 统计行）
    fn stat_card(
        &self,
        theme: &Theme,
        title: &str,
        dot: Rgba,
        main: String,
        stats: Vec<(Rgba, String, Option<String>)>, // (色, 文本, badge)
    ) -> impl IntoElement {
        div()
            .flex_col()
            .p_5()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.panel)
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(8.0)).rounded_full().bg(dot))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(theme.text_muted)
                            .child(title.to_string()),
                    ),
            )
            .child(
                div()
                    .text_size(px(30.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.text)
                    .child(main),
            )
            .child(
                div()
                    .flex_col()
                    .gap_1p5()
                    .children(stats.into_iter().map(|(c, text, badge)| {
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_color(c).text_size(px(12.5)).child(text))
                            .when_some(badge, |s, b| {
                                s.child(
                                    div()
                                        .text_size(px(10.0))
                                        .text_color(theme.text_muted)
                                        .child(b),
                                )
                            })
                    })),
            )
    }
}

impl Render for DashboardView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::dark();
        let s = &self.snap;
        let cpu = &s.cpu;
        let mem = &s.memory;

        // ---- CPU 卡 ----
        let cpu_temp = if cpu.temperature > 0.0 {
            format!("{:.0}°C", cpu.temperature)
        } else {
            "—".to_string()
        };
        let cpu_badge = if cpu.temperature > 0.0 {
            Some(cpu.temp_source.clone())
        } else {
            None
        };
        let cpu_stats = vec![
            (theme.brand, format!("占用 {:.0}%", cpu.usage), None),
            (
                theme.info,
                format!("频率 {:.2} GHz", cpu.clock_mhz / 1000.0),
                Some(cpu.freq_source.clone()),
            ),
            (theme.danger, format!("温度 {}", cpu_temp), cpu_badge),
            (
                theme.text_muted,
                format!("{} 核", cpu.core_count),
                None,
            ),
        ];

        // ---- 内存卡 ----
        let mem_stats = vec![
            (
                theme.brand,
                format!(
                    "已用 {:.1} GB / {:.1} GB",
                    gb(mem.used),
                    gb(mem.total)
                ),
                Some(format!("{:.0}%", mem.usage_percent)),
            ),
            (
                theme.success,
                format!("可用 {:.1} GB", gb(mem.available)),
                None,
            ),
        ];

        // ---- 磁盘卡（首个磁盘摘要）----
        let (disk_title, disk_main, disk_stats): (&str, String, Vec<_>) = match s.disks.first() {
            Some(d) => (
                "磁盘",
                format!("{:.0}%", d.usage_percent),
                vec![
                    (
                        theme.warn,
                        format!("{:.1} GB / {:.1} GB", gb(d.used_space), gb(d.total_space)),
                        Some(d.name.clone()),
                    ),
                    (theme.text_muted, "SMART 详情见「硬件检测」页".to_string(), None),
                ],
            ),
            None => ("磁盘", "—".into(), Vec::new()),
        };

        // ---- 信息行（diag + 版本）----
        let diag = if s.diag.is_empty() {
            "diag: 采集就绪".to_string()
        } else {
            format!("diag: {}", s.diag)
        };

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
                                    .child("硬件信息"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme.text_muted)
                                    .child(format!("每秒刷新 · GPUI v{}", env!("CARGO_PKG_VERSION"))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .size(px(7.0))
                                    .rounded_full()
                                    .bg(theme.success),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child("在线"),
                            ),
                    ),
            )
            // 卡片网格（等宽 2 列：CPU/内存 + GPU/磁盘）
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_4()
                    .child(self.stat_card(&theme, "CPU", theme.brand, format!("{:.0}%", cpu.usage), cpu_stats))
                    .child(self.stat_card(&theme, "内存", theme.success, format!("{:.0}%", mem.usage_percent), mem_stats))
                    .child(self.gpu_stat_card(&theme, s))
                    .child(self.stat_card(&theme, disk_title, theme.warn, disk_main, disk_stats)),
            )
            // diag 行
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.text_muted)
                    .child(diag),
            )
    }
}

fn gb(b: u64) -> f64 {
    b as f64 / (1024.0 * 1024.0 * 1024.0)
}
