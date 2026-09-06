// secm-app::pages::network — 网络诊断页（Phase 4）
// 网站可达性 / TCP 端口 / DNS 解析：阻塞网络 IO 放 BackgroundExecutor，完成后回 UI 更新。

use gpui::prelude::*;
use gpui::{div, px, rgb, SharedString, Window, Context, Render};
use secm_core::network as net;

use crate::theme::Theme;

/// 单行诊断结果
#[derive(Clone)]
struct DiagRow {
    name: String,
    ok: bool,
    detail: String,
}

pub struct NetworkView {
    running: bool,
    status: String,
    site_rows: Vec<DiagRow>,
    port_rows: Vec<DiagRow>,
    dns_rows: Vec<DiagRow>,
}

impl NetworkView {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        log::info!("网络诊断 · 页面已打开");
        Self {
            running: false,
            status: String::from("就绪 — 点击「运行诊断」开始检测"),
            site_rows: Vec::new(),
            port_rows: Vec::new(),
            dns_rows: Vec::new(),
        }
    }

    /// 开始全量诊断：后台线程并行执行三组探测，完成回 UI 一次更新
    fn run_diagnostics(&mut self, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        self.running = true;
        self.status = "诊断执行中…（请稍候）".to_string();
        self.site_rows.clear();
        self.port_rows.clear();
        self.dns_rows.clear();
        // UI 侧日志：用户点击运行诊断（触发点）
        log::info!("网络诊断 · 开始运行网络诊断");
        cx.notify();

        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx: &mut gpui::AsyncApp| {
            // 阻塞网络探测：委托给 BackgroundExecutor（线程池）执行
            let exec = cx.background_executor().clone();
            let (site_rows, port_rows, dns_rows) = exec
                .spawn(async move {
                    let sites: Vec<String> = net::default_probe_sites()
                        .iter()
                        .map(|s| s.to_string())
                        .collect();
                    let target = net::default_port_target().to_string();
                    let ports = [80u16, 443, 22, 3389];

                    // ---- 网站可达性：并行探测，最长 ~单站超时 ----
                    let site_rows: Vec<DiagRow> = std::thread::scope(|s| {
                        let mut handles = Vec::new();
                        for url in &sites {
                            let url = url.clone();
                            handles.push(s.spawn(move || {
                                let url2 = url.clone();
                                match net::http_probe_status(&url2, 5000) {
                                    Some(code) => DiagRow {
                                        name: url,
                                        ok: true,
                                        detail: format!("HTTP {} · 可达", code),
                                    },
                                    None => DiagRow {
                                        name: url,
                                        ok: false,
                                        detail: "连接失败 / 超时".to_string(),
                                    },
                                }
                            }));
                        }
                        handles.into_iter().filter_map(|h| h.join().ok()).collect()
                    });

                    // ---- 端口连通性：并行探测 ----
                    let port_rows: Vec<DiagRow> = std::thread::scope(|s| {
                        let mut handles = Vec::new();
                        for p in ports {
                            let target = target.clone();
                            handles.push(s.spawn(move || match net::probe_tcp(&target, p, 3000) {
                                Ok(true) => DiagRow {
                                    name: format!("{}:{}", target, p),
                                    ok: true,
                                    detail: "端口开放".to_string(),
                                },
                                Ok(false) => DiagRow {
                                    name: format!("{}:{}", target, p),
                                    ok: false,
                                    detail: "端口关闭 / 不可达".to_string(),
                                },
                                Err(e) => DiagRow {
                                    name: format!("{}:{}", target, p),
                                    ok: false,
                                    detail: e,
                                },
                            }));
                        }
                        handles.into_iter().filter_map(|h| h.join().ok()).collect()
                    });

                    // ---- DNS 解析：对每个站点取主机名解析 ----
                    let dns_rows: Vec<DiagRow> = sites
                        .iter()
                        .map(|url| {
                            let host = url
                                .trim_start_matches("https://")
                                .trim_start_matches("http://")
                                .trim_end_matches('/')
                                .to_string();
                            match net::resolve_host(&host) {
                                Ok((v4, v6)) => {
                                    let mut detail = String::new();
                                    if !v4.is_empty() {
                                        detail.push_str(&format!("{} 个 IPv4", v4.len()));
                                    }
                                    if !v6.is_empty() {
                                        if !detail.is_empty() {
                                            detail.push_str(" · ");
                                        }
                                        detail.push_str(&format!("{} 个 IPv6", v6.len()));
                                    }
                                    if let Some(ip) = v4.first() {
                                        detail.push_str(&format!("（{}）", ip));
                                    }
                                    DiagRow { name: host, ok: true, detail }
                                }
                                Err(e) => DiagRow { name: host, ok: false, detail: e },
                            }
                        })
                        .collect();

                    (site_rows, port_rows, dns_rows)
                })
                .await;

            // UI 侧日志：诊断完成，记录通过行数
            {
                let ok_total = site_rows.iter().filter(|r| r.ok).count()
                    + port_rows.iter().filter(|r| r.ok).count()
                    + dns_rows.iter().filter(|r| r.ok).count();
                let all = site_rows.len() + port_rows.len() + dns_rows.len();
                log::info!("网络诊断 · 检测完成：{} / {} 行通过", ok_total, all);
            }
            // 回到 UI 线程，把结果写回视图
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.site_rows = site_rows;
                    this.port_rows = port_rows;
                    this.dns_rows = dns_rows;
                    this.running = false;
                    let ok_total = this.site_rows.iter().filter(|r| r.ok).count()
                        + this.port_rows.iter().filter(|r| r.ok).count()
                        + this.dns_rows.iter().filter(|r| r.ok).count();
                    let all = this.site_rows.len()
                        + this.port_rows.len()
                        + this.dns_rows.len();
                    this.status = format!(
                        "诊断完成：{} / {} 项通过",
                        ok_total, all
                    );
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 结果行列表（三张卡片共用渲染）
    fn rows(&self, theme: &Theme, rows: &[DiagRow]) -> impl IntoElement {
        div()
            .children(rows.iter().map(|r| {
                let name = r.name.clone();
                let detail = r.detail.clone();
                div()
                    .id(SharedString::from(format!("net-row-{}", name)))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .py_1p5()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.5))
                            .text_color(theme.text)
                            .child(name),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().size(px(6.0)).rounded_full().bg(if r.ok { theme.success } else { theme.danger }))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(if r.ok { rgb(0x9be8b4) } else { theme.danger })
                                    .child(detail),
                            ),
                    )
            }))
    }
}

impl Render for NetworkView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::dark();
        let running = self.running;

        div()
            .flex_col()
            .size_full()
            .p_6()
            .gap_4()
            // 页头
            .child(crate::ui::page_header(
                &theme,
                "网络诊断",
                "网站可达性 · TCP 端口 · DNS 解析（阻塞探测在后台线程执行）",
            ))
            // 操作行：运行按钮 + 状态
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("net-run")
                            .px_4()
                            .py_1p5()
                            .rounded_md()
                            .cursor_pointer()
                            .when(running, |s| s.bg(theme.panel_hover).text_color(theme.text_muted))
                            .when(!running, |s| {
                                s.bg(theme.brand)
                                    .hover(|s| s.bg(rgb(0x3d66e6)))
                                    .text_color(rgb(0xffffff))
                            })
                            .text_size(px(13.0))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.run_diagnostics(cx);
                            }))
                            .child(if running { "诊断中…" } else { "运行诊断" }),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(if running { theme.info } else { theme.text_muted })
                            .child(self.status.clone()),
                    ),
            )
            // 三组结果卡
            .child(self.diag_card(&theme, "网站可达性", &self.site_rows))
            .child(self.diag_card(&theme, "端口连通性（目标：www.baidu.com）", &self.port_rows))
            .child(self.diag_card(&theme, "DNS 解析", &self.dns_rows))
    }
}

impl NetworkView {
    fn diag_card(&self, theme: &Theme, title: &str, rows: &[DiagRow]) -> impl IntoElement {
        crate::ui::table_container(theme)
            .child(
                div()
                    .px_4()
                    .py_2()
                    .text_size(px(13.5))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(title.to_string()),
            )
            .when(rows.is_empty(), |s| {
                s.child(crate::ui::table_empty(theme, if self.running { "检测中…" } else { "点击「运行诊断」开始检测" }))
            })
            .when(!rows.is_empty(), |s| {
                let cloned: Vec<DiagRow> = rows.to_vec();
                s.child(self.rows(theme, &cloned))
            })
    }
}
