// secm-app::pages::hardware — 硬件检测页（磁盘清单 + S.M.A.R.T 详情）
// 磁盘枚举走 secm-core::hardware（datasource IOCTL→WMI 三级降级链），
// 点击行展开 SMART 详情（IOCTL 采集可能耗时数百 ms → BackgroundExecutor 后台执行）。
// 呈现层统一接入 crate::ui::page 布局框架，色板取自 pi_clone::theme::Palette（明暗随壳联动）。

use gpui::prelude::*;
use gpui::{div, px, SharedString, Window, Context, Render};
use secm_core::hardware::{self, DiskListItem, DiskSmartView};

use crate::pi_clone::theme::{Appearance, Palette};
use crate::ui::page::{
    banner, button, card, page_header, page_root, status_pill, table_empty, table_head, table_row,
    BannerKind, ButtonKind, ColWidth,
};

pub struct HardwareView {
    disks: Vec<DiskListItem>,
    /// 磁盘列表加载中（IOCTL 枚举慢，后台执行）
    loading_disks: bool,
    /// 正在读取 SMART 的磁盘 id（转圈提示）
    loading_smart: Option<String>,
    /// 展开的磁盘 id → SMART 视图
    smart: std::collections::HashMap<String, DiskSmartView>,
    /// 读取错误反馈
    error: String,
    /// 页面外观，随壳主题联动
    appearance: Appearance,
    /// 页面滚动状态（GPUI 0.2 滚轮需 track_scroll 手动驱动，见 ui::page::page_root）
    page_scroll: gpui::ScrollHandle,
}

impl HardwareView {
    pub fn new(appearance: Appearance, cx: &mut Context<Self>) -> Self {
        log::info!("硬件检测 · 页面已打开");
        let mut v = Self {
            disks: Vec::new(),
            loading_disks: false,
            loading_smart: None,
            smart: std::collections::HashMap::new(),
            error: String::new(),
            appearance,
            page_scroll: gpui::ScrollHandle::new(),
        };
        v.refresh_disks(cx);
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

    /// 后台枚举磁盘（IOCTL 打开 0..64 物理盘 + WMI 回退，慢；结果回填）
    fn refresh_disks(&mut self, cx: &mut Context<Self>) {
        if self.loading_disks {
            return;
        }
        self.loading_disks = true;
        cx.notify();

        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let disks = exec.spawn(async move { hardware::list_disks() }).await;
            // 全链路行为日志：磁盘枚举返回信息
            log::info!("硬件检测 · 磁盘枚举完成，共 {} 块物理盘", disks.len());
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.loading_disks = false;
                    this.disks = disks;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 后台刷新磁盘清单并清除已展开 SMART/错误
    fn refresh(&mut self, cx: &mut Context<Self>) {
        // UI 侧日志：用户点击刷新磁盘列表
        log::info!("硬件检测 · 触发磁盘列表刷新");
        self.smart.clear();
        self.error.clear();
        self.refresh_disks(cx);
        cx.notify();
    }

    /// 后台读取 SMART（展开/刷新详情）
    fn load_smart(&mut self, disk_id: &str, cx: &mut Context<Self>) {
        if self.loading_smart.is_some() {
            return;
        }
        // UI 侧日志：用户展开磁盘详情（触发读取）
        log::info!("硬件检测 · 读取磁盘 {} S.M.A.R.T 详情", disk_id);
        let id = disk_id.to_string();
        self.loading_smart = Some(id.clone());
        self.error.clear();
        cx.notify();

        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx: &mut gpui::AsyncApp| {
            // 阻塞 IOCTL 磁盘读取委托后台线程池
            let exec = cx.background_executor().clone();
            let id2 = id.clone();
            let result = exec.spawn(async move { hardware::read_smart(&id2) }).await;

            // UI 侧日志：SMART 读取结果（磁盘健康摘要）
            match &result {
                Ok(sv) => log::info!(
                    "硬件检测 · 磁盘 {} S.M.A.R.T 读取成功：{}",
                    id,
                    sv.summary.title
                ),
                Err(e) => log::warn!("硬件检测 · 磁盘 {} S.M.A.R.T 读取失败: {}", id, e),
            }

            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.loading_smart = None;
                    match result {
                        Ok(smart_view) => {
                            this.smart.insert(id.clone(), smart_view);
                        }
                        Err(e) => {
                            this.error = format!("读取磁盘 {} S.M.A.R.T 失败：{}", id, e);
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 展开/收起详情
    fn toggle_detail(&mut self, disk_id: &str, cx: &mut Context<Self>) {
        if self.smart.contains_key(disk_id) {
            // 已展开 → 收起
            self.smart.remove(disk_id);
            cx.notify();
        } else {
            // 未展开 → 后台读取
            let id = disk_id.to_string();
            self.load_smart(&id, cx);
        }
    }
}

impl Render for HardwareView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.pal();
        let disks: Vec<DiskListItem> = self.disks.clone();
        let total_gb: f64 = disks.iter().map(|d| d.size_gb).sum();
        let loading_disks = self.loading_disks;

        // 根容器：统一页面骨架（内边距/纵向节奏/超高滚动/页面底色）
        page_root(&pal, "hardware-page-root", &self.page_scroll, &cx.entity())
            // 页头：左标题 + 副标题（随加载态切换），右侧整合刷新按钮
            .child(
                page_header(
                    &pal,
                    "硬件检测",
                    if loading_disks {
                        SharedString::from("正在枚举物理磁盘…")
                    } else {
                        SharedString::from(format!(
                            "{} 块磁盘 · 合计 {:.1} GB · 点击行展开 S.M.A.R.T 详情",
                            disks.len(),
                            total_gb
                        ))
                    },
                )
                .child(
                    button(&pal, ButtonKind::Secondary)
                        .id("hw-refresh")
                        .child(if loading_disks {
                            "检测中…"
                        } else {
                            "刷新磁盘列表"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.refresh(cx);
                        })),
                ),
            )
            // 错误消息
            .when(!self.error.is_empty(), |s| {
                let msg = self.error.clone();
                s.child(banner(&pal, BannerKind::Danger, msg))
            })
            // 磁盘表：卡片容器 + 规格化表头/数据行（列宽规格两侧一致）
            .child(
                card(&pal)
                    .child(table_head(
                        &pal,
                        &[
                            ("型号", ColWidth::Flex),
                            ("接口", ColWidth::Px(80.0)),
                            ("介质", ColWidth::Px(70.0)),
                            ("容量", ColWidth::Px(100.0)),
                            ("健康状态", ColWidth::Px(140.0)),
                            ("操作", ColWidth::Px(80.0)),
                        ],
                    ))
                    .children(disks.iter().map(|d| self.disk_block(&pal, d, cx)))
                    // 空态提示（纯呈现，无业务逻辑）
                    .when(disks.is_empty(), |s| {
                        s.child(table_empty(
                            &pal,
                            if loading_disks {
                                "正在枚举物理磁盘…"
                            } else {
                                "未检测到物理磁盘"
                            },
                        ))
                    }),
            )
    }
}

impl HardwareView {
    /// 一行磁盘 + （可展开）SMART 详情块
    fn disk_block(
        &self,
        pal: &Palette,
        d: &DiskListItem,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let disk = d.clone();
        let expanded = self.smart.contains_key(&d.id);
        let loading = self.loading_smart.as_deref() == Some(d.id.as_str());
        let arrow = if loading { "…" } else if expanded { "▲ 收起" } else { "▼ 详情" };

        // 数据行骨架（列宽规格与表头完全一致：Flex / 80 / 70 / 100 / 140 / 80）
        let row = table_row(pal)
            .id(SharedString::from(format!("hw-disk-{}", d.id)))
            .cursor_pointer()
            .hover(|s| s.bg(pal.bg_hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                let id = disk.id.clone();
                this.toggle_detail(&id, cx);
            }))
            // 型号
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(12.5))
                    .text_color(pal.text)
                    .child(d.model.clone()),
            )
            // 接口
            .child(
                div()
                    .w(px(80.0))
                    .flex_none()
                    .text_size(px(11.5))
                    .text_color(pal.text_muted)
                    .child(d.interface_type.clone()),
            )
            // 介质
            .child(
                div()
                    .w(px(70.0))
                    .flex_none()
                    .text_size(px(11.5))
                    .text_color(pal.text_muted)
                    .child(d.media_type.clone()),
            )
            // 容量
            .child(
                div()
                    .w(px(100.0))
                    .flex_none()
                    .text_size(px(12.0))
                    .text_color(pal.text_muted)
                    .child(format!("{:.1} GB", d.size_gb)),
            );

        // 健康状态列（展开后以 SMART 摘要为准）
        let row = match self.smart.get(&d.id) {
            Some(sv) => {
                let healthy = sv.summary.healthy;
                let title = sv.summary.title.clone();
                // 健康状态点色：健康绿 / 异常红
                let color = if healthy { pal.success } else { pal.danger };
                row.child(
                    div()
                        .w(px(140.0))
                        .flex_none()
                        .child(status_pill(pal, title, color)),
                )
            }
            None => row.child(
                div()
                    .w(px(140.0))
                    .flex_none()
                    .text_size(px(11.5))
                    .text_color(pal.text_muted)
                    .child(if loading { "读取中…" } else { "待检测" }),
            ),
        };

        // 操作列（展开箭头/收起）
        let row = row.child(
            div()
                .w(px(80.0))
                .flex_none()
                .text_size(px(11.5))
                .text_color(pal.accent)
                .child(arrow),
        );

        let mut block = div().flex_col().child(row.into_any_element());
        if let Some(sv) = self.smart.get(&d.id) {
            block = block.child(self.smart_detail(pal, sv).into_any_element());
        }
        block
    }

    /// SMART 详情子块（健康摘要 + 数据行/属性表）
    fn smart_detail(&self, pal: &Palette, sv: &DiskSmartView) -> impl IntoElement {
        let d = &sv.disk;
        let summary = &sv.summary;

        // 来源标签
        let source_label = match d.source.as_str() {
            "ioctl" => "IOCTL 全量采集",
            "wmi" => "WMI 降级采集",
            other => other,
        };

        // 详情行（因 DiskSmartData 字段随来源不同，统一渲染为关键字段行）
        let mut detail_rows: Vec<(String, String)> = Vec::new();
        detail_rows.push(("接口".into(), d.interface_type.clone()));
        detail_rows.push(("介质".into(), d.media_type.clone()));
        detail_rows.push(("数据来源".into(), source_label.to_string()));

        if let Some(nv) = &d.nvme_health {
            detail_rows.push(("温度".into(), format!("{} °C", nv.temperature_c)));
            detail_rows.push(("寿命已用".into(), format!("{}%", nv.percentage_used)));
            detail_rows.push(("备用空间".into(), format!("{}%", nv.available_spare)));
            detail_rows.push(("通电时间".into(), format!("{} 小时", nv.power_on_hours)));
            detail_rows.push(("通电次数".into(), nv.power_cycles.to_string()));
            detail_rows.push(("不安全关机".into(), nv.unsafe_shutdowns.to_string()));
            detail_rows.push(("媒体错误".into(), nv.media_errors.to_string()));
        }

        let mut body = div()
            .flex_col()
            .gap_2()
            .px_5()
            .py_3()
            .bg(pal.bg_subtle)
            .border_b_1()
            .border_color(pal.border);

        // 健康告警行
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(if summary.healthy { pal.success } else { pal.danger })
                        .child(if summary.healthy { "✓ 健康" } else { "⚠ 告警" }),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(pal.text_muted)
                        .child(summary.detail.clone()),
                ),
        );

        // ATA 属性表（attributes 非空时）
        if !d.attributes.is_empty() {
            body = body.child(
                div()
                    .text_size(px(11.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(pal.text_muted)
                    .child("S.M.A.R.T 属性"),
            );
            body = body.child(
                div()
                    .flex_col()
                    .gap_0p5()
                    .children(d.attributes.iter().map(|a| {
                        let status_color = match a.status.as_str() {
                            "OK" => pal.success,
                            "FAILING" | "FAILED" => pal.danger,
                            _ => pal.text_muted,
                        };
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(180.0))
                                    .text_size(px(11.5))
                                    .text_color(pal.text)
                                    .child(a.name.clone()),
                            )
                            .child(
                                div()
                                    .w(px(80.0))
                                    .text_size(px(11.5))
                                    .text_color(pal.text_muted)
                                    .child(a.raw.to_string()),
                            )
                            .child(
                                div()
                                    .w(px(100.0))
                                    .text_size(px(11.5))
                                    .text_color(pal.text_muted)
                                    .child(format!(
                                        "当前 {} / 最差 {} / 阈值 {}",
                                        a.value, a.worst, a.threshold
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(status_color)
                                    .child(a.status.clone()),
                            )
                    })),
            );
        }

        // 关键字段行（NVMe 或无 attributes 时）
        if d.attributes.is_empty() && !detail_rows.is_empty() {
            body = body.child(
                div()
                    .flex_col()
                    .gap_0p5()
                    .children(detail_rows.iter().map(|(k, v)| {
                        let k = k.clone();
                        let v = v.clone();
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(140.0))
                                    .text_size(px(11.5))
                                    .text_color(pal.text_muted)
                                    .child(k),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(pal.text)
                                    .child(v),
                            )
                    })),
            );
        }

        body
    }
}
