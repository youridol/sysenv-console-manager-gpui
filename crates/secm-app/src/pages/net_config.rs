// secm-app::pages::net_config — 网络配置页（Phase 4）
// 适配器枚举 + 当前配置展示 + 常见修改（DHCP/IPv4/DNS/MAC，netsh 后台执行）。
// 修改类操作需管理员权限：命令层 is_admin 门禁返回中文错误。
// 渲染层统一走 crate::ui::page 布局框架，色板取 pi_clone::theme::Palette（明暗随壳联动）。

use gpui::prelude::*;
use gpui::{div, px, Context, Entity, Render, SharedString, WeakEntity, Window};
use secm_core::net_config::{self, ApplyStep};
use secm_core::netif::{self, AdapterConfig};

use crate::pi_clone::theme::{Appearance, Palette};
use crate::ui::page::{
    banner, button, card, card_body, card_divider, card_header, field_label, kv_row_w, page_header,
    page_root, status_pill, BannerKind, ButtonKind,
};
use crate::ui::text_input::{ChangeText, TextField};

pub struct NetConfigView {
    /// 页面外观，随壳主题联动
    appearance: Appearance,
    /// 页面滚动状态（GPUI 0.2 滚轮需 track_scroll 手动驱动，见 ui::page::page_root）
    page_scroll: gpui::ScrollHandle,
    adapters: Vec<AdapterConfig>,
    /// 当前选中的接口名
    selected: Option<String>,
    /// 最近操作结果（步骤列表展示）
    steps: Vec<ApplyStep>,
    /// 状态文本（成功提示等）
    status: String,
    /// 管理员标记
    admin: bool,
    /// 适配器列表加载中
    loading_adapters: bool,
    /// 网络配置应用进行中（netsh 后台执行，互斥防连点）
    applying: bool,
    // 修改用输入框
    mac_input: Entity<TextField>,
    dns_input: Entity<TextField>,
    ipv4_input: Entity<TextField>,
    mask_input: Entity<TextField>,
    gateway_input: Entity<TextField>,
    ipv6_input: Entity<TextField>,
    ipv6_gw_input: Entity<TextField>,
}

impl NetConfigView {
    pub fn new(appearance: Appearance, cx: &mut Context<Self>) -> Self {
        let mac_input = cx.new(|cx| TextField::new("", "AA:BB:CC:DD:EE:FF", cx));
        let dns_input = cx.new(|cx| TextField::new("", "8.8.8.8, 114.114.114.114（逗号分隔）", cx));
        let ipv4_input = cx.new(|cx| TextField::new("", "192.168.1.100", cx));
        let mask_input = cx.new(|cx| TextField::new("", "255.255.255.0", cx));
        let gateway_input = cx.new(|cx| TextField::new("", "192.168.1.1（可选）", cx));
        let ipv6_input = cx.new(|cx| TextField::new("", "2001:db8::2（可选）", cx));
        let ipv6_gw_input = cx.new(|cx| TextField::new("", "2001:db8::1（可选）", cx));

        // 订阅所有输入框值变更 → 刷新页面（便于按钮读取最新值）
        for field in [
            mac_input.clone(),
            dns_input.clone(),
            ipv4_input.clone(),
            mask_input.clone(),
            gateway_input.clone(),
            ipv6_input.clone(),
            ipv6_gw_input.clone(),
        ] {
            cx.subscribe(
                &field,
                |_this, _field: Entity<TextField>, _ev: &ChangeText, cx| {
                    cx.notify();
                },
            )
            .detach();
        }

        let mut v = Self {
            appearance,
            page_scroll: gpui::ScrollHandle::new(),
            adapters: Vec::new(),
            selected: None,
            steps: Vec::new(),
            status: String::new(),
            admin: secm_core::settings::is_admin(),
            loading_adapters: false,
            applying: false,
            mac_input,
            dns_input,
            ipv4_input,
            mask_input,
            gateway_input,
            ipv6_input,
            ipv6_gw_input,
        };
        log::info!("网络配置 · 页面已打开（管理员: {}）", v.admin);
        // 后台枚举适配器并自动选中首个 Up 接口
        v.refresh(cx);
        v
    }

    /// 后台枚举适配器（GetAdaptersAddresses 网络 API），完成后保持/更新选中
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.loading_adapters {
            return;
        }
        self.loading_adapters = true;
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                let adapters = exec
                    .spawn(async move { netif::list_adapters().unwrap_or_default() })
                    .await;
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.loading_adapters = false;
                        this.adapters = adapters;
                        // 保持当前选中（若仍存在），否则自动选首个 Up 接口
                        let keep = this
                            .selected
                            .as_ref()
                            .map(|cur| this.adapters.iter().any(|a| &a.name == cur))
                            .unwrap_or(false);
                        if !keep {
                            this.selected = this
                                .adapters
                                .iter()
                                .find(|a| a.status == "Up")
                                .map(|a| a.name.clone())
                                .or_else(|| this.adapters.first().map(|a| a.name.clone()));
                        }
                        cx.notify();
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    fn selected_adapter(&self) -> Option<&AdapterConfig> {
        self.selected
            .as_ref()
            .and_then(|name| self.adapters.iter().find(|a| &a.name == name))
    }

    /// 选中适配器并预填 MAC 输入框
    fn select_adapter(&mut self, name: &str, cx: &mut Context<Self>) {
        self.selected = Some(name.to_string());
        self.steps.clear();
        self.status.clear();
        // UI 侧日志：用户选中适配器
        log::info!("网络配置 · 选中适配器 {}", name);
        // 预填 MAC 输入框为当前地址（便于直接修改）
        if let Some(a) = self.adapters.iter().find(|a| a.name == name) {
            if let Some(mac) = &a.mac {
                let mac = mac.clone();
                let mac_input = self.mac_input.clone();
                mac_input.update(cx, |f, cx| {
                    f.set_value(mac, cx);
                });
            }
        }
        cx.notify();
    }

    fn apply_result(&mut self, r: net_config::NetworkConfigApplyResult, cx: &mut Context<Self>) {
        self.steps = r.steps;
        self.status = if r.all_ok {
            "全部步骤执行成功".to_string()
        } else {
            "部分步骤失败（见下方明细）".to_string()
        };
        cx.notify();
    }

    /// 后台执行网络配置应用（netsh 串行 2-7 个进程，需数百 ms~秒级 → 绝不上主线程）
    fn run_apply(&mut self, req: net_config::NetworkConfigRequest, cx: &mut Context<Self>) {
        if self.applying {
            return;
        }
        self.applying = true;
        self.status = "正在应用网络配置（netsh 执行中，可能需要几秒）…".to_string();
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let exec = cx.background_executor().clone();
                let result = exec
                    .spawn(async move { net_config::apply_network_config(&req) })
                    .await;
                // UI 侧日志：网络配置应用结果（部分失败用 warn）
                if result.all_ok {
                    log::info!("网络配置 · 配置应用成功（{} 项）", result.steps.len());
                } else {
                    let failed: Vec<&str> = result
                        .steps
                        .iter()
                        .filter(|s| !s.ok)
                        .map(|s| s.name.as_str())
                        .collect();
                    log::warn!(
                        "网络配置 · 配置应用失败步骤（{}）: {}",
                        failed.len(),
                        failed.join("，")
                    );
                }
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.applying = false;
                        this.apply_result(result, cx);
                    })
                    .ok();
                    // netsh 生效后延迟后台刷新配置
                    view.update(cx, |this, cx| {
                        this.schedule_refresh(cx);
                    })
                    .ok();
                }
            },
        )
        .detach();
    }

    /// 一键切换 IPv4 DHCP
    fn set_dhcp(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.selected.clone() else {
            self.status = "请先选择一个网络适配器".to_string();
            log::warn!("网络配置 · 切换 DHCP 失败：未选择适配器");
            cx.notify();
            return;
        };
        if !self.admin {
            self.status = "需要管理员权限执行网络配置修改".to_string();
            log::warn!("网络配置 · 切换 DHCP 失败：需要管理员权限");
            cx.notify();
            return;
        }
        log::info!("网络配置 · 切换适配器 {} 为 DHCP", name);
        let req = net_config::NetworkConfigRequest {
            ifname: name,
            mode_v4: "dhcp".into(),
            ipv4s: vec![],
            mode_dns4: "dhcp".into(),
            dns4: vec![],
            mode_v6: "dhcp".into(),
            ipv6: None,
            ipv6_gateway: None,
            mode_dns6: "dhcp".into(),
            dns6: vec![],
        };
        self.run_apply(req, cx);
    }

    /// 应用静态 IPv4（地址/掩码/网关）+ 静态 DNS
    fn apply_static_v4(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.selected.clone() else {
            self.status = "请先选择一个网络适配器".to_string();
            log::warn!("网络配置 · 应用静态 IPv4 失败：未选择适配器");
            cx.notify();
            return;
        };
        if !self.admin {
            self.status = "需要管理员权限执行网络配置修改".to_string();
            log::warn!("网络配置 · 应用静态 IPv4 失败：需要管理员权限");
            cx.notify();
            return;
        }
        let ip = self.ipv4_input.read(cx).value().to_string();
        let mask = self.mask_input.read(cx).value().to_string();
        let gateway = self.gateway_input.read(cx).value().to_string();
        let dns_text = self.dns_input.read(cx).value().to_string();

        if ip.trim().is_empty() || mask.trim().is_empty() {
            self.status = "请填写 IPv4 地址与子网掩码".to_string();
            log::warn!("网络配置 · 应用静态 IPv4 失败：未填写地址与掩码");
            cx.notify();
            return;
        }
        log::info!(
            "网络配置 · 应用静态 IPv4 {} / {} 到适配器 {}",
            ip.trim(),
            mask.trim(),
            name
        );
        // DNS 解析逗号/空格分隔列表
        let dns_list: Vec<String> = dns_text
            .split([',', '，', ' '])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let req = net_config::NetworkConfigRequest {
            ifname: name.clone(),
            mode_v4: "static".into(),
            ipv4s: vec![net_config::Ipv4Entry {
                ip: ip.trim().to_string(),
                mask: mask.trim().to_string(),
                gateway: if gateway.trim().is_empty() {
                    None
                } else {
                    Some(gateway.trim().to_string())
                },
            }],
            mode_dns4: if dns_list.is_empty() {
                "dhcp".into()
            } else {
                "static".into()
            },
            dns4: dns_list,
            mode_v6: "dhcp".into(),
            ipv6: None,
            ipv6_gateway: None,
            mode_dns6: "dhcp".into(),
            dns6: vec![],
        };
        self.run_apply(req, cx);
    }

    /// 应用静态 IPv6 地址/网关
    fn apply_static_v6(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.selected.clone() else {
            self.status = "请先选择一个网络适配器".to_string();
            log::warn!("网络配置 · 应用静态 IPv6 失败：未选择适配器");
            cx.notify();
            return;
        };
        if !self.admin {
            self.status = "需要管理员权限执行网络配置修改".to_string();
            log::warn!("网络配置 · 应用静态 IPv6 失败：需要管理员权限");
            cx.notify();
            return;
        }
        let ipv6 = self.ipv6_input.read(cx).value().to_string();
        let gw6 = self.ipv6_gw_input.read(cx).value().to_string();

        log::info!("网络配置 · 应用静态 IPv6 {} 到适配器 {}", ipv6.trim(), name);
        let req = net_config::NetworkConfigRequest {
            ifname: name.clone(),
            mode_v4: "dhcp".into(),
            ipv4s: vec![],
            mode_dns4: "dhcp".into(),
            dns4: vec![],
            mode_v6: "static".into(),
            ipv6: if ipv6.trim().is_empty() {
                None
            } else {
                Some(ipv6.trim().to_string())
            },
            ipv6_gateway: if gw6.trim().is_empty() {
                None
            } else {
                Some(gw6.trim().to_string())
            },
            mode_dns6: "dhcp".into(),
            dns6: vec![],
        };
        self.run_apply(req, cx);
    }

    /// 应用 MAC 修改（注册表 + 重启网卡）
    fn apply_mac(&mut self, cx: &mut Context<Self>) {
        let Some(adapter) = self.selected_adapter().cloned() else {
            self.status = "请先选择一个网络适配器".to_string();
            log::warn!("网络配置 · 修改 MAC 失败：未选择适配器");
            cx.notify();
            return;
        };
        if !self.admin {
            self.status = "需要管理员权限执行 MAC 修改".to_string();
            log::warn!("网络配置 · 修改 MAC 失败：需要管理员权限");
            cx.notify();
            return;
        }
        let mac = self.mac_input.read(cx).value().to_string();
        if mac.trim().is_empty() {
            self.status = "请填写新的 MAC 地址".to_string();
            log::warn!("网络配置 · 修改 MAC 失败：未填写新 MAC");
            cx.notify();
            return;
        }
        log::info!(
            "网络配置 · 修改适配器 {} MAC 为 {}",
            adapter.name,
            mac.trim()
        );
        // 后台执行（netsh 禁启用网卡可能阻塞数百 ms）
        let weak = cx.entity().downgrade();
        let mac_v = mac.trim().to_string();
        let name_v = adapter.name.clone();
        let guid_v = adapter.guid.clone();
        cx.spawn(async move |_, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let mac2 = mac_v.clone();
            let name2 = name_v.clone();
            let guid2 = guid_v.clone();
            let result = exec
                .spawn(async move { net_config::set_network_mac(&name2, &mac2, guid2.as_deref()) })
                .await;
            // UI 侧日志：MAC 修改结果
            match &result {
                Ok(msg) => log::info!("网络配置 · 适配器 {} MAC 修改成功：{}", name_v, msg),
                Err(e) => log::warn!("网络配置 · 适配器 {} MAC 修改失败: {}", name_v, e),
            }
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    match result {
                        Ok(msg) => {
                            this.status = msg;
                            this.steps.clear();
                        }
                        Err(e) => {
                            this.status = format!("MAC 修改失败：{}", e);
                            this.steps.clear();
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        self.status = "正在应用 MAC…（网卡将短暂重启）".to_string();
        cx.notify();
    }

    /// 延迟后台刷新适配器配置（netsh 生效后）
    fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        cx.spawn(
            async move |_this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                gpui::Timer::after(std::time::Duration::from_millis(1500)).await;
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.refresh(cx);
                    })
                    .ok();
                }
            },
        )
        .detach();
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

    /// 基本信息行（label/value 对，空值显示占位符；统一走框架 kv_row_w，转自有值规避 'static 约束）
    fn kv(&self, pal: &Palette, label: &str, value: &str) -> impl IntoElement {
        kv_row_w(
            pal,
            110.0,
            label.to_string(),
            if value.is_empty() {
                "—".to_string()
            } else {
                value.to_string()
            },
        )
    }
}

impl Render for NetConfigView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.pal();
        let admin = self.admin;
        let selected_adapter = self.selected_adapter().cloned();
        let steps: Vec<ApplyStep> = self.steps.clone();
        let status_msg = self.status.clone();

        // 页面根容器 + 页头（右侧权限状态）+ 状态横幅 + 各功能卡
        page_root(
            &pal,
            "net_config-page-root",
            &self.page_scroll,
            &cx.entity(),
        )
        // 页头：标题/副标题 + 管理员权限状态徽标
        .child(
            page_header(&pal, "网络配置", "适配器配置 · DHCP/静态切换 · MAC").child(status_pill(
                &pal,
                if admin {
                    "管理员权限"
                } else {
                    "普通权限（修改需管理员）"
                },
                if admin { pal.success } else { pal.warning },
            )),
        )
        // 错误/状态消息
        .when(!status_msg.is_empty(), |s| {
            let msg = status_msg.clone();
            s.child(banner(&pal, BannerKind::Info, msg))
        })
        // 适配器选择 + 当前配置
        .child(self.adapter_panel(&pal, cx))
        // 修改操作
        .when(selected_adapter.is_some(), |s| {
            s.child(self.action_panel(&pal, cx))
        })
        // 步骤结果
        .when(!steps.is_empty(), |s| {
            s.child(self.steps_panel(&pal, &steps))
        })
    }
}

impl NetConfigView {
    /// 适配器卡：卡片头 + 分隔线 + 滚动列表（行结构保留，仅色板化）
    fn adapter_panel(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let adapters: Vec<AdapterConfig> = self.adapters.clone();
        let selected = self.selected.clone();

        card(pal)
            .child(card_header(pal, "网络适配器"))
            .child(card_divider(pal))
            .child(
                div()
                    .id("nc-adapter-scroll")
                    .flex_col()
                    .max_h(px(220.0))
                    .overflow_scroll()
                    .children(adapters.iter().map(|a| {
                        let name = a.name.clone();
                        let name_click = name.clone();
                        let desc = a.description.clone();
                        let status = a.status.clone();
                        let mac = a.mac.clone().unwrap_or_default();
                        let is_sel = selected.as_deref() == Some(name.as_str());
                        let has_ips = !a.ipv4.is_empty();
                        div()
                            .id(SharedString::from(format!("nc-adapter-{}", name)))
                            .flex_col()
                            .gap_0p5()
                            .px_4()
                            .py_2()
                            .cursor_pointer()
                            .when(is_sel, |s| s.bg(pal.bg_hover))
                            .border_b_1()
                            .border_color(pal.border)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_adapter(&name_click, cx);
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().size(px(6.0)).rounded_full().bg(
                                        if status == "Up" {
                                            pal.success
                                        } else {
                                            pal.text_muted
                                        },
                                    ))
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(13.0))
                                            .text_color(if is_sel { pal.accent } else { pal.text })
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(if status == "Up" {
                                                pal.success
                                            } else {
                                                pal.text_muted
                                            })
                                            .child(if status == "Up" {
                                                "已连接"
                                            } else {
                                                "已断开"
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(pal.text_muted)
                                    .child(SharedString::from(desc)),
                            )
                            .when(has_ips, |s| {
                                let ip = a.ipv4.first().cloned().unwrap_or_default();
                                s.child(
                                    div().text_size(px(11.0)).text_color(pal.text_muted).child(
                                        SharedString::from(format!("IP {} · MAC {}", ip, mac)),
                                    ),
                                )
                            })
                    })),
            )
    }

    /// 修改操作区：当前配置 / IPv4 / IPv6 / MAC 四张卡
    fn action_panel(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(a) = self.selected_adapter() else {
            return div().into_any_element();
        };
        let adapter = a.clone();

        let mut panel = div().flex_col().gap_4();

        // 当前配置卡：键值信息行列表
        let ipv4s = adapter.ipv4.join(", ");
        let dns4 = adapter.ipv4_dns.join(", ");
        let ipv6 = adapter.ipv6_link_local.join(", ");
        let mac = adapter.mac.clone().unwrap_or_else(|| "—".to_string());
        panel = panel.child(
            card(pal)
                .child(card_header(pal, "当前配置"))
                .child(card_divider(pal))
                .child(
                    card_body(pal)
                        .child(self.kv(pal, "接口名", &adapter.name))
                        .child(self.kv(pal, "描述", &adapter.description))
                        .child(self.kv(pal, "MAC 地址", &mac))
                        .child(self.kv(pal, "IPv4", &ipv4s))
                        .child(self.kv(pal, "IPv4 DNS", &dns4))
                        .child(self.kv(pal, "IPv6 链路本地", &ipv6)),
                ),
        );

        // IPv4 设置卡：DHCP/刷新按钮 + 地址/掩码/网关/DNS 输入 + 应用按钮
        panel = panel.child(
            card(pal)
                .child(card_header(pal, "IPv4 设置"))
                .child(card_divider(pal))
                .child(
                    card_body(pal)
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    button(pal, ButtonKind::Primary)
                                        .id("nc-dhcp")
                                        .child("切换为 DHCP（自动获取）")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.set_dhcp(cx);
                                        })),
                                )
                                .child(
                                    button(pal, ButtonKind::Secondary)
                                        .id("nc-refresh")
                                        .child("刷新配置")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.refresh(cx);
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_col()
                                        .flex_1()
                                        .gap_1()
                                        .child(field_label(pal, "IP 地址"))
                                        .child(self.ipv4_input.clone()),
                                )
                                .child(
                                    div()
                                        .flex_col()
                                        .flex_1()
                                        .gap_1()
                                        .child(field_label(pal, "子网掩码"))
                                        .child(self.mask_input.clone()),
                                )
                                .child(
                                    div()
                                        .flex_col()
                                        .flex_1()
                                        .gap_1()
                                        .child(field_label(pal, "默认网关（可选）"))
                                        .child(self.gateway_input.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex_col()
                                .gap_1()
                                .child(field_label(pal, "DNS 服务器（逗号分隔；留空=DHCP）"))
                                .child(self.dns_input.clone()),
                        )
                        .child(
                            div().flex().justify_end().child(
                                button(pal, ButtonKind::Secondary)
                                    .id("nc-static-v4")
                                    .child("应用静态 IPv4 + DNS")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.apply_static_v4(cx);
                                    })),
                            ),
                        ),
                ),
        );

        // IPv6 设置卡：地址/网关输入 + 应用按钮
        panel = panel.child(
            card(pal)
                .child(card_header(pal, "IPv6 设置（静态地址/网关）"))
                .child(card_divider(pal))
                .child(
                    card_body(pal)
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_col()
                                        .flex_1()
                                        .gap_1()
                                        .child(field_label(pal, "IPv6 地址（可选）"))
                                        .child(self.ipv6_input.clone()),
                                )
                                .child(
                                    div()
                                        .flex_col()
                                        .flex_1()
                                        .gap_1()
                                        .child(field_label(pal, "IPv6 网关（可选）"))
                                        .child(self.ipv6_gw_input.clone()),
                                ),
                        )
                        .child(
                            div().flex().justify_end().child(
                                button(pal, ButtonKind::Secondary)
                                    .id("nc-static-v6")
                                    .child("应用静态 IPv6")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.apply_static_v6(cx);
                                    })),
                            ),
                        ),
                ),
        );

        // MAC 设置卡：新 MAC 输入 + 危险操作按钮
        let mac = adapter.mac.clone().unwrap_or_default();
        panel = panel.child(
            card(pal)
                .child(card_header(pal, "MAC 地址修改（高级）"))
                .child(card_divider(pal))
                .child(
                    card_body(pal)
                        .child(
                            div()
                                .flex_col()
                                .gap_1()
                                .child(field_label(pal, "新 MAC（AA:BB:CC:DD:EE:FF）"))
                                .child(self.mac_input.clone()),
                        )
                        .when(!mac.is_empty(), |s| {
                            s.child(div().text_size(px(11.0)).text_color(pal.text_muted).child(
                                SharedString::from(format!(
                                    "当前：{}（修改后网卡将短暂重启）",
                                    mac
                                )),
                            ))
                        })
                        .child(
                            div().flex().justify_end().child(
                                button(pal, ButtonKind::Danger)
                                    .id("nc-mac")
                                    .child("修改 MAC")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.apply_mac(cx);
                                    })),
                            ),
                        ),
                ),
        );

        panel.into_any_element()
    }

    /// 步骤结果卡：逐行展示 netsh 应用结果（✓/✗ 行结构保留，仅色板化）
    fn steps_panel(&self, pal: &Palette, steps: &[ApplyStep]) -> impl IntoElement {
        card(pal)
            .child(card_header(pal, "执行结果"))
            .child(card_divider(pal))
            .children(steps.iter().map(|s| {
                let name = s.name.clone();
                let msg = s.message.clone();
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .py_1p5()
                    .border_b_1()
                    .border_color(pal.border)
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.5))
                            .text_color(pal.text)
                            .child(name),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(if s.ok { pal.success } else { pal.danger })
                                    .child(if s.ok { "✓ 成功" } else { "✗ 失败" }),
                            )
                            .when(!msg.is_empty() && !s.ok, |r| {
                                r.child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(pal.text_muted)
                                        .child(msg),
                                )
                            }),
                    )
            }))
    }
}
