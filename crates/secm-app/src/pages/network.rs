// secm-app::pages::network — 网络诊断页（ADR-0001/0002 全量重写，对齐原版 /network 全能力面）
//
// 结构（对齐原版 Network.tsx）：
//   ① DHCP 卡：服务器检测（探测+注册表基线合并）+ 深度检查（逐台 ping+拓扑判定）
//   ② 网站测试卡：4 默认站点 + 自定义增删改 + JSON 持久化 + 手动/自动检测（3~60s）
//   ③ 网络工具卡：Ping/Traceroute/Nslookup/NAT/Port/iperf3 六工具
//      共享参数（目标/端口/网络栈/协议）+ 每工具参数 + 流式输出区（自动滚底 + 清除）
// 执行模型（ADR-0002 §3）：
//   每任务独立 cmdId + 后台线程 → mpsc 无界通道 → UI 30ms 合并刷新（高频流不刷爆重绘）
//   停止 = cancel_command 置位 → 任务循环头/切片唤醒处退出 → DONE 哨兵冲刷 → 无孤儿任务
// 日志零丢失（ADR 强制项）：
//   页内输出 = 无界通道 + 页面 Vec 全量保存；右侧日志流 = 每事件 log:: 打点
//   （LogBuffer 2000 条环形 + 按天落盘，用户操作/状态变化/结果/取消/异常全可溯）。

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{div, point, px, Context, Entity, Render, ScrollHandle, SharedString, Window};
use secm_core::net_diag::cancel as cancel_reg;
use secm_core::net_diag::{self as diag, StreamEvent};

use crate::pi_clone::theme::{Appearance, Palette, TRANSPARENT};
use crate::ui::page::{
    badge, banner, button, button_sm, card, card_body, card_divider, card_header, field_label,
    page_header, page_root, soft, table_empty, BannerKind, ButtonKind,
};
use crate::ui::text_input::{ChangeText, TextField};
use crate::ui::toast;

/// 单工具页内输出容量（超出丢弃最旧，防内存/渲染膨胀；全程已落盘日志流不受影响）
const OUT_LINES_CAP: usize = 2000;

// ---------------------------------------------------------------------------
// 工具页签
// ---------------------------------------------------------------------------

/// 诊断工具标识（与原版 ToolId 一致）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolId {
    Ping,
    Trace,
    Nsl,
    Nat,
    Port,
    Iperf,
}

impl ToolId {
    const ALL: [ToolId; 6] = [
        ToolId::Ping,
        ToolId::Trace,
        ToolId::Nsl,
        ToolId::Nat,
        ToolId::Port,
        ToolId::Iperf,
    ];

    fn label(self) -> &'static str {
        match self {
            ToolId::Ping => "Ping",
            ToolId::Trace => "Traceroute",
            ToolId::Nsl => "Nslookup",
            ToolId::Nat => "NAT",
            ToolId::Port => "Port",
            ToolId::Iperf => "iperf3",
        }
    }

    fn idx(self) -> usize {
        match self {
            ToolId::Ping => 0,
            ToolId::Trace => 1,
            ToolId::Nsl => 2,
            ToolId::Nat => 3,
            ToolId::Port => 4,
            ToolId::Iperf => 5,
        }
    }

    /// 开始按钮文案（对齐原版：nsl=查询 / nat=检测 / iperf=测试 / 其余=开始）
    fn start_label(self) -> &'static str {
        match self {
            ToolId::Nsl => "查询",
            ToolId::Nat => "检测",
            ToolId::Iperf => "测试",
            _ => "开始",
        }
    }
}

// ---------------------------------------------------------------------------
// 运行态
// ---------------------------------------------------------------------------

/// 单工具运行态
struct ToolState {
    running: bool,
    /// 当前任务取消 ID（cancel 注册表键）
    cmd_id: String,
    /// 输出行（流式追加）
    lines: Vec<String>,
    /// 错误行（红显；对应原版 error）
    error: Option<String>,
    /// 汇总状态行（对应原版 summary）
    status: String,
    /// 事件通道接收端（任务运行时存在）
    rx: Option<Receiver<StreamEvent>>,
}

impl ToolState {
    fn new() -> Self {
        Self {
            running: false,
            cmd_id: String::new(),
            lines: Vec::new(),
            error: None,
            status: String::new(),
            rx: None,
        }
    }
}

/// 站点检测状态
#[derive(Clone)]
struct SiteStatus {
    testing: bool,
    ok: Option<bool>,
    ms: u64,
    status_code: Option<u16>,
}

/// 站点编辑态（idx=None 表示新增）
struct SiteEdit {
    idx: Option<usize>,
    name: Entity<TextField>,
    url: Entity<TextField>,
}

/// 任务执行器（cmdId + 事件发送端 → 最终结果消息）
type Runner = Box<dyn FnOnce(&str, Sender<StreamEvent>) -> Result<String, String> + Send>;

// ---------------------------------------------------------------------------
// 视图
// ---------------------------------------------------------------------------

pub struct NetworkView {
    /// 页面外观，随壳主题联动
    appearance: Appearance,
    /// 页面滚动状态
    page_scroll: ScrollHandle,
    /// 输出区滚动句柄（自动滚底）
    out_scroll: ScrollHandle,

    // ── 工具页签与共享参数 ──
    active_tool: ToolId,
    target_input: Entity<TextField>,
    port_input: Entity<TextField>,
    /// 网络栈：0=auto 1=v4 2=v6
    ip_version: usize,
    /// 协议：0=icmp 1=tcp 2=udp
    proto: usize,

    // ── 每工具参数 ──
    ping_count: Entity<TextField>,
    ping_interval: Entity<TextField>,
    ping_size: Entity<TextField>,
    ping_ttl: Entity<TextField>,
    ping_deadline: Entity<TextField>,
    ping_continuous: bool,
    trace_hops: Entity<TextField>,
    /// "system" = 本机 DNS，或 DNS IP（预设 pill / 自定义输入共用同一状态）
    trace_dns: String,
    /// Traceroute 自定义 DNS 输入（ChangeText 订阅 → trace_dns）
    trace_dns_custom: Entity<TextField>,
    /// 记录类型：0=A 1=AAAA
    nsl_type: usize,
    /// STUN 服务器（host:port）
    stun_server: String,
    stun_custom: Entity<TextField>,
    iperf_port: Entity<TextField>,
    iperf_duration: Entity<TextField>,

    // ── 六工具运行态（按 ToolId::idx 索引）──
    tools: [ToolState; 6],

    // ── DHCP ──
    dhcp_running: bool,
    dhcp_result: Option<diag::dhcp_probe::DhcpProbeResult>,
    dhcp_error: Option<String>,
    deep_running: bool,
    deep_result: Option<diag::dhcp_probe::DhcpDeepCheckResult>,
    deep_error: Option<String>,

    // ── 网站测试 ──
    custom_sites: Vec<diag::sites::SiteItem>,
    site_status: HashMap<String, SiteStatus>,
    site_edit: Option<SiteEdit>,
    adding: bool,
    new_name: Entity<TextField>,
    new_url: Entity<TextField>,
    auto_test: bool,
    auto_interval_ms: u64,
    auto_gen: u64,
}

/// 当前时刻 HH:MM:SS
fn hms() -> String {
    secm_core::logger::now_hms()
}

impl NetworkView {
    pub fn new(appearance: Appearance, cx: &mut Context<Self>) -> Self {
        // 统一文本字段工厂（SharedString 要求 'static 字面量；先建字段后订阅）
        let mut text = |v: &'static str, ph: &'static str| cx.new(|cx| TextField::new(v, ph, cx));
        let custom_sites = diag::sites::load_sites();
        log::info!(
            "网络诊断 · 页面已打开（自定义站点 {} 条）",
            custom_sites.len()
        );
        let trace_dns_custom = text("", "自定义 DNS IP");
        let target_input = text("bilibili.com", "8.8.8.8 或 google.com");
        let port_input = text("443", "1-65535");
        let ping_count = text("4", "1-9999");
        let ping_interval = text("1000", "10-60000");
        let ping_size = text("56", "32-65507");
        let ping_ttl = text("64", "1-255");
        let ping_deadline = text("0", "0=不限");
        let trace_hops = text("30", "1-64");
        let stun_custom = text("", "host:port");
        let iperf_port = text("5201", "1-65535");
        let iperf_duration = text("10", "1-120");
        let new_name = text("", "名称");
        let new_url = text("", "https://...");
        // 自定义 DNS 输入 → trace_dns 实时同步（对齐原版 DnsSelect 单一状态语义）
        cx.subscribe(&trace_dns_custom, |this, field, _ev: &ChangeText, _cx| {
            let v = field.read(_cx).value().trim().to_string();
            this.trace_dns = if v.is_empty() {
                String::from("system")
            } else {
                v
            };
        })
        .detach();
        Self {
            appearance,
            page_scroll: ScrollHandle::new(),
            out_scroll: ScrollHandle::new(),
            active_tool: ToolId::Ping,
            target_input,
            port_input,
            ip_version: 0,
            proto: 0,
            ping_count,
            ping_interval,
            ping_size,
            ping_ttl,
            ping_deadline,
            ping_continuous: false,
            trace_hops,
            trace_dns: String::from("system"),
            trace_dns_custom,
            nsl_type: 0,
            stun_server: diag::nat::get_stun_servers()[0].clone(),
            stun_custom,
            iperf_port,
            iperf_duration,
            tools: [
                ToolState::new(),
                ToolState::new(),
                ToolState::new(),
                ToolState::new(),
                ToolState::new(),
                ToolState::new(),
            ],
            dhcp_running: false,
            dhcp_result: None,
            dhcp_error: None,
            deep_running: false,
            deep_result: None,
            deep_error: None,
            custom_sites,
            site_status: HashMap::new(),
            site_edit: None,
            adding: false,
            new_name,
            new_url,
            auto_test: false,
            auto_interval_ms: 5000,
            auto_gen: 0,
        }
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

    fn tool_state(&self, tool: ToolId) -> &ToolState {
        &self.tools[tool.idx()]
    }

    /// 任一工具运行中（共享参数运行中锁定语义）
    fn any_running(&self) -> bool {
        self.tools.iter().any(|t| t.running)
    }

    /// 数字输入解析（非法/越界 → Err 文案；不静默纠偏，明示原因）
    fn parse_num(
        field: &Entity<TextField>,
        label: &str,
        min: u64,
        max: u64,
        cx: &Context<Self>,
    ) -> Result<u64, String> {
        let raw = field.read(cx).value().trim().to_string();
        let v: u64 = raw
            .parse()
            .map_err(|_| format!("{label} 不是有效数字（输入：{raw:?}）"))?;
        if v < min || v > max {
            return Err(format!("{label} 超出范围 {min}-{max}（输入：{v}）"));
        }
        Ok(v)
    }

    /// 当前共享目标
    fn shared_target(&self, cx: &Context<Self>) -> String {
        self.target_input.read(cx).value().trim().to_string()
    }

    /// 当前共享端口
    fn shared_port(&self, cx: &Context<Self>) -> Result<u16, String> {
        let v = Self::parse_num(&self.port_input, "端口", 1, 65535, cx)?;
        Ok(v as u16)
    }

    fn ip_version_str(&self) -> &'static str {
        match self.ip_version {
            1 => "v4",
            2 => "v6",
            _ => "auto",
        }
    }

    fn proto_str(&self) -> &'static str {
        match self.proto {
            1 => "tcp",
            2 => "udp",
            _ => "icmp",
        }
    }

    // ======================================================================
    // 工具执行（启动 / 停止 / 清除 / 事件排空）
    // ======================================================================

    /// 启动当前工具
    fn start_tool(&mut self, cx: &mut Context<Self>) {
        let tool = self.active_tool;
        if self.tool_state(tool).running {
            log::warn!("网络诊断 · {} · 忽略重复开始（任务运行中）", tool.label());
            return;
        }

        // ── 参数收集与校验（IIFE 借用 self/cx，结束即释放）──
        let target = self.shared_target(cx);
        let build: Result<Runner, String> = (|| {
            let runner: Runner = match tool {
                ToolId::Ping => {
                    if target.is_empty() {
                        return Err("目标地址不能为空".to_string());
                    }
                    let count = Self::parse_num(&self.ping_count, "次数", 1, 9999, cx)? as u32;
                    let interval = Self::parse_num(&self.ping_interval, "间隔ms", 10, 60000, cx)?;
                    let size = Self::parse_num(&self.ping_size, "字节", 32, 65507, cx)? as u32;
                    let ttl = Self::parse_num(&self.ping_ttl, "TTL", 1, 255, cx)? as u32;
                    let deadline =
                        Self::parse_num(&self.ping_deadline, "截止s", 0, 3600, cx)? as u32;
                    let port = self.shared_port(cx)?;
                    let params = diag::ping::PingParams {
                        target: target.clone(),
                        count,
                        interval_ms: interval,
                        continuous: self.ping_continuous,
                        ip_version: self.ip_version_str().to_string(),
                        proto: self.proto_str().to_string(),
                        size,
                        ttl,
                        deadline_secs: deadline,
                        port,
                    };
                    let t = target.clone();
                    Box::new(move |cmd_id: &str, tx: Sender<StreamEvent>| {
                        let tx2 = tx.clone();
                        let emit = move |ev: StreamEvent| {
                            let _ = tx2.send(ev);
                        };
                        diag::ping::ping_streaming(&params, cmd_id, &emit)
                            .map(|s| {
                                format!(
                                    "发送 {} · 接收 {} · 丢失 {:.1}% · 平均 {:.1} ms",
                                    s.sent, s.received, s.loss_percent, s.avg_rtt_ms
                                )
                            })
                            .map_err(|e| format!("{t}: {e}"))
                    })
                }
                ToolId::Trace => {
                    if target.is_empty() {
                        return Err("目标地址不能为空".to_string());
                    }
                    let hops = Self::parse_num(&self.trace_hops, "最大跳数", 1, 64, cx)? as u8;
                    let dns = if self.trace_dns.is_empty() || self.trace_dns == "system" {
                        None
                    } else {
                        Some(self.trace_dns.clone())
                    };
                    let ipv = self.ip_version_str().to_string();
                    let t = target.clone();
                    Box::new(move |cmd_id: &str, tx: Sender<StreamEvent>| {
                        let tx2 = tx.clone();
                        let emit = move |ev: StreamEvent| {
                            let _ = tx2.send(ev);
                        };
                        diag::traceroute::trace_streaming(
                            &t,
                            hops,
                            cmd_id,
                            &ipv,
                            dns.as_deref(),
                            &emit,
                        )
                        .map(|r| format!("{} 跳", r.hops.len()))
                    })
                }
                ToolId::Nsl => {
                    if target.is_empty() {
                        return Err("域名不能为空".to_string());
                    }
                    let rt = match self.nsl_type {
                        1 => "AAAA",
                        _ => "A",
                    }
                    .to_string();
                    let t = target.clone();
                    Box::new(move |cmd_id: &str, tx: Sender<StreamEvent>| {
                        let tx2 = tx.clone();
                        let emit = move |ev: StreamEvent| {
                            let _ = tx2.send(ev);
                        };
                        diag::nslookup::nslookup_streaming(&t, &rt, cmd_id, &emit)
                            .map(|r| format!("{} 条记录", r.records.len()))
                    })
                }
                ToolId::Nat => {
                    let custom = self.stun_custom.read(cx).value().trim().to_string();
                    let server = if custom.is_empty() {
                        self.stun_server.clone()
                    } else {
                        custom
                    };
                    Box::new(move |cmd_id: &str, tx: Sender<StreamEvent>| {
                        let tx2 = tx.clone();
                        let emit = move |ev: StreamEvent| {
                            let _ = tx2.send(ev);
                        };
                        diag::nat::detect_nat_streaming(&server, cmd_id, &emit).map(|r| r.nat_label)
                    })
                }
                ToolId::Port => {
                    if target.is_empty() {
                        return Err("目标地址不能为空".to_string());
                    }
                    let port = self.shared_port(cx)?;
                    let t = target.clone();
                    Box::new(move |_cmd_id: &str, tx: Sender<StreamEvent>| {
                        let tx2 = tx.clone();
                        let emit = move |ev: StreamEvent| {
                            let _ = tx2.send(ev);
                        };
                        let r = diag::sites::port_probe(&t, port, 5000);
                        let line = if r.ok {
                            format!("[{}]  端口 {t}:{port} ✓ 开放（{} ms）", hms(), r.ms)
                        } else {
                            format!(
                                "[{}]  端口 {t}:{port} ✗ 不可达（{}）",
                                hms(),
                                r.error.unwrap_or_else(|| "超时".to_string())
                            )
                        };
                        emit(StreamEvent::text("info", line));
                        Ok("检测完成".to_string())
                    })
                }
                ToolId::Iperf => {
                    let port = Self::parse_num(&self.iperf_port, "端口", 1, 65535, cx)? as u16;
                    let dur = Self::parse_num(&self.iperf_duration, "秒数", 1, 120, cx)? as u32;
                    let host = if target.is_empty() {
                        "127.0.0.1".to_string()
                    } else {
                        target.clone()
                    };
                    Box::new(move |cmd_id: &str, tx: Sender<StreamEvent>| {
                        // iperf3 的 stdout 读线程为 'static，emit 以 Arc 共享
                        let arc: Arc<dyn Fn(StreamEvent) + Send + Sync> =
                            Arc::new(move |ev: StreamEvent| {
                                let _ = tx.send(ev);
                            });
                        diag::iperf3::run_streaming(&host, port, dur, cmd_id, arc).map(|stderr| {
                            if stderr.is_empty() {
                                "测试完成".to_string()
                            } else {
                                stderr
                            }
                        })
                    })
                }
            };
            Ok(runner)
        })();

        let runner = match build {
            Ok(r) => r,
            Err(e) => {
                let st = &mut self.tools[tool.idx()];
                st.status = format!("启动失败：{e}");
                log::warn!("网络诊断 · {} · 启动失败：{}", tool.label(), e);
                cx.notify();
                return;
            }
        };

        // ── 任务生命周期 ──
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let cmd_id = format!("net-{}-{now_ms}", tool.label().to_lowercase());
        let _flag = cancel_reg::cancel_flag(&cmd_id);
        let (tx, rx) = channel::<StreamEvent>();

        {
            let st = &mut self.tools[tool.idx()];
            st.running = true;
            st.cmd_id = cmd_id.clone();
            st.lines.clear();
            st.error = None;
            st.status = "运行中…".to_string();
            st.rx = Some(rx);
        }

        log::info!(
            "网络诊断 · {} · 开始 · 目标={} 网络栈={} 协议={} cmdId={}",
            tool.label(),
            if target.is_empty() {
                "（默认）"
            } else {
                &target
            },
            self.ip_version_str(),
            self.proto_str(),
            cmd_id
        );
        cx.notify();

        // ── worker：后台线程执行诊断，结束发 DONE 哨兵 ──
        cx.spawn(async move |_, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let tx_worker = tx.clone();
            let worker_cmd = cmd_id.clone();
            let result = exec
                .spawn(async move { runner(&worker_cmd, tx_worker) })
                .await;
            let done = match &result {
                Ok(msg) => serde_json::json!({ "ok": true, "msg": msg }),
                Err(e) => serde_json::json!({ "ok": false, "msg": e }),
            };
            let _ = tx.send(StreamEvent::text(diag::KIND_DONE, done.to_string()));
        })
        .detach();

        // ── drainer：30ms 合并排空事件（零丢失；任务结束即冲刷收尾）──
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx: &mut gpui::AsyncApp| loop {
            gpui::Timer::after(Duration::from_millis(30)).await;
            let mut done = false;
            if let Some(view) = weak.upgrade() {
                let _ = view.update(cx, |this, _cx| {
                    done = this.drain_events(tool);
                });
            } else {
                break;
            }
            if done {
                break;
            }
        })
        .detach();
    }

    /// 排空当前工具事件通道（返回 true = 任务已结束）
    fn drain_events(&mut self, tool: ToolId) -> bool {
        let mut finished = false;
        let mut appended = 0usize;
        {
            let st = &mut self.tools[tool.idx()];
            let Some(rx) = st.rx.as_ref() else {
                return true;
            };
            loop {
                match rx.try_recv() {
                    Ok(ev) => {
                        let is_done = ev.kind == diag::KIND_DONE;
                        match ev.kind.as_str() {
                            "error" => {
                                log::warn!("网络诊断 · {} · {}", tool.label(), ev.data);
                                st.error = Some(format!("[{}]  {}", hms(), ev.data));
                            }
                            "summary" => {
                                st.status = summary_label(&ev.data);
                            }
                            "done" => {}
                            _ => {
                                if let Some(line) = format_event(&ev) {
                                    log::info!("网络诊断 · {} · {}", tool.label(), line);
                                    st.lines.push(line);
                                    appended += 1;
                                    if st.lines.len() > OUT_LINES_CAP {
                                        let drop = st.lines.len() - OUT_LINES_CAP;
                                        st.lines.drain(..drop);
                                    }
                                }
                            }
                        }
                        if is_done {
                            // 最终结果：JSON {ok, msg}
                            let (ok, msg) = serde_json::from_str::<serde_json::Value>(&ev.data)
                                .ok()
                                .and_then(|v| {
                                    Some((
                                        v.get("ok")?.as_bool()?,
                                        v.get("msg")?.as_str()?.to_string(),
                                    ))
                                })
                                .unwrap_or((true, ev.data.clone()));
                            st.running = false;
                            st.rx = None;
                            if ok {
                                st.status = msg.clone();
                                log::info!("网络诊断 · {} · 完成 · {}", tool.label(), msg);
                            } else {
                                st.status = format!("失败：{msg}");
                                if st.error.is_none() {
                                    st.error = Some(format!("[{}]  {}", hms(), msg));
                                }
                                log::warn!("网络诊断 · {} · 失败 · {}", tool.label(), msg);
                            }
                            finished = true;
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        if appended > 0 || finished {
            // 输出区贴底（最新行可见；对齐原版流式自动滚动）
            let max = f32::from(self.out_scroll.max_offset().height);
            self.out_scroll.set_offset(point(px(0.0), px(-max)));
        }
        finished
    }

    /// 停止当前工具（取消必达：置位 → 任务退出 → DONE 冲刷 → 可观察可记录）
    fn stop_tool(&mut self, cx: &mut Context<Self>) {
        let tool = self.active_tool;
        let st = &self.tools[tool.idx()];
        if !st.running {
            return;
        }
        let cmd_id = st.cmd_id.clone();
        cancel_reg::cancel_command(&cmd_id);
        log::info!(
            "网络诊断 · {} · 用户请求停止（cmdId={cmd_id}）",
            tool.label()
        );
        cx.notify();
    }

    /// 清除当前工具输出（运行中禁止，防丢实时流）
    fn clear_tool(&mut self, cx: &mut Context<Self>) {
        let tool = self.active_tool;
        let st = &mut self.tools[tool.idx()];
        if st.running {
            return;
        }
        st.lines.clear();
        st.error = None;
        st.status.clear();
        log::info!("网络诊断 · {} · 输出已清除", tool.label());
        cx.notify();
    }

    // ======================================================================
    // DHCP 检测
    // ======================================================================

    /// DHCP 服务器检测（探测 ∪ 注册表基线；事件逐条进右侧日志流）
    fn run_dhcp_probe(&mut self, cx: &mut Context<Self>) {
        if self.dhcp_running {
            log::warn!("网络诊断 · DHCP · 忽略重复检测（任务运行中）");
            return;
        }
        self.dhcp_running = true;
        self.dhcp_error = None;
        log::info!("网络诊断 · DHCP · 开始服务器检测（广播 Discover + 注册表基线合并）");
        cx.notify();

        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let result = exec
                .spawn(async move {
                    diag::dhcp_probe::probe_dhcp_servers_streaming("dhcp-probe", &|ev| {
                        if ev.kind == "error" {
                            log::warn!("网络诊断 · DHCP · {}", ev.data);
                        } else {
                            log::info!("网络诊断 · DHCP · {}", ev.data);
                        }
                    })
                })
                .await;
            if let Some(view) = weak.upgrade() {
                let _ = view.update(cx, |this, cx| {
                    this.dhcp_running = false;
                    match result {
                        Ok(r) => {
                            log::info!(
                                "网络诊断 · DHCP · 完成 · {} 台 · {}",
                                r.count,
                                if r.healthy { "健康" } else { "疑似冲突" }
                            );
                            this.dhcp_result = Some(r);
                        }
                        Err(e) => {
                            log::warn!("网络诊断 · DHCP · 失败 · {e}");
                            this.dhcp_error = Some(e);
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// DHCP 深度检查（探测 → 逐台 ping → 拓扑判定）
    fn run_deep_check(&mut self, cx: &mut Context<Self>) {
        if self.deep_running {
            log::warn!("网络诊断 · DHCP深度 · 忽略重复检查（任务运行中）");
            return;
        }
        self.deep_running = true;
        self.deep_error = None;
        log::info!("网络诊断 · DHCP深度 · 开始深度检查（探测 → 逐台 ping → 网段/网关拓扑判定）");
        cx.notify();

        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let result = exec
                .spawn(async move {
                    diag::dhcp_probe::deep_check_dhcp_servers_streaming("dhcp-deep", &|ev| {
                        if ev.kind == "error" {
                            log::warn!("网络诊断 · DHCP深度 · {}", ev.data);
                        } else {
                            log::info!("网络诊断 · DHCP深度 · {}", ev.data);
                        }
                    })
                })
                .await;
            if let Some(view) = weak.upgrade() {
                let _ = view.update(cx, |this, cx| {
                    this.deep_running = false;
                    match result {
                        Ok(r) => {
                            log::info!("网络诊断 · DHCP深度 · 完成 · {}", r.summary);
                            this.deep_result = Some(r);
                        }
                        Err(e) => {
                            log::warn!("网络诊断 · DHCP深度 · 失败 · {e}");
                            this.deep_error = Some(e);
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    // ======================================================================
    // 网站测试
    // ======================================================================

    /// 全部站点列表（默认在前，与原版 allSites 顺序一致）
    fn all_sites(&self) -> Vec<diag::sites::SiteItem> {
        let mut out: Vec<diag::sites::SiteItem> = diag::sites::DEFAULT_SITES
            .iter()
            .map(|(n, u)| diag::sites::SiteItem {
                name: n.to_string(),
                url: u.to_string(),
            })
            .collect();
        out.extend(self.custom_sites.iter().cloned());
        out
    }

    /// 检测单站点（后台 HEAD 探测 8s；状态徽标实时更新）
    fn test_site(&mut self, url: String, cx: &mut Context<Self>) {
        if self
            .site_status
            .get(&url)
            .map(|s| s.testing)
            .unwrap_or(false)
        {
            return;
        }
        self.site_status.insert(
            url.clone(),
            SiteStatus {
                testing: true,
                ok: None,
                ms: 0,
                status_code: None,
            },
        );
        log::info!("网络诊断 · 网站测试 · 开始 · {url}");
        cx.notify();

        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let url_probe = url.clone();
            let r = exec
                .spawn(async move { diag::sites::head_probe(&url_probe, 8000) })
                .await;
            if let Some(view) = weak.upgrade() {
                let _ = view.update(cx, |this, cx| {
                    let testing = this
                        .site_status
                        .get(&url)
                        .map(|s| s.testing)
                        .unwrap_or(false);
                    if testing {
                        this.site_status.insert(
                            url.clone(),
                            SiteStatus {
                                testing: false,
                                ok: Some(r.ok),
                                ms: r.ms,
                                status_code: r.status,
                            },
                        );
                        match (r.ok, r.status) {
                            (true, Some(code)) => log::info!(
                                "网络诊断 · 网站测试 · {url} · 可达 · HTTP {code} · {} ms",
                                r.ms
                            ),
                            (true, None) => {
                                log::info!("网络诊断 · 网站测试 · {url} · 可达 · {} ms", r.ms)
                            }
                            (false, _) => log::warn!(
                                "网络诊断 · 网站测试 · {url} · 不可达 · {}（{} ms）",
                                r.error.as_deref().unwrap_or("超时"),
                                r.ms
                            ),
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 全部站点并发检测
    fn test_all_sites(&mut self, cx: &mut Context<Self>) {
        let urls: Vec<String> = self.all_sites().into_iter().map(|s| s.url).collect();
        log::info!("网络诊断 · 网站测试 · 全部检测（{} 个站点）", urls.len());
        for url in urls {
            self.test_site(url, cx);
        }
    }

    /// 自动检测开关（generation 计数停止旧循环，可观察可取消）
    fn set_auto_test(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.auto_test == on {
            return;
        }
        self.auto_test = on;
        self.auto_gen += 1;
        if on {
            let gen = self.auto_gen;
            log::info!(
                "网络诊断 · 网站测试 · 自动检测开启（间隔 {}s）",
                self.auto_interval_ms / 1000
            );
            self.auto_loop(gen, cx);
        } else {
            log::info!("网络诊断 · 网站测试 · 自动检测关闭");
        }
        cx.notify();
    }

    /// 自动检测循环（按当前间隔轮询；generation 变化即退出）
    fn auto_loop(&mut self, gen: u64, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx: &mut gpui::AsyncApp| {
            loop {
                // 立即执行一轮
                if let Some(view) = weak.upgrade() {
                    let _ = view.update(cx, |this, cx| this.test_all_sites(cx));
                } else {
                    break;
                }
                // 等待当前间隔（等待中改变间隔 → 下一轮生效）
                let (cont, interval) = match weak.upgrade() {
                    Some(view) => view
                        .update(cx, |this, _| {
                            (
                                this.auto_test && this.auto_gen == gen,
                                this.auto_interval_ms,
                            )
                        })
                        .unwrap_or((false, 5000)),
                    None => break,
                };
                if !cont {
                    break;
                }
                gpui::Timer::after(Duration::from_millis(interval)).await;
                let cont = match weak.upgrade() {
                    Some(view) => view
                        .update(cx, |this, _| this.auto_test && this.auto_gen == gen)
                        .unwrap_or(false),
                    None => false,
                };
                if !cont {
                    break;
                }
            }
        })
        .detach();
    }

    /// 保存自定义站点（持久化 + 日志；失败明示 + 全局泡泡）
    fn persist_sites(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = diag::sites::save_sites(&self.custom_sites) {
            log::warn!("网络诊断 · 网站测试 · 站点持久化失败：{e}");
            toast::error(format!("站点列表保存失败：{e}"), cx);
        } else {
            log::info!(
                "网络诊断 · 网站测试 · 站点列表已保存（{} 条自定义）",
                self.custom_sites.len()
            );
        }
        cx.notify();
    }

    /// 提交新增站点
    fn submit_add(&mut self, cx: &mut Context<Self>) {
        let name = self.new_name.read(cx).value().trim().to_string();
        let url = self.new_url.read(cx).value().trim().to_string();
        if name.is_empty() || url.is_empty() {
            log::warn!("网络诊断 · 网站测试 · 添加失败：名称与 URL 均不能为空");
            toast::warning("名称与 URL 均不能为空", cx);
            cx.notify();
            return;
        }
        self.custom_sites.push(diag::sites::SiteItem {
            name: name.clone(),
            url: url.clone(),
        });
        self.persist_sites(cx);
        self.adding = false;
        log::info!("网络诊断 · 网站测试 · 已添加站点「{name}」{url}");
        toast::success(format!("已添加站点「{name}」"), cx);
        cx.notify();
    }

    /// 提交编辑站点
    fn submit_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.site_edit.take() else {
            return;
        };
        let name = edit.name.read(cx).value().trim().to_string();
        let url = edit.url.read(cx).value().trim().to_string();
        let Some(idx) = edit.idx else { return };
        if name.is_empty() || url.is_empty() {
            log::warn!("网络诊断 · 网站测试 · 编辑失败：名称与 URL 均不能为空");
            toast::warning("名称与 URL 均不能为空", cx);
            cx.notify();
            return;
        }
        if let Some(old) = self.custom_sites.get(idx) {
            let old_url = old.url.clone();
            if let Some(item) = self.custom_sites.get_mut(idx) {
                item.name = name.clone();
                item.url = url.clone();
            }
            // 旧 URL 的状态缓存一并清理
            self.site_status.remove(&old_url);
            log::info!("网络诊断 · 网站测试 · 已更新站点「{name}」→ {url}");
            toast::success(format!("已更新站点「{name}」"), cx);
            self.persist_sites(cx);
        }
        cx.notify();
    }

    /// 删除自定义站点
    fn delete_site(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some(item) = self.custom_sites.get(idx) {
            let (name, url) = (item.name.clone(), item.url.clone());
            self.custom_sites.remove(idx);
            self.site_status.remove(&url);
            log::info!("网络诊断 · 网站测试 · 已删除站点「{name}」{url}");
            toast::info(format!("已删除站点「{name}」"), cx);
            self.persist_sites(cx);
        }
        cx.notify();
    }
}

// ============================================================================
// 渲染
// ============================================================================

impl Render for NetworkView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.pal();
        let running = self.tools.iter().filter(|t| t.running).count();
        let status = if running > 0 {
            format!("{running} 个任务运行中")
        } else {
            "就绪".to_string()
        };

        page_root(&pal, "network-page-root", &self.page_scroll, &cx.entity())
            .child(
                page_header(
                    &pal,
                    "网络诊断",
                    "DHCP 检测 · 网站测试 · 网络工具（实时流式输出 · 全程日志可溯 · 可取消）",
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(12.0))
                        .text_color(if running > 0 {
                            pal.accent
                        } else {
                            pal.text_muted
                        })
                        .child(SharedString::from(status)),
                ),
            )
            .child(self.top_row(&pal, cx))
            .child(self.tools_card(&pal, cx))
    }
}

impl NetworkView {
    /// 顶部两列：DHCP 卡 + 网站测试卡（对齐原版 lg:grid-cols-2）
    fn top_row(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_wrap()
            .gap_4()
            .child(self.dhcp_card(pal, cx).flex_1().min_w(px(360.0)))
            .child(self.sites_card(pal, cx).flex_1().min_w(px(360.0)))
    }

    // ------------------------------------------------------------------
    // DHCP 卡
    // ------------------------------------------------------------------

    fn dhcp_card(&self, pal: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        card(pal)
            .child(
                card_header(pal, "DHCP 服务器检测")
                    .child(
                        button_sm(pal, ButtonKind::Secondary)
                            .id("net-dhcp-deep")
                            .child(if self.deep_running {
                                "检查中…"
                            } else {
                                "深度检查"
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.run_deep_check(cx))),
                    )
                    .child(
                        button_sm(pal, ButtonKind::Primary)
                            .id("net-dhcp-probe")
                            .child(if self.dhcp_running {
                                "检测中…"
                            } else {
                                "检测"
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.run_dhcp_probe(cx))),
                    ),
            )
            .child(card_divider(pal))
            .child(card_body(pal).child(self.dhcp_body(pal)))
    }

    fn dhcp_body(&self, pal: &Palette) -> impl IntoElement {
        div().flex_col().gap_2().children({
            let mut items: Vec<gpui::AnyElement> = Vec::new();
            // 基础检测态
            if self.dhcp_running {
                items.push(
                    div()
                        .text_size(px(11.5))
                        .text_color(pal.text_muted)
                        .child("正在广播 DHCP Discover 并收集响应…（约 3-4 秒）")
                        .into_any_element(),
                );
            } else if let Some(r) = self.dhcp_result.as_ref() {
                items.push(self.dhcp_result_view(pal, r).into_any_element());
            } else if let Some(e) = self.dhcp_error.as_ref() {
                items.push(
                    banner(pal, BannerKind::Danger, SharedString::from(format!("检测失败：{e}")))
                        .into_any_element(),
                );
            } else {
                items.push(
                    div()
                        .text_size(px(11.5))
                        .text_color(pal.text_muted)
                        .child("未检测。点击「检测」向当前子网广播 DHCP Discover（约 3-4 秒），统计能响应的 DHCP 服务器数量；1 台为正常，多台可能冲突。")
                        .into_any_element(),
                );
            }
            // 深度检查态（独立状态区）
            if self.deep_running {
                items.push(
                    div()
                        .text_size(px(11.5))
                        .text_color(pal.text_muted)
                        .child("正在深度检查：探测 DHCP 服务器 → 逐台 ping 可达性 → 网段/网关拓扑判定…")
                        .into_any_element(),
                );
            } else if let Some(r) = self.deep_result.as_ref() {
                items.push(self.deep_result_view(pal, r).into_any_element());
            } else if let Some(e) = self.deep_error.as_ref() {
                items.push(
                    banner(pal, BannerKind::Danger, SharedString::from(format!("深度检查失败：{e}")))
                        .into_any_element(),
                );
            }
            items
        })
    }

    /// DHCP 探测结果视图（健康绿 / 冲突黄 + 服务器徽标 + 建议清单 + note）
    fn dhcp_result_view(
        &self,
        pal: &Palette,
        r: &diag::dhcp_probe::DhcpProbeResult,
    ) -> impl IntoElement {
        div()
            .flex_col()
            .gap_2()
            .child(if r.healthy {
                banner(
                    pal,
                    BannerKind::Success,
                    SharedString::from(format!("健康 · {} 台", r.count)),
                )
            } else {
                banner(
                    pal,
                    BannerKind::Warn,
                    SharedString::from(format!(
                        "检测到 {} 台 DHCP 服务器，可能存在 DHCP 冲突",
                        r.count
                    )),
                )
            })
            .when(!r.servers.is_empty(), |s| {
                s.child(
                    div().flex().flex_wrap().gap_1().children(
                        r.servers
                            .iter()
                            .map(|srv| badge(pal, SharedString::from(srv.clone()), pal.accent)),
                    ),
                )
            })
            .when(!r.healthy, |s| {
                s.child(
                    div()
                        .flex_col()
                        .gap_1()
                        .rounded(px(10.0))
                        .border_1()
                        .border_color(soft(pal.warning, 0.30))
                        .bg(soft(pal.warning, 0.05))
                        .p(px(10.0))
                        .child(
                            div()
                                .text_size(px(11.5))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(pal.warning)
                                .child("建议检查清单"),
                        )
                        .children(
                            [
                                "路由器 DHCP 设置",
                                "多路由器级联",
                                "软路由 / 旁路由 / 热点",
                                "AP 的 DHCP 是否关闭",
                            ]
                            .map(|t| {
                                div()
                                    .text_size(px(11.0))
                                    .text_color(pal.text_muted)
                                    .child(SharedString::from(format!("· {t}")))
                            }),
                        ),
                )
            })
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(pal.text_muted)
                    .child(SharedString::from(r.note.clone())),
            )
    }

    /// DHCP 深度检查结果视图（本机定位 + 逐台诊断 + 综合结论）
    fn deep_result_view(
        &self,
        pal: &Palette,
        r: &diag::dhcp_probe::DhcpDeepCheckResult,
    ) -> impl IntoElement {
        let sev_color = |sev: &str| -> gpui::Rgba {
            match sev {
                "ok" => pal.success,
                "warn" => pal.warning,
                _ => pal.danger,
            }
        };
        let summary_kind = if r.checks.iter().any(|c| c.severity == "critical") {
            BannerKind::Danger
        } else if r.checks.iter().any(|c| c.severity == "warn") {
            BannerKind::Warn
        } else {
            BannerKind::Success
        };
        div()
            .flex_col()
            .gap_2()
            // 本机网络定位
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(pal.text_muted)
                            .child("本机 IP"),
                    )
                    .child(badge(
                        pal,
                        SharedString::from(r.local_ip.clone().unwrap_or_else(|| "—".into())),
                        pal.text_muted,
                    ))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(pal.text_muted)
                            .child("掩码"),
                    )
                    .child(badge(
                        pal,
                        SharedString::from(r.subnet_mask.clone().unwrap_or_else(|| "—".into())),
                        pal.text_muted,
                    ))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(pal.text_muted)
                            .child("默认网关"),
                    )
                    .child(badge(
                        pal,
                        SharedString::from(r.gateway.clone().unwrap_or_else(|| "—".into())),
                        pal.text_muted,
                    )),
            )
            // 逐台服务器诊断
            .children(r.checks.iter().map(|c| {
                let color = sev_color(&c.severity);
                div()
                    .flex_col()
                    .gap_1()
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(soft(color, 0.30))
                    .bg(soft(color, 0.05))
                    .p(px(10.0))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(pal.text)
                                    .child(SharedString::from(c.server.clone())),
                            )
                            .child(if c.ping_ok {
                                badge(
                                    pal,
                                    SharedString::from(format!("ping {:.1}ms", c.ping_rtt_ms)),
                                    pal.success,
                                )
                            } else {
                                badge(pal, "ping 不通", pal.danger)
                            })
                            .child(if c.same_subnet {
                                badge(pal, "同网段", pal.text_muted)
                            } else {
                                badge(pal, "跨网段", pal.warning)
                            })
                            .child(if c.is_gateway {
                                badge(pal, "= 默认网关", pal.success)
                            } else {
                                badge(pal, "≠ 默认网关", pal.text_muted)
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(color)
                            .child(SharedString::from(c.diagnosis.clone())),
                    )
            }))
            // 综合结论
            .child(banner(
                pal,
                summary_kind,
                SharedString::from(r.summary.clone()),
            ))
    }

    // ------------------------------------------------------------------
    // 网站测试卡
    // ------------------------------------------------------------------

    fn sites_card(&self, pal: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        let intervals: [u64; 6] = [3000, 5000, 10000, 15000, 30000, 60000];
        card(pal)
            .child(
                card_header(pal, "网站测试")
                    .child(
                        button_sm(pal, ButtonKind::Secondary)
                            .id("net-sites-testall")
                            .child("全部检测")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.test_all_sites(cx);
                            })),
                    )
                    .child(
                        button_sm(pal, ButtonKind::Secondary)
                            .id("net-sites-add")
                            .child("添加")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.adding = true;
                                log::info!("网络诊断 · 网站测试 · 打开添加站点表单");
                                cx.notify();
                            })),
                    )
                    .child(
                        self.pill(
                            pal,
                            "net-sites-auto",
                            if self.auto_test {
                                "自动检测"
                            } else {
                                "手动"
                            },
                            self.auto_test,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            let on = !this.auto_test;
                            this.set_auto_test(on, cx);
                        })),
                    )
                    .when(self.auto_test, |s| {
                        s.children(intervals.map(|ms| {
                            let label = if ms >= 60000 {
                                format!("{}min", ms / 60000)
                            } else {
                                format!("{}s", ms / 1000)
                            };
                            let active = self.auto_interval_ms == ms;
                            self.pill(
                                pal,
                                SharedString::from(format!("net-sites-iv-{ms}")),
                                &label,
                                active,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if this.auto_interval_ms != ms {
                                        this.auto_interval_ms = ms;
                                        log::info!(
                                            "网络诊断 · 网站测试 · 自动检测间隔 → {} ms",
                                            ms
                                        );
                                        cx.notify();
                                    }
                                },
                            ))
                        }))
                    }),
            )
            .child(card_divider(pal))
            .child(card_body(pal).child(self.sites_body(pal, cx)))
    }

    fn sites_body(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div().flex_col().gap_2();
        // 新增表单
        if self.adding {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .p(px(8.0))
                    .rounded(px(8.0))
                    .bg(pal.bg_subtle)
                    .child(div().w(px(110.0)).child(self.new_name.clone()))
                    .child(div().flex_1().child(self.new_url.clone()))
                    .child(
                        button_sm(pal, ButtonKind::Primary)
                            .id("net-sites-save")
                            .child("保存")
                            .on_click(cx.listener(|this, _, _, cx| this.submit_add(cx))),
                    )
                    .child(
                        button_sm(pal, ButtonKind::Ghost)
                            .id("net-sites-cancel-add")
                            .child("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.adding = false;
                                log::info!("网络诊断 · 网站测试 · 取消添加站点");
                                cx.notify();
                            })),
                    ),
            );
        }
        // 编辑表单
        if let Some(edit) = self.site_edit.as_ref() {
            let name = edit.name.clone();
            let url = edit.url.clone();
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .p(px(8.0))
                    .rounded(px(8.0))
                    .bg(pal.bg_subtle)
                    .child(div().w(px(110.0)).child(name))
                    .child(div().flex_1().child(url))
                    .child(
                        button_sm(pal, ButtonKind::Primary)
                            .id("net-sites-edit-save")
                            .child("保存")
                            .on_click(cx.listener(|this, _, _, cx| this.submit_edit(cx))),
                    )
                    .child(
                        button_sm(pal, ButtonKind::Ghost)
                            .id("net-sites-edit-cancel")
                            .child("取消")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.site_edit = None;
                                log::info!("网络诊断 · 网站测试 · 取消编辑站点");
                                cx.notify();
                            })),
                    ),
            );
        }
        // 站点网格（默认 + 自定义；点击测试）
        let sites = self.all_sites();
        let default_count = diag::sites::DEFAULT_SITES.len();
        body = body.child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .children(sites.iter().enumerate().map(|(idx, site)| {
                    let url = site.url.clone();
                    let status = self.site_status.get(&url);
                    let testing = status.map(|s| s.testing).unwrap_or(false);
                    let ok = status.and_then(|s| s.ok);
                    let is_default = idx < default_count;
                    let (border, bg) = if testing {
                        (soft(pal.accent, 0.40), soft(pal.accent, 0.06))
                    } else {
                        match ok {
                            Some(true) => (soft(pal.success, 0.35), soft(pal.success, 0.06)),
                            Some(false) => (soft(pal.danger, 0.35), soft(pal.danger, 0.06)),
                            None => (pal.border, pal.bg_subtle),
                        }
                    };
                    let mut tile = div()
                        .id(SharedString::from(format!("net-site-{idx}")))
                        .flex_col()
                        .gap_1()
                        .w(px(190.0))
                        .p(px(10.0))
                        .rounded(px(10.0))
                        .border_1()
                        .border_color(border)
                        .bg(bg)
                        .cursor_pointer()
                        .hover(|s| s.bg(soft(pal.accent, 0.10)))
                        .on_click({
                            let url = url.clone();
                            cx.listener(move |this, _, _, cx| this.test_site(url.clone(), cx))
                        })
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(pal.text)
                                        .child(SharedString::from(site.name.clone())),
                                )
                                .when(!is_default, |s| {
                                    s.child(
                                        div()
                                            .flex()
                                            .gap(px(2.0))
                                            .child(
                                                button_sm(pal, ButtonKind::Ghost)
                                                    .id(SharedString::from(format!(
                                                        "net-site-edit-{idx}"
                                                    )))
                                                    .child("改")
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.begin_edit(idx, cx);
                                                        },
                                                    )),
                                            )
                                            .child(
                                                button_sm(pal, ButtonKind::Ghost)
                                                    .id(SharedString::from(format!(
                                                        "net-site-del-{idx}"
                                                    )))
                                                    .child("删")
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.delete_site(
                                                                idx - default_count,
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                            ),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(pal.text_muted)
                                .truncate()
                                .child(SharedString::from(site.url.clone())),
                        );
                    // 状态行
                    let status_line: gpui::AnyElement = if testing {
                        badge(pal, "检测中", pal.accent).into_any_element()
                    } else {
                        match (ok, status.and_then(|s| s.status_code)) {
                            (Some(true), code) => {
                                let ms = status.map(|s| s.ms).unwrap_or(0);
                                let text = match code {
                                    Some(c) => format!("{ms}ms · HTTP {c}"),
                                    None => format!("{ms}ms"),
                                };
                                badge(pal, SharedString::from(text), pal.success).into_any_element()
                            }
                            (Some(false), _) => badge(pal, "不可达", pal.danger).into_any_element(),
                            _ => div()
                                .text_size(px(10.5))
                                .text_color(pal.text_muted)
                                .child("点击检测")
                                .into_any_element(),
                        }
                    };
                    tile = tile.child(status_line);
                    tile
                })),
        );
        body
    }

    /// 打开编辑表单（预填当前值）
    fn begin_edit(&mut self, custom_idx: usize, cx: &mut Context<Self>) {
        let Some(item) = self.custom_sites.get(custom_idx) else {
            return;
        };
        let (name, url) = (item.name.clone(), item.url.clone());
        let name_field = self.site_edit.as_ref().map(|e| e.name.clone());
        let url_field = self.site_edit.as_ref().map(|e| e.url.clone());
        let name_field = name_field.unwrap_or_else(|| cx.new(|cx| TextField::new("", "名称", cx)));
        let url_field =
            url_field.unwrap_or_else(|| cx.new(|cx| TextField::new("", "https://...", cx)));
        name_field.update(cx, |f, cx| f.set_value(name.clone(), cx));
        url_field.update(cx, |f, cx| f.set_value(url, cx));
        self.site_edit = Some(SiteEdit {
            idx: Some(custom_idx),
            name: name_field,
            url: url_field,
        });
        log::info!("网络诊断 · 网站测试 · 打开编辑站点「{name}」");
        cx.notify();
    }

    // ------------------------------------------------------------------
    // 网络工具卡
    // ------------------------------------------------------------------

    fn tools_card(&self, pal: &Palette, cx: &mut Context<Self>) -> gpui::Div {
        let tool = self.active_tool;
        let st = self.tool_state(tool);
        let running = st.running;

        card(pal)
            .child(card_header(pal, "网络工具").child(if running {
                button(pal, ButtonKind::Danger)
                    .id("net-tool-stop")
                    .child("停止")
                    .on_click(cx.listener(|this, _, _, cx| this.stop_tool(cx)))
            } else {
                button(pal, ButtonKind::Primary)
                    .id("net-tool-start")
                    .child(SharedString::from(tool.start_label().to_string()))
                    .on_click(cx.listener(|this, _, _, cx| this.start_tool(cx)))
            }))
            .child(card_divider(pal))
            .child(
                card_body(pal)
                    // 工具页签行
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .children(ToolId::ALL.map(|t| {
                                let t_running = self.tool_state(t).running;
                                let active = self.active_tool == t;
                                self.pill(
                                    pal,
                                    SharedString::from(format!("net-tool-{}", t.label())),
                                    t.label(),
                                    active,
                                )
                                .when(t_running, |s| {
                                    s.child(div().size(px(6.0)).rounded_full().bg(pal.accent))
                                })
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        if this.active_tool != t {
                                            this.active_tool = t;
                                            log::info!("网络诊断 · 切换工具 → {}", t.label());
                                            cx.notify();
                                        }
                                    },
                                ))
                            })),
                    )
                    // 共享参数行
                    .child(self.shared_row(pal, cx))
                    // 当前工具参数行
                    .child(self.params_row(pal, cx))
                    // 输出区
                    .child(self.output_area(pal, cx)),
            )
    }

    /// 共享参数行：目标 / 端口 / 网络栈 / 协议（运行中视觉锁定：仅提示，参数在启动时快照）
    fn shared_row(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let tool = self.active_tool;
        let locked = self.any_running();
        div()
            .flex()
            .flex_wrap()
            .items_end()
            .gap_2()
            .when(locked, |s| s.opacity(0.55))
            .child(
                div()
                    .flex_col()
                    .flex_1()
                    .min_w(px(140.0))
                    .gap_1()
                    .child(field_label(
                        pal,
                        if tool == ToolId::Nat {
                            "STUN 服务器（下方配置）"
                        } else {
                            "目标地址（域名/IP）"
                        },
                    ))
                    .when(tool == ToolId::Nat, |s| s.opacity(0.5))
                    .child(self.target_input.clone()),
            )
            .child(
                div()
                    .flex_col()
                    .w(px(96.0))
                    .gap_1()
                    .child(field_label(pal, "端口"))
                    .child(self.port_input.clone()),
            )
            .child(
                div()
                    .flex_col()
                    .gap_1()
                    .child(field_label(pal, "网络栈"))
                    .child(
                        div().flex().gap_1().children(
                            ["自动", "V4", "V6"]
                                .into_iter()
                                .enumerate()
                                .map(|(i, label)| {
                                    let active = self.ip_version == i;
                                    self.pill(
                                        pal,
                                        SharedString::from(format!("net-ip-{i}")),
                                        label,
                                        active,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            if this.ip_version != i {
                                                this.ip_version = i;
                                                log::info!(
                                                    "网络诊断 · 网络栈 → {}",
                                                    ["auto", "v4", "v6"][i]
                                                );
                                                cx.notify();
                                            }
                                        },
                                    ))
                                }),
                        ),
                    ),
            )
            .child(
                div()
                    .flex_col()
                    .gap_1()
                    .child(field_label(pal, "协议"))
                    .child(
                        div().flex().gap_1().children(
                            ["ICMP", "TCP", "UDP"]
                                .into_iter()
                                .enumerate()
                                .map(|(i, label)| {
                                    let active = self.proto == i;
                                    self.pill(
                                        pal,
                                        SharedString::from(format!("net-proto-{i}")),
                                        label,
                                        active,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            if this.proto != i {
                                                this.proto = i;
                                                log::info!(
                                                    "网络诊断 · 协议 → {}",
                                                    ["icmp", "tcp", "udp"][i]
                                                );
                                                cx.notify();
                                            }
                                        },
                                    ))
                                }),
                        ),
                    ),
            )
    }

    /// 当前工具专属参数行
    fn params_row(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let num_field = |label: &str, field: &Entity<TextField>| {
            div()
                .flex_col()
                .w(px(96.0))
                .gap_1()
                .child(field_label(pal, SharedString::from(label.to_string())))
                .child(field.clone())
        };
        let mut row = div().flex().flex_wrap().items_end().gap_2().mt(px(2.0));
        match self.active_tool {
            ToolId::Ping => {
                row = row
                    .child(num_field("次数", &self.ping_count))
                    .child(num_field("间隔ms", &self.ping_interval))
                    .child(num_field("字节", &self.ping_size))
                    .child(num_field("TTL", &self.ping_ttl))
                    .child(num_field("截止s", &self.ping_deadline))
                    .child(
                        self.pill(
                            pal,
                            "net-ping-continuous",
                            if self.ping_continuous {
                                "连续：开"
                            } else {
                                "连续：关"
                            },
                            self.ping_continuous,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.ping_continuous = !this.ping_continuous;
                            log::info!(
                                "网络诊断 · Ping · 连续模式 → {}",
                                if this.ping_continuous { "开" } else { "关" }
                            );
                            cx.notify();
                        })),
                    );
            }
            ToolId::Trace => {
                row = row.child(num_field("最大跳数", &self.trace_hops)).child(
                    div()
                        .flex_col()
                        .gap_1()
                        .child(field_label(pal, "DNS 查询（本机 / 预设 / 自定义）"))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_1()
                                .child(
                                    self.pill(
                                        pal,
                                        "net-trace-dns-sys",
                                        "本机 DNS",
                                        self.trace_dns == "system",
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.trace_dns = "system".into();
                                            this.trace_dns_custom
                                                .update(cx, |f, cx| f.set_value("", cx));
                                            log::info!("网络诊断 · Traceroute · DNS → 本机 DNS");
                                            cx.notify();
                                        },
                                    )),
                                )
                                .children(TRACE_DNS_PRESETS.iter().copied().map(|(label, ip)| {
                                    let active = self.trace_dns == ip;
                                    self.pill(
                                        pal,
                                        SharedString::from(format!("net-trace-dns-{ip}")),
                                        label,
                                        active,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.trace_dns = ip.to_string();
                                            this.trace_dns_custom
                                                .update(cx, |f, cx| f.set_value(ip, cx));
                                            log::info!(
                                                "网络诊断 · Traceroute · DNS → {label} ({ip})"
                                            );
                                            cx.notify();
                                        },
                                    ))
                                }))
                                .child(div().w(px(150.0)).child(self.trace_dns_custom.clone())),
                        ),
                );
            }
            ToolId::Nsl => {
                row = row.child(
                    div()
                        .flex_col()
                        .gap_1()
                        .child(field_label(pal, "记录类型"))
                        .child(div().flex().gap_1().children(
                            [("A (IPv4)", 0usize), ("AAAA (IPv6)", 1usize)].map(|(label, i)| {
                                let active = self.nsl_type == i;
                                self.pill(
                                    pal,
                                    SharedString::from(format!("net-nsl-{i}")),
                                    label,
                                    active,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        if this.nsl_type != i {
                                            this.nsl_type = i;
                                            log::info!(
                                                "网络诊断 · Nslookup · 记录类型 → {}",
                                                ["A", "AAAA"][i]
                                            );
                                            cx.notify();
                                        }
                                    },
                                ))
                            }),
                        )),
                );
            }
            ToolId::Nat => {
                row = row.child(
                    div()
                        .flex_col()
                        .flex_1()
                        .min_w(px(240.0))
                        .gap_1()
                        .child(field_label(pal, "STUN 服务器（15 预设 + 自定义）"))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_1()
                                .children(diag::nat::get_stun_servers().iter().enumerate().map(
                                    |(i, srv)| {
                                        let active = self.stun_server == *srv
                                            && self.stun_custom.read(cx).value().trim().is_empty();
                                        let srv = srv.clone();
                                        self.pill(
                                            pal,
                                            SharedString::from(format!("net-stun-{i}")),
                                            &srv,
                                            active,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.stun_server = srv.clone();
                                                log::info!("网络诊断 · NAT · STUN 服务器 → {srv}");
                                                cx.notify();
                                            }),
                                        )
                                    },
                                ))
                                .child(div().w(px(170.0)).child(self.stun_custom.clone())),
                        ),
                );
            }
            ToolId::Port => {
                // 端口检测使用共享目标/端口，无专属参数
            }
            ToolId::Iperf => {
                row = row
                    .child(num_field("端口", &self.iperf_port))
                    .child(num_field("秒数", &self.iperf_duration));
            }
        }
        row
    }

    /// 流式输出区（对齐原版 DiagOutput：行数/清除/错误红显 + 自动滚底）
    fn output_area(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let tool = self.active_tool;
        let st = self.tool_state(tool);
        let has_content = !st.lines.is_empty() || st.error.is_some() || st.running;
        if !has_content {
            return table_empty(
                pal,
                "点击「开始」运行后，此处实时流式输出（同步写入右侧日志流）",
            )
            .into_any_element();
        }
        let running = st.running;
        let lines = st.lines.clone();
        let error = st.error.clone();
        let status = st.status.clone();
        let entity = cx.entity();
        div()
            .flex_col()
            .mt(px(10.0))
            .border_t_1()
            .border_color(pal.border)
            .pt(px(8.0))
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(if running { pal.accent } else { pal.text_muted })
                            .child(if running {
                                SharedString::from("诊断进行中…")
                            } else {
                                SharedString::from(format!("输出 {} 行", lines.len()))
                            }),
                    )
                    .child(
                        button_sm(pal, ButtonKind::Ghost)
                            .id("net-tool-clear")
                            .child("清除")
                            .on_click(cx.listener(|this, _, _, cx| this.clear_tool(cx))),
                    ),
            )
            .child(
                div()
                    .id("net-out-scroll")
                    .flex_col()
                    .h(px(220.0))
                    .overflow_y_scroll()
                    .scrollbar_width(px(0.0))
                    .track_scroll(&self.out_scroll)
                    .on_scroll_wheel(move |_ev: &gpui::ScrollWheelEvent, _w, cx| {
                        let _ = entity.update(cx, |_, cx| cx.notify());
                    })
                    .gap_1()
                    .children(lines.iter().map(|l| {
                        div()
                            .text_size(px(11.0))
                            .text_color(pal.text_muted)
                            .child(SharedString::from(l.clone()))
                    }))
                    .when_some(error.clone(), |s, e| {
                        s.child(
                            div()
                                .text_size(px(11.0))
                                .text_color(pal.danger)
                                .child(SharedString::from(e)),
                        )
                    })
                    .when(!status.is_empty() && !running, |s| {
                        s.child(
                            div()
                                .text_size(px(11.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(pal.text)
                                .child(SharedString::from(status.clone())),
                        )
                    }),
            )
            .into_any_element()
    }

    /// 通用 pill（页签/开关/预设选择统一骨架；调用方接 .on_click）
    fn pill(
        &self,
        pal: &Palette,
        id: impl Into<gpui::ElementId>,
        label: &str,
        active: bool,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap_1()
            .px(px(8.0))
            .h(px(24.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .text_size(px(11.0))
            .text_color(if active { pal.accent } else { pal.text_muted })
            .bg(if active { pal.bg_selected } else { TRANSPARENT })
            .border_1()
            .border_color(if active {
                soft(pal.accent, 0.40)
            } else {
                pal.border
            })
            .hover(move |s| s.bg(pal.bg_hover))
            .child(SharedString::from(label.to_string()))
    }
}

/// Traceroute DNS 预设（与原版 TRACE_DNS_PRESETS 一致）
const TRACE_DNS_PRESETS: &[(&str, &str)] = &[
    ("腾讯 DNSPod", "119.29.29.29"),
    ("阿里 AliDNS", "223.5.5.5"),
    ("百度 DNS", "180.76.76.76"),
    ("114 DNS", "114.114.114.114"),
    ("Google DNS", "8.8.8.8"),
    ("Cloudflare", "1.1.1.1"),
];

// ---------------------------------------------------------------------------
// 事件格式化（与原版 formatStreamMessage 文案逐条对齐）
// ---------------------------------------------------------------------------

/// 安全解析 JSON（对齐原版 safeParse：非 JSON 降级为原文，不中断）
fn safe_parse(data: &str) -> serde_json::Value {
    serde_json::from_str(data).unwrap_or(serde_json::Value::String(data.to_string()))
}

/// StreamEvent → 输出行（原版 formatStreamMessage 语义）
fn format_event(ev: &StreamEvent) -> Option<String> {
    match ev.kind.as_str() {
        "ping" => {
            let v = safe_parse(&ev.data);
            let seq = v.get("sequence").and_then(|x| x.as_u64()).unwrap_or(0);
            let ok = v.get("success").and_then(|x| x.as_bool()).unwrap_or(false);
            Some(if ok {
                let rtt = v.get("rtt_ms").and_then(|x| x.as_f64()).unwrap_or(0.0);
                format!("第 #{seq:03} 包: 成功  |  延迟 = {rtt:.1} ms")
            } else {
                format!("第 #{seq:03} 包: 超时  |  请求超时，无响应")
            })
        }
        "trace-hop" => {
            let v = safe_parse(&ev.data);
            let hop = v.get("hop").and_then(|x| x.as_u64()).unwrap_or(0);
            let ip = v
                .get("ip")
                .and_then(|x| x.as_str())
                .unwrap_or("*")
                .to_string();
            let answered = v
                .get("probes_answered")
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            let sent = v.get("probes_sent").and_then(|x| x.as_u64()).unwrap_or(3);
            let avg = v.get("rtt_avg_ms").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let min = v.get("rtt_min_ms").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let max = v.get("rtt_max_ms").and_then(|x| x.as_f64()).unwrap_or(0.0);
            Some(if avg > 0.0 {
                format!(
                    "跃点 {hop:02}: {ip:<18}  avg={avg:.1}ms  min={min:.1}ms  max={max:.1}ms  ({answered}/{sent})"
                )
            } else {
                format!("跃点 {hop:02}: {:<18}  * * *  ({answered}/{sent})", " *")
            })
        }
        "stun-step" => {
            let v = safe_parse(&ev.data);
            let phase = v
                .get("phase")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let desc = v
                .get("description")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            Some(format!("[{phase}] {desc}"))
        }
        "dns-record" | "info" => Some(ev.data.clone()),
        _ => None,
    }
}

/// summary 事件 → 状态行（按工具汇总结构识别）
fn summary_label(data: &str) -> String {
    let v = safe_parse(data);
    // ping 汇总
    if let (Some(sent), Some(recv)) = (
        v.get("sent").and_then(|x| x.as_u64()),
        v.get("received").and_then(|x| x.as_u64()),
    ) {
        let loss = v
            .get("loss_percent")
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0);
        return format!("汇总：发送 {sent} · 接收 {recv} · 丢失 {loss:.1}%");
    }
    // traceroute 汇总
    if let Some(hops) = v.get("hops").and_then(|x| x.as_array()) {
        return format!("汇总：{} 跳", hops.len());
    }
    // NAT 汇总
    if let Some(label) = v.get("nat_label").and_then(|x| x.as_str()) {
        return format!("汇总：{label}");
    }
    // DNS 汇总
    if let Some(records) = v.get("records").and_then(|x| x.as_array()) {
        return format!("汇总：{} 条记录", records.len());
    }
    "汇总".to_string()
}
