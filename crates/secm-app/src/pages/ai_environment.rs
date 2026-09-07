// secm-app::pages::ai_environment — AI 环境页
// npm 环境 + AI 工具管理（白名单安装/升级/卸载）+ MCP 服务器管理 + Skills/扩展扫描。
//
// 并发模型（多线程、互不阻塞）：
// - 四组检测（npm/工具/MCP/扩展）各自独立后台任务并发执行，互不等待；
// - 检测结果在后台线程算好、经 WeakEntity 回 UI 直接赋值（主线程绝不重跑查询）；
// - 安装/升级/卸载为外部命令操作，独立互斥锁防并发执行，但不断言 UI 线程；
// - 主线程仅做状态赋值与 cx.notify()。
//
// 渲染层已接入 crate::ui::page 统一页面布局框架，
// 色板取自 pi_clone::theme::Palette（明暗双套），随壳主题联动刷新。

use gpui::prelude::*;
use gpui::{div, px, SharedString, Window, Context, Render, WeakEntity};
use secm_core::environment::{
    self, AiExtension, AiTool, McpServerInfo, NpmEnvironment,
};

use crate::pi_clone::theme::{Appearance, Palette};
use crate::ui::page::{
    banner, button, button_sm, card, card_body, card_divider, card_header, kv_row_w, page_header,
    page_root, table_empty, BannerKind, ButtonKind,
};

/// 检测区（每组独立加载状态，可并发）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetectKind {
    Npm,
    Tools,
    Mcp,
    Ext,
}

/// 工具区操作（npm install -g / uninstall，外部命令）
#[derive(Debug, Clone)]
enum ToolAction {
    Install(String),
    Upgrade(String),
    Uninstall(String),
}

/// MCP 操作（npm install -g / uninstall，外部命令）
#[derive(Debug, Clone)]
enum McpAction {
    Install(String),
    Uninstall(String),
}

pub struct AiEnvironmentView {
    /// npm 环境
    npm: Option<NpmEnvironment>,
    npm_loading: bool,
    /// AI 工具
    tools: Vec<AiTool>,
    tools_loading: bool,
    /// MCP 服务器
    mcps: Vec<McpServerInfo>,
    mcps_loading: bool,
    /// Skills/扩展
    extensions: Vec<AiExtension>,
    ext_loading: bool,
    /// 操作互斥（安装/卸载类命令串行，防并发 npm 写）
    action_busy: bool,
    /// 状态/结果消息
    status: String,
    /// 页面外观，随壳主题联动
    appearance: Appearance,
    /// 页面滚动状态（GPUI 0.2 滚轮需 track_scroll 手动驱动，见 ui::page::page_root）
    page_scroll: gpui::ScrollHandle,
}

impl AiEnvironmentView {
    pub fn new(appearance: Appearance, cx: &mut Context<Self>) -> Self {
        log::info!("AI 环境 · 页面已打开");
        let mut v = Self {
            npm: None,
            npm_loading: false,
            tools: Vec::new(),
            tools_loading: false,
            mcps: Vec::new(),
            mcps_loading: false,
            extensions: Vec::new(),
            ext_loading: false,
            action_busy: false,
            status: String::new(),
            appearance,
            page_scroll: gpui::ScrollHandle::new(),
        };
        // 四组检测并发启动（各自独立后台任务）
        v.start_detect(DetectKind::Npm, cx);
        v.start_detect(DetectKind::Tools, cx);
        v.start_detect(DetectKind::Mcp, cx);
        v.start_detect(DetectKind::Ext, cx);
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

    // ------------------------------------------------------------------
    // 检测（每组独立并发；结果后台算好回填，主线程不重跑）
    // ------------------------------------------------------------------

    fn loading_of(&self, kind: DetectKind) -> bool {
        match kind {
            DetectKind::Npm => self.npm_loading,
            DetectKind::Tools => self.tools_loading,
            DetectKind::Mcp => self.mcps_loading,
            DetectKind::Ext => self.ext_loading,
        }
    }

    fn mark_loading(&mut self, kind: DetectKind, loading: bool) {
        match kind {
            DetectKind::Npm => self.npm_loading = loading,
            DetectKind::Tools => self.tools_loading = loading,
            DetectKind::Mcp => self.mcps_loading = loading,
            DetectKind::Ext => self.ext_loading = loading,
        }
    }

    /// 启动一组检测（已有同组在跑则忽略；每组独立，可并发四组）
    fn start_detect(&mut self, kind: DetectKind, cx: &mut Context<Self>) {
        if self.loading_of(kind) {
            return;
        }
        self.mark_loading(kind, true);
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            // 阻塞查询全部在后台线程执行
            let result = exec
                .spawn(async move {
                    match kind {
                        DetectKind::Npm => DetectOutcome::Npm(environment::check_npm_environment()),
                        DetectKind::Tools => {
                            DetectOutcome::Tools(environment::check_ai_tools().tools)
                        }
                        DetectKind::Mcp => DetectOutcome::Mcp(environment::list_mcp_servers()),
                        DetectKind::Ext => DetectOutcome::Ext(environment::list_extensions()),
                    }
                })
                .await;

            // UI 侧日志：各组检测完成（仅一次，不逐条）
            match &result {
                DetectOutcome::Npm(n) => log::info!(
                    "AI 环境 · npm 检测完成（可用: {}，全局包 {} 个）",
                    n.available,
                    n.global_packages
                ),
                DetectOutcome::Tools(t) => log::info!("AI 环境 · AI 工具检测完成，共 {} 项", t.len()),
                DetectOutcome::Mcp(m) => log::info!("AI 环境 · MCP 服务器检测完成，共 {} 项", m.len()),
                DetectOutcome::Ext(e) => log::info!("AI 环境 · Skills 扩展扫描完成，共 {} 项", e.len()),
            }

            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.mark_loading(kind, false);
                    match result {
                        DetectOutcome::Npm(n) => this.npm = Some(n),
                        DetectOutcome::Tools(t) => this.tools = t,
                        DetectOutcome::Mcp(m) => this.mcps = m,
                        DetectOutcome::Ext(e) => this.extensions = e,
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    // ------------------------------------------------------------------
    // 操作（安装/升级/卸载 — 外部命令，独立互斥，串行执行防并发写）
    // ------------------------------------------------------------------

    /// 工具操作（安装/升级/卸载同一包名语义为 install_or_upgrade）
    fn run_tool_action(&mut self, action: ToolAction, cx: &mut Context<Self>) {
        if self.action_busy {
            return;
        }
        // 全链路行为日志：用户触发 AI 工具安装/升级/卸载
        log::info!("AI 环境 · 触发{} {}", action.label(), action.package());
        self.action_busy = true;
        self.status = action.status_text();
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let action_worker = action.clone();
            let result = exec
                .spawn(async move {
                    match &action_worker {
                        ToolAction::Install(p) | ToolAction::Upgrade(p) => {
                            environment::install_or_upgrade_tool(p)
                        }
                        ToolAction::Uninstall(p) => environment::uninstall_ai_tool(p),
                    }
                })
                .await;

            // 全链路行为日志：AI 工具操作返回信息
            match &result {
                Ok(msg) => log::info!("AI 环境 · {} {} 成功: {}", action.label(), action.package(), msg),
                Err(e) => log::warn!("AI 环境 · {} {} 失败: {}", action.label(), action.package(), e),
            }

            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.action_busy = false;
                    this.status = match &result {
                        Ok(msg) => msg.clone(),
                        Err(e) => format!("{}：{}", action.label(), e),
                    };
                    cx.notify();
                })
                .ok();
                // 操作后后台重扫工具列表（不回主线程重跑）
                view.update(cx, |this, cx| {
                    this.start_detect(DetectKind::Tools, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    /// MCP 操作
    fn run_mcp_action(&mut self, action: McpAction, cx: &mut Context<Self>) {
        if self.action_busy {
            return;
        }
        // 全链路行为日志：用户触发 MCP 安装/卸载
        log::info!("AI 环境 · 触发{} {}", action.label(), action.package());
        self.action_busy = true;
        self.status = action.status_text();
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let action_worker = action.clone();
            let result = exec
                .spawn(async move {
                    match &action_worker {
                        McpAction::Install(p) => environment::install_mcp_server(p),
                        McpAction::Uninstall(p) => environment::uninstall_mcp_server(p),
                    }
                })
                .await;

            // 全链路行为日志：MCP 操作返回信息
            match &result {
                Ok(msg) => log::info!("AI 环境 · {} {} 成功: {}", action.label(), action.package(), msg),
                Err(e) => log::warn!("AI 环境 · {} {} 失败: {}", action.label(), action.package(), e),
            }

            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.action_busy = false;
                    this.status = match &result {
                        Ok(msg) => msg.clone(),
                        Err(e) => format!("{}：{}", action.label(), e),
                    };
                    cx.notify();
                })
                .ok();
                view.update(cx, |this, cx| {
                    this.start_detect(DetectKind::Mcp, cx);
                })
                .ok();
            }
        })
        .detach();
    }
}

/// 检测后台任务统一产出
enum DetectOutcome {
    Npm(NpmEnvironment),
    Tools(Vec<AiTool>),
    Mcp(Vec<McpServerInfo>),
    Ext(Vec<AiExtension>),
}

impl ToolAction {
    fn label(&self) -> &'static str {
        match self {
            Self::Install(_) => "安装",
            Self::Upgrade(_) => "升级",
            Self::Uninstall(_) => "卸载",
        }
    }
    fn package(&self) -> &str {
        match self {
            Self::Install(p) | Self::Upgrade(p) | Self::Uninstall(p) => p,
        }
    }
    fn status_text(&self) -> String {
        match self {
            Self::Install(p) | Self::Upgrade(p) | Self::Uninstall(p) => {
                format!("{} {}…", self.label(), p)
            }
        }
    }
}

impl McpAction {
    fn label(&self) -> &'static str {
        match self {
            Self::Install(_) => "安装 MCP",
            Self::Uninstall(_) => "卸载 MCP",
        }
    }
    fn package(&self) -> &str {
        match self {
            Self::Install(p) | Self::Uninstall(p) => p,
        }
    }
    fn status_text(&self) -> String {
        match self {
            Self::Install(p) => format!("安装 MCP {}…", p),
            Self::Uninstall(p) => format!("卸载 MCP {}…", p),
        }
    }
}

impl Render for AiEnvironmentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.pal();
        let npm = self.npm.clone();
        let tools: Vec<AiTool> = self.tools.clone();
        let mcps: Vec<McpServerInfo> = self.mcps.clone();
        let extensions: Vec<AiExtension> = self.extensions.clone();
        let status = self.status.clone();
        let action_busy = self.action_busy;

        // 统一页面骨架：根容器（内边距/纵向节奏/内容超高时整页纵向滚动）
        page_root(&pal, "ai_environment-page-root", &self.page_scroll, &cx.entity())
            // 页头：标题 + 副标题，右侧「全部刷新」
            .child(
                page_header(&pal, "AI 环境", "npm 环境 · AI 工具 · MCP 服务器 · Skills 扩展").child(
                    button(&pal, ButtonKind::Secondary)
                        .id("ai-rescan")
                        .child("全部刷新")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.start_detect(DetectKind::Npm, cx);
                            this.start_detect(DetectKind::Tools, cx);
                            this.start_detect(DetectKind::Mcp, cx);
                            this.start_detect(DetectKind::Ext, cx);
                        })),
                ),
            )
            // 状态消息
            .when(!status.is_empty(), |s| {
                let msg = status.clone();
                s.child(banner(&pal, BannerKind::Info, msg))
            })
            // npm 环境卡
            .child(self.npm_card(&pal, &npm, cx))
            // AI 工具卡
            .child(self.tools_card(&pal, &tools, action_busy, cx))
            // MCP 卡 + 扩展卡双列（flex 等宽两列；禁 grid —— taffy grid 滚动容器内不渲染）
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(self.mcp_card(&pal, &mcps, action_busy, cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(self.ext_card(&pal, &extensions, cx)),
                    ),
            )
    }
}

impl AiEnvironmentView {
    fn npm_card(
        &self,
        pal: &Palette,
        npm: &Option<NpmEnvironment>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let loading = self.npm_loading;
        card(pal)
            .child(
                card_header(pal, "npm 环境").child(
                    button_sm(pal, ButtonKind::Secondary)
                        .id("ai-npm-refresh")
                        .child(if loading { "检测中…" } else { "刷新" })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.start_detect(DetectKind::Npm, cx);
                        })),
                ),
            )
            .child(card_divider(pal))
            .when(npm.is_none() && !loading, |s| {
                s.child(table_empty(pal, "点击「刷新」检测 npm 环境"))
            })
            .when(npm.is_none() && loading, |s| {
                s.child(table_empty(pal, "检测中…"))
            })
            .when_some(npm.clone(), |s, n| {
                let rows = [
                    ("Node.js", n.node_version.clone()),
                    ("npm", n.npm_version.clone()),
                    ("全局前缀", n.prefix.clone()),
                    ("全局根目录", n.root.clone()),
                    ("registry", n.registry.clone()),
                    ("全局包数", n.global_packages.to_string()),
                ];
                s.child(card_body(pal).children(rows.iter().map(|(k, v)| {
                    let k = k.to_string();
                    let v = v.clone();
                    if v.is_empty() {
                        // 值缺失：键值行同款结构，值以 danger 色显示「不可用」
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .py(px(3.0))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(110.0))
                                    .text_size(px(12.0))
                                    .text_color(pal.text_muted)
                                    .child(k),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .text_size(px(12.5))
                                    .text_color(pal.danger)
                                    .child("不可用"),
                            )
                    } else {
                        kv_row_w(pal, 110.0, k, v)
                    }
                })))
            })
    }

    fn tools_card(
        &self,
        pal: &Palette,
        tools: &[AiTool],
        action_busy: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let loading = self.tools_loading;
        card(pal)
            .child(
                card_header(pal, "AI 开发工具").child(
                    button_sm(pal, ButtonKind::Secondary)
                        .id("ai-tools-refresh")
                        .child(if loading { "检测中…" } else { "刷新" })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.start_detect(DetectKind::Tools, cx);
                        })),
                ),
            )
            .child(card_divider(pal))
            .when(tools.is_empty() && loading, |s| {
                s.child(table_empty(pal, "检测中…（npm 查询）"))
            })
            .when(tools.is_empty() && !loading, |s| {
                s.child(table_empty(pal, "暂无数据 — 点击「刷新」"))
            })
            .children(tools.iter().map(|t| {
                let tool = t.clone();
                let pkg = tool.npm_package.clone();
                let name = tool.display_name.clone();
                let installed = tool.installed;
                let version = tool.version.clone();
                let upgradable = tool.upgradable;
                let disabled = action_busy;
                let pkg_upgrade = pkg.clone();
                let pkg_install = pkg.clone();
                let pkg_uninstall = pkg.clone();
                div()
                    .id(SharedString::from(format!("ai-tool-{}", tool.name)))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_5()
                    .py_2()
                    .border_b_1()
                    .border_color(pal.border)
                    .child(
                        div()
                            .size(px(6.0))
                            .rounded_full()
                            .bg(if installed { pal.success } else { pal.text_muted }),
                    )
                    .child(
                        div()
                            .w(px(110.0))
                            .flex_none()
                            .text_size(px(12.5))
                            .text_color(pal.text)
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.5))
                            .text_color(pal.text_muted)
                            .child(if installed { version } else { "未安装".to_string() }),
                    )
                    .when(installed && upgradable, |r| {
                        r.child(
                            button_sm(pal, ButtonKind::Warning)
                                .id("upgrade-tool")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !disabled {
                                        this.run_tool_action(ToolAction::Upgrade(pkg_upgrade.clone()), cx);
                                    }
                                }))
                                .child("升级"),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(
                                // 已装=卸载（Danger）/ 未装=安装（Primary），按钮随状态切换语义
                                button_sm(
                                    pal,
                                    if installed {
                                        ButtonKind::Danger
                                    } else {
                                        ButtonKind::Primary
                                    },
                                )
                                .id("install-tool")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !disabled {
                                        if installed {
                                            this.run_tool_action(
                                                ToolAction::Uninstall(pkg_uninstall.clone()),
                                                cx,
                                            );
                                        } else {
                                            this.run_tool_action(
                                                ToolAction::Install(pkg_install.clone()),
                                                cx,
                                            );
                                        }
                                    }
                                }))
                                .child(if installed { "卸载" } else { "安装" }),
                            ),
                    )
            }))
    }

    fn mcp_card(
        &self,
        pal: &Palette,
        mcps: &[McpServerInfo],
        action_busy: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let loading = self.mcps_loading;
        card(pal)
            .child(
                card_header(pal, "MCP 服务器").child(
                    button_sm(pal, ButtonKind::Secondary)
                        .id("mcp-refresh")
                        .child(if loading { "检测中…" } else { "刷新" })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.start_detect(DetectKind::Mcp, cx);
                        })),
                ),
            )
            .child(card_divider(pal))
            .children(mcps.iter().map(|m| {
                let pkg_uninstall = m.package.clone();
                let pkg_install = m.package.clone();
                let installed = m.installed;
                let disabled = action_busy;
                div()
                    .id(SharedString::from(format!("ai-mcp-{}", m.name)))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_5()
                    .py_1p5()
                    .border_b_1()
                    .border_color(pal.border)
                    .child(
                        div()
                            .size(px(6.0))
                            .rounded_full()
                            .bg(if installed { pal.success } else { pal.text_muted }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.5))
                            .text_color(pal.text)
                            .child(m.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(pal.text_muted)
                            .child(m.package.clone()),
                    )
                    .when(installed, |r| {
                        r.child(
                            button_sm(pal, ButtonKind::Danger)
                                .id("uninstall-mcp")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !disabled {
                                        this.run_mcp_action(McpAction::Uninstall(pkg_uninstall.clone()), cx);
                                    }
                                }))
                                .child("卸载"),
                        )
                    })
                    .when(!installed, |r| {
                        r.child(
                            button_sm(pal, ButtonKind::Primary)
                                .id("install-mcp")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !disabled {
                                        this.run_mcp_action(McpAction::Install(pkg_install.clone()), cx);
                                    }
                                }))
                                .child("安装"),
                        )
                    })
            }))
            .when(mcps.is_empty() && !loading, |s| {
                s.child(table_empty(pal, "暂无 MCP 数据 — 点击「刷新」"))
            })
            .when(mcps.is_empty() && loading, |s| {
                s.child(table_empty(pal, "检测中…"))
            })
    }

    fn ext_card(
        &self,
        pal: &Palette,
        extensions: &[AiExtension],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let loading = self.ext_loading;
        card(pal)
            .child(
                card_header(pal, "Skills / 扩展").child(
                    button_sm(pal, ButtonKind::Secondary)
                        .id("ext-refresh")
                        .child(if loading { "扫描中…" } else { "刷新" })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.start_detect(DetectKind::Ext, cx);
                        })),
                ),
            )
            .child(card_divider(pal))
            .when(extensions.is_empty() && !loading, |s| {
                s.child(table_empty(pal, "未发现扩展（点击「刷新」扫描用户目录）"))
            })
            .when(extensions.is_empty() && loading, |s| {
                s.child(table_empty(pal, "扫描中…"))
            })
            .children(extensions.iter().take(12).map(|e| {
                let tool = e.tool.clone();
                let kind = e.kind.clone();
                let name = e.name.clone();
                let desc = e.description.clone();
                div()
                    .flex_col()
                    .gap_0p5()
                    .px_5()
                    .py_1p5()
                    .border_b_1()
                    .border_color(pal.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(pal.text)
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(pal.text_muted)
                                    .child(SharedString::from(format!("{}/{}", tool, kind))),
                            ),
                    )
                    .when(!desc.is_empty(), |s| {
                        s.child(
                            div()
                                .text_size(px(11.0))
                                .text_color(pal.text_muted)
                                .child(desc),
                        )
                    })
            }))
    }
}
