// secm-app::pages::dashboard — 硬件信息页
//
// ⚠ 布局纪律：本页全程 flex 列/行装配，禁用 `.grid()` —— gpui 0.2.2 (taffy 0.9)
// 的 grid 子树在 `overflow_y_scroll` 滚动容器内不产出可渲染布局（v2.9.1 审计实证：
// 页头/背景正常、网格子树零像素），全应用已验证渲染路径均为 flex。
//
// 功能面（v3.1 整理）：
// - 每秒轮询传感器（CPU 占用/频率/温度、内存、磁盘 —— SensorService 快照）；
// - CPU/GPU/内存 60 秒趋势图 + 下载/上传速率趋势图（sensor_history 1s 节拍采样，
//   JSON 持久化 %LOCALAPPDATA%\SECM\cache\，跨重启恢复）；
//   上行四卡（CPU/内存/GPU/网络速率趋势）等高，趋势波形统一贴卡片底部对齐；
// - 网络流量卡：仅展示**已连接**网卡的链接信息（名称/协商速度/IPv4/实时 ↓↑ 速率，
//   link_speed 非空判定）+ 活跃 TCP 连接数；v3.1 移除数据源/采样间隔档位与
//   UI 侧单网卡采样任务（数据全部来自统一快照，1s 节拍）；
// - 磁盘存储卡：型号/容量/用量条 + SMART 健康状态（正常绿/风险关注黄/告警红 + 温度）。

use gpui::prelude::*;
use gpui::{div, px, Context, Div, Render, Rgba, SharedString, Timer, Window};
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
            disks: Vec::new(),
            disks_loading: false,
            smart: HashMap::new(),
            smart_loading: Vec::new(),
            disk_error: String::new(),
        };
        view.schedule_refresh(cx);
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
        cx.spawn(
            async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
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
            },
        )
        .detach();
    }

    /// 磁盘清单 + 全部 SMART 顺序后台加载（IOCTL 逐盘，避免句柄风暴）
    fn load_disks(&mut self, cx: &mut Context<Self>) {
        if self.disks_loading {
            return;
        }
        self.disks_loading = true;
        cx.notify();

        cx.spawn(
            async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
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
            },
        )
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
                // flex_1：撑满卡片剩余高度（行内四卡等高），使下方趋势波形贴卡底
                card_body(pal)
                    .flex_1()
                    .child(metric_value(pal, main))
                    // 卡内纵向节奏用显式 mt（容器纵向 gap 在 taffy 0.9.0 不生效）
                    .child(div().mt_2().flex_col().children(stats.into_iter().map(
                        |(c, text, badge)| {
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .py(px(2.0))
                                .child(div().text_color(c).text_size(px(12.5)).child(text))
                                .when_some(badge, |s, b| {
                                    s.child(
                                        div()
                                            .text_size(px(10.0))
                                            .text_color(pal.text_muted)
                                            .child(b),
                                    )
                                })
                        },
                    )))
                    .child(
                        // 趋势子组：标签紧贴图表（组内 4px）；mt_auto 推到卡底
                        //（v3.1：CPU/内存/GPU/网络速率趋势四卡波形底部统一对齐）
                        div()
                            .mt_auto()
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
        // Metric 语义：None = 不可用（显示 —，不伪造 0）
        let temp_text = match cpu.temperature.value {
            Some(t) => format!("温度 {:.0}°C", t),
            None => "温度 —".to_string(),
        };
        let clock_text = match cpu.clock_mhz.value {
            Some(mhz) if mhz > 0.0 => format!("频率 {:.2} GHz", mhz / 1000.0),
            _ => "频率 —".to_string(),
        };
        let stats = vec![
            (
                pal.text,
                temp_text,
                Some(cpu.temperature.source.as_str().to_string()),
            ),
            (
                pal.text_dim,
                clock_text,
                Some(cpu.clock_mhz.source.as_str().to_string()),
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
            (
                pal.success,
                format!("可用 {:.1} GB", gb(mem.available)),
                None,
            ),
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
                        match g.temperature.value {
                            Some(t) => format!("温度 {:.0}°C", t),
                            None => "温度 —".to_string(),
                        },
                        None,
                    ),
                    (
                        pal.text_dim,
                        match (g.vram_used.value, g.vram_total.value) {
                            (Some(u), Some(t)) if t > 0 => {
                                format!("显存 {:.0} / {:.0} GB", gb(u), gb(t))
                            }
                            _ => "显存 —".to_string(),
                        },
                        None,
                    ),
                    (pal.text_muted, g.name.clone(), None),
                ];
                let gpu_main = match g.usage.value {
                    Some(u) => format!("{:.0}%", u),
                    None => "—".to_string(),
                };
                self.stat_card(
                    pal,
                    "GPU",
                    pal.warning,
                    gpu_main,
                    stats,
                    window_vals(&self.hist_gpu),
                    pal.warning,
                    "等待采样…",
                )
            }
            None => {
                let stats = vec![(
                    pal.text_muted,
                    "未检测到 GPU（NVML/DXGI 未枚举到适配器）".to_string(),
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
            .child(card_header_accent(
                pal,
                "磁盘存储 · SMART 健康",
                pal.text_muted,
            ))
            .child(card_divider(pal))
            .child(body)
    }

    /// 网络速率趋势卡（总量下行/上行 60s；历史跨重启持久化）
    fn net_trend_card(&self, pal: &Palette) -> Div {
        let rx_60 = window_vals(&self.hist_rx);
        let tx_60 = window_vals(&self.hist_tx);
        // 当前总量速率（直接对统一快照求和；与 sensor_history 1s 节拍同源）
        let (total_rx, total_tx) = net_total_now(&self.snap);
        card(pal)
            .child(card_header_accent(pal, "网络速率趋势", pal.accent))
            .child(card_divider(pal))
            .child(
                // flex_1：撑满卡片剩余高度（行内四卡等高），使下方上行波形贴卡底
                card_body(pal)
                    .flex_1()
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
                                                fmt_kbps(total_rx)
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
                                                fmt_kbps(total_tx)
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
                        // 上行趋势子组：mt_auto 推到卡底（v3.1：四卡波形底部统一对齐）
                        div()
                            .mt_auto()
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

    /// 网络流量卡：仅展示**已连接**网卡的链接信息（v3.1 简化）。
    /// "已连接" = 链路协商速度已生效（link_speed 非空）；每张已连接网卡展示
    /// 名称 / 协商速度 / IPv4 / 实时 ↓↑ 速率，头部概览 = 连接数 + 活跃 TCP 连接数。
    /// 数据全部来自统一快照（1s 节拍），不再有数据源/采样间隔档位与 UI 侧采样任务。
    fn net_traffic_card(&self, pal: &Palette) -> Div {
        // 已连接网卡（快照接口序 = 名称升序，稳定不抖动）
        let connected: Vec<&secm_core::sensor::NetIfStat> = self
            .snap
            .net
            .interfaces
            .iter()
            .filter(|i| !i.link_speed.is_empty())
            .collect();

        let mut body = card_body(pal).flex_1();

        // 概览行：连接数 + 活跃 TCP 连接数
        body = body.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_size(px(10.5)).text_color(pal.text_dim).child(
                    SharedString::from(format!("已连接 {} 张网卡", connected.len())),
                ))
                .child(status_pill(
                    pal,
                    SharedString::from(format!("TCP 活跃 {}", self.snap.net.tcp_established)),
                    pal.accent,
                )),
        );

        if connected.is_empty() {
            body = body.child(
                div()
                    .mt_2()
                    .py_2()
                    .text_size(px(11.5))
                    .text_color(pal.text_muted)
                    .child("无已连接的网卡"),
            );
        } else {
            body = body.child(
                div()
                    .mt_2()
                    .flex_col()
                    .gap_2()
                    .children(connected.iter().map(|i| {
                        div()
                            .flex_col()
                            .gap_1()
                            .child(
                                // 行 1：名称（左）+ 协商速度（右）
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .min_w(px(0.0))
                                            .truncate()
                                            .text_size(px(12.5))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(pal.text)
                                            .child(SharedString::from(i.name.clone())),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(px(10.5))
                                            .text_color(pal.text_dim)
                                            .child(SharedString::from(i.link_speed.clone())),
                                    ),
                            )
                            .child(
                                // 行 2：IPv4（左）+ 实时 ↓↑ 速率（右）
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .truncate()
                                            .text_size(px(11.0))
                                            .text_color(pal.text_muted)
                                            .child(SharedString::from(if i.ipv4.is_empty() {
                                                "IPv4 —".to_string()
                                            } else {
                                                format!("IPv4 {}", i.ipv4)
                                            })),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(px(11.0))
                                            .text_color(pal.text_muted)
                                            .child(SharedString::from(format!(
                                                "↓ {}",
                                                fmt_kbps(i.rx_kbps.value_or(0.0))
                                            ))),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(px(11.0))
                                            .text_color(pal.text_muted)
                                            .child(SharedString::from(format!(
                                                "↑ {}",
                                                fmt_kbps(i.tx_kbps.value_or(0.0))
                                            ))),
                                    ),
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

        // 行装配：等宽 flex（禁 grid —— 见文件头布局纪律）。
        // v2.11.0：上行 = CPU / 内存 / GPU / 网络速率趋势 一行四列；
        //          下行 = 磁盘存储 · SMART 健康 | 网络流量 左右两列。
        let cpu = self.cpu_card(&pal, &s).flex_1().min_w(px(0.0));
        let mem = self.mem_card(&pal, &s).flex_1().min_w(px(0.0));
        let gpu = self.gpu_card(&pal, &s).flex_1().min_w(px(0.0));
        let disk = self.disk_card(&pal, cx).flex_1().min_w(px(0.0));
        let trend = self.net_trend_card(&pal).flex_1().min_w(px(0.0));
        let traffic = self.net_traffic_card(&pal).flex_1().min_w(px(0.0));
        let row = |a: gpui::Div, b: gpui::Div| div().flex().gap(px(ROW_GAP)).child(a).child(b);
        let row4 = |a: gpui::Div, b: gpui::Div, c: gpui::Div, d: gpui::Div| {
            div()
                .flex()
                .gap(px(ROW_GAP))
                .child(a)
                .child(b)
                .child(c)
                .child(d)
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
            .child(row4(cpu, mem, gpu, trend))
            .child(row(disk, traffic))
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
    (
        sv.summary.healthy,
        risk && sv.summary.healthy,
        sv.summary.detail.clone(),
    )
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

/// 当前网络总量速率 (下行, 上行 KB/s)（统一快照 per-NIC 求和；与趋势序列同源）
fn net_total_now(snap: &SensorSnapshot) -> (f32, f32) {
    snap.net
        .interfaces
        .iter()
        .fold((0.0f32, 0.0f32), |(rx, tx), i| {
            (rx + i.rx_kbps.value_or(0.0), tx + i.tx_kbps.value_or(0.0))
        })
}

/// 取 60s 窗口内的数值序列（趋势图渲染输入）
fn window_vals(points: &[HistoryPoint]) -> Vec<f32> {
    let cutoff = now_ms().saturating_sub(CHART_WINDOW_MS);
    points
        .iter()
        .filter(|p| p.t >= cutoff)
        .map(|p| p.v)
        .collect()
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
