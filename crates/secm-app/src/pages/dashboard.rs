// secm-app::pages::dashboard — 硬件信息页
//
// ⚠ 布局纪律：本页全程 flex 列/行装配，禁用 `.grid()` —— gpui 0.2.2 (taffy 0.9)
// 的 grid 子树在 `overflow_y_scroll` 滚动容器内不产出可渲染布局（v2.9.1 审计实证：
// 页头/背景正常、网格子树零像素），全应用已验证渲染路径均为 flex。
//
// 功能面（v2.9.1 补齐）：
// - 每秒轮询传感器（CPU 占用/频率/温度、内存、磁盘 —— SensorService 快照）；
// - CPU/GPU/内存 60 秒趋势图 + 下载/上传速率趋势图（sensor_history 1s 节拍采样，
//   JSON 持久化 %LOCALAPPDATA%\SECM\cache\，跨重启恢复）；
// - 网络流量卡：总量/各网卡源切换、0.5s–5s 可调采样间隔（GetIfTable2 累计字节
//   差分，无 PDH ≥1s 限制）、活跃 TCP 连接数、链路协商速度；
// - 磁盘存储卡：型号/容量/用量条 + SMART 健康状态（正常绿/风险关注黄/告警红 + 温度）。

use gpui::{div, px, Div, SharedString, Window, Context, Render, Timer, Rgba};
use gpui::prelude::*;
use secm_core::hardware::{self, DiskListItem, DiskSmartView};
use secm_core::sensor::SensorSnapshot;
use secm_core::sensor_history::{self, HistoryPoint, CHART_WINDOW_MS};
use secm_core::sensor_service::SensorService;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::pi_clone::theme::{Appearance, Palette};
use crate::ui::page::{
    button_sm, card, card_body, card_divider, card_header_accent, metric_value, page_header,
    page_root, sparkline, sparkline_empty, status_pill, ButtonKind,
};

/// 网络采样间隔档位（秒）
const NET_INTERVALS: [(f32, &str); 4] = [(0.5, "0.5s"), (1.0, "1s"), (2.0, "2s"), (5.0, "5s")];
/// 行内两卡横向间距（与页级纵向节奏 PAGE_GAP 一致，统一 20px 网格感）
const ROW_GAP: f32 = 20.0;

pub struct DashboardView {
    snap: SensorSnapshot,
    /// 页面可见性门控（P1-12）：仅当前页激活时拉取快照并 notify
    active: Arc<AtomicBool>,
    /// 页面外观，随壳主题联动
    appearance: Appearance,
    /// 页面滚动状态（GPUI 0.2 滚轮需 track_scroll 手动驱动，见 ui::page::page_root）
    page_scroll: gpui::ScrollHandle,
    // ---- 趋势历史（sensor_history 1s 节拍；JSON 持久化跨重启恢复）----
    hist_cpu: Vec<HistoryPoint>,
    hist_gpu: Vec<HistoryPoint>,
    hist_mem: Vec<HistoryPoint>,
    hist_rx: Vec<HistoryPoint>,
    hist_tx: Vec<HistoryPoint>,
    // ---- 网络流量卡 ----
    /// 采样间隔（秒；0.5/1/2/5，UI 档位切换后下一拍生效）
    net_interval: f32,
    /// 数据源：None=总量；Some(别名)=单网卡
    net_source: Option<String>,
    /// 每网卡实时速率（别名, 下行 KB/s, 上行 KB/s；名称升序）
    net_rates: Vec<(String, f32, f32)>,
    /// 总量速率（下行, 上行 KB/s）
    net_total: (f32, f32),
    /// 活跃 TCP 连接数（ESTABLISHED）
    tcp_estab: u32,
    /// 接口别名 → 链路协商速度
    link_speeds: HashMap<String, String>,
    /// 视图侧累计字节快照（速率差分基线）：(unix_ms, 别名 → (In, Out))
    net_prev: Option<(u64, HashMap<String, (u64, u64)>)>,
    // ---- 磁盘存储 + SMART ----
    disks: Vec<DiskListItem>,
    disks_loading: bool,
    /// 已读取的 SMART（磁盘 id → 视图）
    smart: HashMap<String, DiskSmartView>,
    /// SMART 读取中的磁盘 id
    smart_loading: Vec<String>,
    /// SMART 读取错误反馈
    disk_error: String,
}

impl DashboardView {
    pub fn new(active: Arc<AtomicBool>, appearance: Appearance, cx: &mut Context<Self>) -> Self {
        log::info!("硬件信息 · 页面已打开（传感器 1s 轮询 + 趋势历史 + 网络采样启动）");
        SensorService::start_once();
        // 趋势历史采样（幂等；含持久化恢复），随后拉取 60s 窗口渲染
        sensor_history::start_once();
        let hs = sensor_history::snapshot_series_window(CHART_WINDOW_MS);
        let mut view = Self {
            snap: SensorService::snapshot(),
            active,
            appearance,
            page_scroll: gpui::ScrollHandle::new(),
            hist_cpu: hs.cpu,
            hist_gpu: hs.gpu,
            hist_mem: hs.mem,
            hist_rx: hs.rx,
            hist_tx: hs.tx,
            net_interval: 1.0,
            net_source: None,
            net_rates: Vec::new(),
            net_total: (0.0, 0.0),
            tcp_estab: 0,
            link_speeds: HashMap::new(),
            net_prev: None,
            disks: Vec::new(),
            disks_loading: false,
            smart: HashMap::new(),
            smart_loading: Vec::new(),
            disk_error: String::new(),
        };
        view.schedule_refresh(cx);
        view.start_net_sampler(cx);
        view.load_disks(cx);
        view
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

    // ------------------------------------------------------------------
    // 后台任务
    // ------------------------------------------------------------------

    /// 1s 传感器轮询（P1-12 门控）+ 趋势历史窗口拉取（同拍一次 notify）
    fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            loop {
                Timer::after(Duration::from_millis(1000)).await;
                // 不可见时仅睡眠轮询，不取快照不 notify（P1-12）
                let is_active = this
                    .update(cx, |v, _| v.active.load(Ordering::Relaxed))
                    .unwrap_or(false);
                if !is_active {
                    continue;
                }
                let snap = SensorService::snapshot();
                let hist = sensor_history::snapshot_series_window(CHART_WINDOW_MS);
                let _ = this.update(cx, |view, cx| {
                    view.snap = snap;
                    view.hist_cpu = hist.cpu;
                    view.hist_gpu = hist.gpu;
                    view.hist_mem = hist.mem;
                    view.hist_rx = hist.rx;
                    view.hist_tx = hist.tx;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 网络采样任务：间隔可调（0.5–5s，每拍重读生效）；后台累计字节差分 +
    /// TCP 连接数 + 协商速度；单网卡序列回填 sensor_history（内存态）。
    fn start_net_sampler(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            loop {
                // 每拍重读间隔（档位切换下一拍生效，无需重启任务）
                let interval_ms = match this.update(cx, |v, _| (v.net_interval * 1000.0) as u64) {
                    Ok(ms) => ms.max(200),
                    Err(_) => return, // 视图已释放
                };
                Timer::after(Duration::from_millis(interval_ms)).await;
                let exec = cx.background_executor().clone();
                let sample = exec.spawn(async move { sample_net_once() }).await;
                let ok = this.update(cx, |v, cx| {
                    let (rates, total) = diff_rates(v.net_prev.as_ref(), &sample.map, sample.t);
                    v.net_prev = Some((sample.t, sample.map.clone()));
                    v.net_rates = rates.clone();
                    v.net_total = total;
                    v.tcp_estab = sample.tcp;
                    v.link_speeds = sample.speeds;
                    let refs: Vec<(String, f32, f32)> = rates
                        .iter()
                        .map(|(n, r, t)| (n.clone(), *r, *t))
                        .collect();
                    sensor_history::record_adapter_rates(&refs);
                    cx.notify();
                });
                if ok.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    /// 磁盘清单 + 全部 SMART 顺序后台加载（IOCTL 逐盘，避免句柄风暴）
    fn load_disks(&mut self, cx: &mut Context<Self>) {
        if self.disks_loading {
            return;
        }
        self.disks_loading = true;
        cx.notify();

        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let disks = exec.spawn(async move { hardware::list_disks() }).await;
            log::info!("硬件信息 · 磁盘枚举完成，共 {} 块物理盘", disks.len());
            if this
                .update(cx, |v, cx| {
                    v.disks_loading = false;
                    v.disks = disks;
                    cx.notify();
                })
                .is_err()
            {
                return;
            }
            let disks = this.update(cx, |v, _| v.disks.clone()).unwrap_or_default();
            for d in disks {
                let id = d.id.clone();
                if this
                    .update(cx, |v, cx| {
                        v.smart_loading.push(id.clone());
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
                let exec = cx.background_executor().clone();
                let res = exec.spawn(async move { hardware::read_smart(&id) }).await;
                let _ = this.update(cx, |v, cx| {
                    v.smart_loading.retain(|x| x != &d.id);
                    match res {
                        Ok(sv) => {
                            v.smart.insert(d.id.clone(), sv);
                        }
                        Err(e) => {
                            v.disk_error = format!("读取磁盘 {} SMART 失败：{}", d.model, e);
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 手动刷新磁盘与 SMART（清缓存重读）
    fn refresh_disks(&mut self, cx: &mut Context<Self>) {
        log::info!("硬件信息 · 手动刷新磁盘与 SMART");
        self.smart.clear();
        self.disk_error.clear();
        self.load_disks(cx);
    }

    // ------------------------------------------------------------------
    // 卡片装配（全部返回 Div，可继续链式；行内等宽由外层 flex_1 包裹实现）
    // ------------------------------------------------------------------

    /// 摘要统计卡：卡片头 + 主值 + 统计行 + 60s 趋势
    #[allow(clippy::too_many_arguments)]
    fn stat_card(
        &self,
        pal: &Palette,
        title: &str,
        dot: Rgba,
        main: String,
        stats: Vec<(Rgba, String, Option<String>)>,
        trend: Vec<f32>,
        trend_color: Rgba,
        trend_empty: &'static str,
    ) -> Div {
        card(pal)
            .child(card_header_accent(pal, title.to_string(), dot))
            .child(card_divider(pal))
            .child(
                card_body(pal)
                    .child(metric_value(pal, main))
                    // 卡内纵向节奏用显式 mt（容器纵向 gap 在 taffy 0.9.0 不生效）
                    .child(
                        div()
                            .mt_2()
                            .flex_col()
                            .children(stats.into_iter().map(|(c, text, badge)| {
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .py(px(2.0))
                                    .child(
                                        div().text_color(c).text_size(px(12.5)).child(text),
                                    )
                                    .when_some(badge, |s, b| {
                                        s.child(
                                            div()
                                                .text_size(px(10.0))
                                                .text_color(pal.text_muted)
                                                .child(b),
                                        )
                                    })
                            })),
                    )
                    .child(
                        // 趋势子组：标签紧贴图表（组内 4px），与统计行间隔 8px
                        div()
                            .mt_2()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(pal.text_dim)
                                    .child("60 秒趋势"),
                            )
                            .child(if trend.is_empty() {
                                sparkline_empty(pal, trend_empty).into_any_element()
                            } else {
                                sparkline(&trend, trend_color).into_any_element()
                            }),
                    ),
            )
    }

    /// CPU 卡
    fn cpu_card(&self, pal: &Palette, s: &SensorSnapshot) -> Div {
        let cpu = &s.cpu;
        let temp_text = if cpu.temperature > 0.0 {
            format!("温度 {:.0}°C", cpu.temperature)
        } else {
            "温度 —".to_string()
        };
        let stats = vec![
            (pal.text, temp_text, Some(cpu.temp_source.clone())),
            (
                pal.text_dim,
                format!("频率 {:.2} GHz", cpu.clock_mhz / 1000.0),
                Some(cpu.freq_source.clone()),
            ),
            (pal.text_muted, format!("{} 核", cpu.core_count), None),
        ];
        self.stat_card(
            pal,
            "CPU",
            pal.accent,
            format!("{:.0}%", cpu.usage),
            stats,
            window_vals(&self.hist_cpu),
            pal.accent,
            "等待采样…",
        )
    }

    /// 内存卡
    fn mem_card(&self, pal: &Palette, s: &SensorSnapshot) -> Div {
        let mem = &s.memory;
        let stats = vec![
            (
                pal.text,
                format!("已用 {:.1} / {:.1} GB", gb(mem.used), gb(mem.total)),
                Some(format!("{:.0}%", mem.usage_percent)),
            ),
            (pal.success, format!("可用 {:.1} GB", gb(mem.available)), None),
        ];
        self.stat_card(
            pal,
            "内存",
            pal.success,
            format!("{:.0}%", mem.usage_percent),
            stats,
            window_vals(&self.hist_mem),
            pal.success,
            "等待采样…",
        )
    }

    /// GPU 卡（无 GPU 时占位）
    fn gpu_card(&self, pal: &Palette, s: &SensorSnapshot) -> Div {
        match s.gpu.first() {
            Some(g) => {
                let stats = vec![
                    (
                        pal.text,
                        if g.temperature > 0.0 {
                            format!("温度 {:.0}°C", g.temperature)
                        } else {
                            "温度 —".to_string()
                        },
                        None,
                    ),
                    (
                        pal.text_dim,
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
                    (pal.text_muted, g.name.clone(), None),
                ];
                self.stat_card(
                    pal,
                    "GPU",
                    pal.warning,
                    format!("{:.0}%", g.usage),
                    stats,
                    window_vals(&self.hist_gpu),
                    pal.warning,
                    "等待采样…",
                )
            }
            None => {
                let stats = vec![(
                    pal.text_muted,
                    "未检测到 GPU（LHM 不可用或无独显）".to_string(),
                    None,
                )];
                self.stat_card(
                    pal,
                    "GPU",
                    pal.warning,
                    "—".to_string(),
                    stats,
                    Vec::new(),
                    pal.warning,
                    "无 GPU 数据",
                )
            }
        }
    }

    /// 磁盘存储 + SMART 健康卡
    fn disk_card(&self, pal: &Palette, cx: &mut Context<Self>) -> Div {
        let disks: Vec<DiskListItem> = self.disks.clone();
        let mut body = card_body(pal);

        if self.disks_loading && disks.is_empty() {
            body = body.child(
                div()
                    .py_4()
                    .text_size(px(12.0))
                    .text_color(pal.text_muted)
                    .child("正在枚举物理磁盘…"),
            );
        } else if disks.is_empty() {
            body = body.child(
                div()
                    .py_4()
                    .text_size(px(12.0))
                    .text_color(pal.text_muted)
                    .child("未检测到物理磁盘"),
            );
        }

        for (ix, d) in disks.iter().enumerate() {
            // SMART 状态行（读取中 → 弱化；绿/黄/红三级）
            let smart_line = match self.smart.get(&d.id) {
                Some(sv) => {
                    let (healthy, risk, detail) = smart_state(sv);
                    let (color, label) = if !healthy {
                        (pal.danger, "告警")
                    } else if risk {
                        (pal.warning, "关注")
                    } else {
                        (pal.success, "正常")
                    };
                    let temp = disk_temp(sv)
                        .map(|t| format!("温度 {:.0}°C", t))
                        .unwrap_or_else(|| "温度 —".to_string());
                    div()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(status_pill(pal, label, color))
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(pal.text_muted)
                                        .child(temp),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .text_size(px(10.5))
                                        .text_color(pal.text_dim)
                                        .truncate()
                                        .child(SharedString::from(format!(
                                            "{} · {}",
                                            d.interface_type, d.media_type
                                        ))),
                                ),
                        )
                        .when(!detail.is_empty() && color != pal.success, |s| {
                            s.child(
                                div()
                                    .mt_1()
                                    .text_size(px(10.5))
                                    .text_color(pal.text_muted)
                                    .truncate()
                                    .child(SharedString::from(detail)),
                            )
                        })
                }
                None => {
                    let reading = self.smart_loading.iter().any(|x| x == &d.id);
                    div().child(if reading {
                        status_pill(pal, "SMART 读取中…", pal.text_dim)
                    } else {
                        status_pill(pal, "SMART 待读取", pal.text_dim)
                    })
                }
            };
            // 每块盘一个分组（型号行 + SMART 行），盘间 12px 用显式 mt 承担
            //（容器纵向 gap 在 taffy 0.9.0 不生效）
            body = body.child(
                div()
                    .flex_col()
                    .when(ix > 0, |s| s.mt(px(12.0)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .truncate()
                                    .text_size(px(12.5))
                                    .text_color(pal.text)
                                    .child(SharedString::from(d.model.clone())),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.0))
                                    .text_color(pal.text_muted)
                                    .child(SharedString::from(format!("{:.0} GB", d.size_gb))),
                            ),
                    )
                    .child(smart_line.mt_1()),
            );
        }

        // 尾部：手动刷新 + 错误反馈
        let err = self.disk_error.clone();
        body = body
            .when(!err.is_empty(), |s| {
                s.child(
                    div()
                        .text_size(px(10.5))
                        .text_color(pal.danger)
                        .child(SharedString::from(err)),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        button_sm(pal, ButtonKind::Secondary)
                            .id("dash-disk-refresh")
                            .child("刷新磁盘与 SMART")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh_disks(cx);
                            })),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(pal.text_dim)
                            .child("SMART 详情见「硬件检测」页"),
                    ),
            );

        card(pal)
            .child(card_header_accent(pal, "磁盘存储 · SMART 健康", pal.text_muted))            .child(card_divider(pal))
            .child(body)
    }

    /// 网络速率趋势卡（总量下行/上行 60s；历史跨重启持久化）
    fn net_trend_card(&self, pal: &Palette) -> Div {
        let rx_60 = window_vals(&self.hist_rx);
        let tx_60 = window_vals(&self.hist_tx);
        card(pal)
            .child(card_header_accent(pal, "网络速率趋势", pal.accent))
            .child(card_divider(pal))
            .child(
                card_body(pal)
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(div().size(px(6.0)).rounded_full().bg(pal.accent))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(pal.text)
                                            .child(SharedString::from(format!(
                                                "↓ 下行 {}",
                                                fmt_kbps(self.net_total.0)
                                            ))),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(div().size(px(6.0)).rounded_full().bg(pal.success))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(pal.text)
                                            .child(SharedString::from(format!(
                                                "↑ 上行 {}",
                                                fmt_kbps(self.net_total.1)
                                            ))),
                                    ),
                            ),
                    )
                    .child(
                        // 下行趋势子组：标签紧贴图表（组间 8px 用显式 mt）
                        div()
                            .mt_2()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(pal.text_dim)
                                    .child("下行 · 60 秒（每秒采样 · 跨重启恢复）"),
                            )
                            .child(if rx_60.is_empty() {
                                sparkline_empty(pal, "等待采样…").into_any_element()
                            } else {
                                sparkline(&rx_60, pal.accent).into_any_element()
                            }),
                    )
                    .child(
                        // 上行趋势子组：标签紧贴图表
                        div()
                            .mt_2()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(pal.text_dim)
                                    .child("上行 · 60 秒"),
                            )
                            .child(if tx_60.is_empty() {
                                sparkline_empty(pal, "等待采样…").into_any_element()
                            } else {
                                sparkline(&tx_60, pal.success).into_any_element()
                            }),
                    ),
            )
    }

    /// 网络流量卡：源切换 + 间隔档位 + 各网卡速率 + TCP 连接数 + 协商速度
    fn net_traffic_card(&self, pal: &Palette, cx: &mut Context<Self>) -> Div {
        // 源档位：总量 + 各网卡（上限 6 个，避免行溢出）
        let sources: Vec<Option<String>> = std::iter::once(None)
            .chain(
                self.net_rates
                    .iter()
                    .take(6)
                    .map(|(n, _, _)| Some(n.clone())),
            )
            .collect();
        let selected = self.net_source.clone();
        let tcp = self.tcp_estab;

        // 展示行：按流量降序取 8；选中源强制保留
        let mut rows: Vec<&(String, f32, f32)> = self.net_rates.iter().collect();
        rows.sort_by(|a, b| {
            let at = a.1 + a.2;
            let bt = b.1 + b.2;
            bt.partial_cmp(&at).unwrap_or(std::cmp::Ordering::Equal)
        });
        rows.truncate(8);
        if let Some(sel) = &selected {
            if !rows.iter().any(|(n, _, _)| n == sel) {
                if let Some(r) = self.net_rates.iter().find(|(n, _, _)| n == sel) {
                    rows.insert(0, r);
                }
            }
        }

        // 协商速度：选中网卡优先，否则取流量最大的已连接口
        let speed_line = {
            let name = selected.clone().or_else(|| {
                self.net_rates
                    .first()
                    .map(|(n, _, _)| n.clone())
            });
            name.and_then(|n| {
                self.link_speeds.get(&n).cloned().map(|sp| (n, sp))
            })
            .map(|(n, sp)| {
                format!("协商速度 {}（{}）", sp, n)
            })
            .unwrap_or_else(|| "协商速度 —".to_string())
        };

        let mut body = card_body(pal);

        // 数据源档位行
        body = body.child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_1p5()
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(pal.text_dim)
                        .child("数据源"),
                )
                .children(sources.into_iter().map(|src| {
                    let is_sel = src == selected;
                    let label = src.clone().unwrap_or_else(|| "总量".to_string());
                    let src_c = src.clone();
                    let kind = if is_sel {
                        ButtonKind::Primary
                    } else {
                        ButtonKind::Ghost
                    };
                    button_sm(pal, kind)
                        .id(SharedString::from(format!("net-src-{}", label)))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.net_source = src_c.clone();
                            cx.notify();
                        }))
                })),
        );

        // 采样间隔档位行 + TCP 连接数（与上一段间隔 8px —— 显式 mt）
        body = body.child(
            div()
                .mt_2()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_1p5()
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(pal.text_dim)
                        .child("采样间隔"),
                )
                .children(NET_INTERVALS.iter().map(|(val, label)| {
                    let is_sel = (self.net_interval - val).abs() < f32::EPSILON;
                    let val_c = *val;
                    let kind = if is_sel {
                        ButtonKind::Primary
                    } else {
                        ButtonKind::Ghost
                    };
                    button_sm(pal, kind)
                        .id(SharedString::from(format!("net-int-{}", label)))
                        .child((*label).to_string())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.net_interval = val_c;
                            log::info!("硬件信息 · 网络采样间隔 → {}s", val_c);
                            cx.notify();
                        }))
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .justify_end()
                        .child(status_pill(
                            pal,
                            SharedString::from(format!("TCP 活跃 {}", tcp)),
                            pal.accent,
                        )),
                ),
        );

        // 当前源速率大字
        let (cur_rx, cur_tx) = match &selected {
            Some(name) => self
                .net_rates
                .iter()
                .find(|(n, _, _)| n == name)
                .map(|(_, r, t)| (*r, *t))
                .unwrap_or((0.0, 0.0)),
            None => self.net_total,
        };
        body = body.child(
            div()
                .mt_2()
                .flex()
                .items_baseline()
                .gap_3()
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(pal.text)
                        .child(SharedString::from(format!(
                            "↓ {}",
                            fmt_kbps(cur_rx)
                        ))),
                )
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(pal.text)
                        .child(SharedString::from(format!(
                            "↑ {}",
                            fmt_kbps(cur_tx)
                        ))),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(pal.text_dim)
                        .child(SharedString::from(speed_line)),
                ),
        );

        // 各网卡速率行（与上一段间隔 8px —— 显式 mt；行距 4px 用 py 承担）
        if rows.is_empty() {
            body = body.child(
                div()
                    .mt_2()
                    .py_2()
                    .text_size(px(11.5))
                    .text_color(pal.text_muted)
                    .child("等待网络采样…"),
            );
        } else {
            body = body.child(
                div().mt_2().flex_col().children(rows.into_iter().map(|(name, rx, tx)| {
                    let is_sel = selected.as_deref() == Some(name.as_str());
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py(px(3.0))
                        .rounded(px(6.0))
                        .when(is_sel, |s| s.bg(pal.bg_hover))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(12.0))
                                .text_color(if is_sel { pal.text } else { pal.text_muted })
                                .child(SharedString::from(name.clone())),
                        )
                        .child(
                            div()
                                .w(px(92.0))
                                .flex_none()
                                .text_size(px(11.0))
                                .text_color(pal.text_muted)
                                .child(SharedString::from(format!("↓ {}", fmt_kbps(*rx)))),
                        )
                        .child(
                            div()
                                .w(px(92.0))
                                .flex_none()
                                .text_size(px(11.0))
                                .text_color(pal.text_muted)
                                .child(SharedString::from(format!("↑ {}", fmt_kbps(*tx)))),
                        )
                })),
            );
        }

        card(pal)
            .child(card_header_accent(pal, "网络流量", pal.success))
            .child(card_divider(pal))
            .child(body)
    }
}

impl Render for DashboardView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.pal();
        let s = self.snap.clone();

        // 行装配：等宽两列 flex（禁 grid —— 见文件头布局纪律）
        let cpu = self.cpu_card(&pal, &s).flex_1().min_w(px(0.0));
        let mem = self.mem_card(&pal, &s).flex_1().min_w(px(0.0));
        let gpu = self.gpu_card(&pal, &s).flex_1().min_w(px(0.0));
        let disk = self.disk_card(&pal, cx).flex_1().min_w(px(0.0));
        let trend = self.net_trend_card(&pal).flex_1().min_w(px(0.0));
        let traffic = self.net_traffic_card(&pal, cx).flex_1().min_w(px(0.0));
        let row = |a: gpui::Div, b: gpui::Div| {
            div().flex().gap(px(ROW_GAP)).child(a).child(b)
        };

        let diag = if s.diag.is_empty() {
            "diag: 采集就绪".to_string()
        } else {
            format!("diag: {}", s.diag)
        };

        page_root(&pal, "dashboard-page-root", &self.page_scroll, &cx.entity())
            .child(
                page_header(
                    &pal,
                    "硬件信息",
                    format!(
                        "每秒刷新 · CPU / 内存 / GPU / 磁盘 · 60s 趋势跨重启 · GPUI v{}",
                        env!("CARGO_PKG_VERSION")
                    ),
                )
                .child(status_pill(&pal, "在线", pal.success)),
            )
            .child(row(cpu, mem))
            .child(row(gpu, disk))
            .child(row(trend, traffic))
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(pal.text_muted)
                    .child(diag),
            )
    }
}

// ---------------------------------------------------------------------------
// 采样与推导辅助
// ---------------------------------------------------------------------------

/// 网络单拍采样（后台线程执行；全微秒级 Win32 调用）
struct NetSample {
    t: u64,
    map: HashMap<String, (u64, u64)>,
    tcp: u32,
    speeds: HashMap<String, String>,
}

fn sample_net_once() -> NetSample {
    NetSample {
        t: now_ms(),
        map: secm_core::netif::if_bytes_map(),
        tcp: secm_core::netif::tcp_connection_count(),
        speeds: secm_core::netif::link_speeds(),
    }
}

/// 累计字节差分 → 每网卡速率（KB/s）+ 总量；首拍无基线返回空集
fn diff_rates(
    prev: Option<&(u64, HashMap<String, (u64, u64)>)>,
    map: &HashMap<String, (u64, u64)>,
    t: u64,
) -> (Vec<(String, f32, f32)>, (f32, f32)) {
    let mut rates: Vec<(String, f32, f32)> = Vec::new();
    let mut total = (0.0f32, 0.0f32);
    if let Some((pt, pmap)) = prev {
        let dt = (t.saturating_sub(*pt)) as f32 / 1000.0;
        if dt >= 0.05 {
            for (name, (rx, tx)) in map {
                if let Some((prx, ptx)) = pmap.get(name) {
                    let drx = if rx >= prx {
                        (*rx - prx) as f32 / dt / 1024.0
                    } else {
                        0.0
                    };
                    let dtx = if tx >= ptx {
                        (*tx - ptx) as f32 / dt / 1024.0
                    } else {
                        0.0
                    };
                    total.0 += drx;
                    total.1 += dtx;
                    rates.push((name.clone(), drx, dtx));
                }
            }
        }
    }
    rates.sort_by(|a, b| a.0.cmp(&b.0));
    (rates, total)
}

/// SMART 状态推导：(健康, 风险关注, 明细)。
/// 黄色"关注" = 健康但存在劣化前兆（NVMe 寿命 ≥80% / 媒体错误 / 备用空间逼近阈值）。
fn smart_state(sv: &DiskSmartView) -> (bool, bool, String) {
    let d = &sv.disk;
    let mut risk = false;
    if let Some(nv) = &d.nvme_health {
        if nv.percentage_used >= 80 {
            risk = true;
        }
        if nv.media_errors > 0 {
            risk = true;
        }
        if nv.available_spare_threshold > 0
            && nv.available_spare <= nv.available_spare_threshold.saturating_mul(2)
        {
            risk = true;
        }
    }
    (sv.summary.healthy, risk && sv.summary.healthy, sv.summary.detail.clone())
}

/// 磁盘温度（NVMe 健康日志 → WMI → ATA 194/190 属性，单位 °C）
fn disk_temp(sv: &DiskSmartView) -> Option<f32> {
    let d = &sv.disk;
    if let Some(nv) = &d.nvme_health {
        if nv.temperature_c > 0 {
            return Some(nv.temperature_c as f32);
        }
    }
    if let Some(w) = &d.wmi_health {
        if let Some(t) = w.temperature_c {
            if t > 0 {
                return Some(t as f32);
            }
        }
    }
    d.attributes
        .iter()
        .find(|a| a.id == 194 || a.id == 190)
        .map(|a| a.value as f32)
}

/// 取 60s 窗口内的数值序列（趋势图渲染输入）
fn window_vals(points: &[HistoryPoint]) -> Vec<f32> {
    let cutoff = now_ms().saturating_sub(CHART_WINDOW_MS);
    points.iter().filter(|p| p.t >= cutoff).map(|p| p.v).collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// KB/s → 人类可读
fn fmt_kbps(kbps: f32) -> String {
    if kbps < 0.1 {
        "0 KB/s".to_string()
    } else if kbps < 1024.0 {
        format!("{:.0} KB/s", kbps)
    } else {
        format!("{:.2} MB/s", kbps / 1024.0)
    }
}

fn gb(b: u64) -> f64 {
    b as f64 / (1024.0 * 1024.0 * 1024.0)
}
