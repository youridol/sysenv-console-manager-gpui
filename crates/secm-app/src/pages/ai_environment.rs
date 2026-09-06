// secm-app::pages::ai_environment — AI 环境页
// npm 环境 + AI 工具管理（白名单安装/升级/卸载）+ MCP 服务器管理 + Skills/扩展扫描。
//
// 并发模型（多线程、互不阻塞）：
// - 四组检测（npm/工具/MCP/扩展）各自独立后台任务并发执行，互不等待；
// - 检测结果在后台线程算好、经 WeakEntity 回 UI 直接赋值（主线程绝不重跑查询）；
// - 安装/升级/卸载为外部命令操作，独立互斥锁防并发执行，但不断言 UI 线程；
// - 主线程仅做状态赋值与 cx.notify()。

use gpui::prelude::*;
use gpui::{div, px, rgb, SharedString, Window, Context, Render, WeakEntity};
use secm_core::environment::{
    self, AiExtension, AiTool, McpServerInfo, NpmEnvironment,
};

use crate::theme::Theme;

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
}

impl AiEnvironmentView {
    pub fn new(cx: &mut Context<Self>) -> Self {
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
        };
        // 四组检测并发启动（各自独立后台任务）
        v.start_detect(DetectKind::Npm, cx);
        v.start_detect(DetectKind::Tools, cx);
        v.start_detect(DetectKind::Mcp, cx);
        v.start_detect(DetectKind::Ext, cx);
        v
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
    fn status_text(&self) -> String {
        match self {
            Self::Install(p) => format!("安装 MCP {}…", p),
            Self::Uninstall(p) => format!("卸载 MCP {}…", p),
        }
    }
}

impl Render for AiEnvironmentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::dark();
        let npm = self.npm.clone();
        let tools: Vec<AiTool> = self.tools.clone();
        let mcps: Vec<McpServerInfo> = self.mcps.clone();
        let extensions: Vec<AiExtension> = self.extensions.clone();
        let status = self.status.clone();
        let action_busy = self.action_busy;

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
                                    .child("AI 环境"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme.text_muted)
                                    .child("npm 环境 · AI 工具 · MCP · Skills 扩展"),
                            ),
                    )
                    .child(
                        div()
                            .id("ai-rescan")
                            .px_4()
                            .py_1p5()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(theme.panel_hover)
                            .hover(|s| s.bg(theme.border))
                            .text_color(theme.text)
                            .text_size(px(12.5))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_detect(DetectKind::Npm, cx);
                                this.start_detect(DetectKind::Tools, cx);
                                this.start_detect(DetectKind::Mcp, cx);
                                this.start_detect(DetectKind::Ext, cx);
                            }))
                            .child("全部刷新"),
                    ),
            )
            // 状态消息
            .when(!status.is_empty(), |s| {
                let msg = status.clone();
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
            // npm 环境卡
            .child(self.npm_card(&theme, &npm, cx))
            // AI 工具卡
            .child(self.tools_card(&theme, &tools, action_busy, cx))
            // MCP 卡 + 扩展卡双列
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_4()
                    .child(self.mcp_card(&theme, &mcps, action_busy, cx))
                    .child(self.ext_card(&theme, &extensions, cx)),
            )
    }
}

impl AiEnvironmentView {
    fn npm_card(
        &self,
        theme: &Theme,
        npm: &Option<NpmEnvironment>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let loading = self.npm_loading;
        crate::ui::table_container(theme)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_5()
                    .py_3()
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child("npm 环境"),
                    )
                    .child(
                        div()
                            .id("ai-npm-refresh")
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(theme.panel_hover)
                            .hover(|s| s.bg(theme.border))
                            .text_color(theme.text)
                            .text_size(px(11.5))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_detect(DetectKind::Npm, cx);
                            }))
                            .child(if loading { "检测中…" } else { "刷新" }),
                    ),
            )
            .when(npm.is_none() && !loading, |s| {
                s.child(crate::ui::table_empty(theme, "点击「刷新」检测 npm 环境"))
            })
            .when(npm.is_none() && loading, |s| {
                s.child(crate::ui::table_empty(theme, "检测中…"))
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
                s.children(rows.iter().map(|(k, v)| {
                    let k = k.to_string();
                    let v = v.clone();
                    div()
                        .flex()
                        .items_center()
                        .px_5()
                        .py_1p5()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(
                            div()
                                .w(px(110.0))
                                .flex_none()
                                .text_size(px(12.0))
                                .text_color(theme.text_muted)
                                .child(k),
                        )
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(if v.is_empty() { theme.danger } else { theme.text })
                                .child(if v.is_empty() { "不可用".to_string() } else { v }),
                        )
                }))
            })
    }

    fn tools_card(
        &self,
        theme: &Theme,
        tools: &[AiTool],
        action_busy: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let loading = self.tools_loading;
        crate::ui::table_container(theme)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_5()
                    .py_3()
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child("AI 开发工具"),
                    )
                    .child(
                        div()
                            .id("ai-tools-refresh")
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(theme.panel_hover)
                            .hover(|s| s.bg(theme.border))
                            .text_color(theme.text)
                            .text_size(px(11.5))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_detect(DetectKind::Tools, cx);
                            }))
                            .child(if loading { "检测中…" } else { "刷新" }),
                    ),
            )
            .when(tools.is_empty() && loading, |s| {
                s.child(crate::ui::table_empty(theme, "检测中…（npm 查询）"))
            })
            .when(tools.is_empty() && !loading, |s| {
                s.child(crate::ui::table_empty(theme, "暂无数据 — 点击「刷新」"))
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
                    .border_color(theme.border)
                    .child(
                        div()
                            .size(px(6.0))
                            .rounded_full()
                            .bg(if installed { theme.success } else { theme.text_muted }),
                    )
                    .child(
                        div()
                            .w(px(110.0))
                            .flex_none()
                            .text_size(px(12.5))
                            .text_color(theme.text)
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.5))
                            .text_color(theme.text_muted)
                            .child(if installed { version } else { "未安装".to_string() }),
                    )
                    .when(installed && upgradable, |r| {
                        r.child(
                            div()
                                .id("upgrade-tool")
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .cursor_pointer()
                                .bg(rgb(0x78350f))
                                .hover(|s| s.bg(rgb(0x92400e)))
                                .text_color(rgb(0xfde68a))
                                .text_size(px(11.0))
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
                                div()
                                    .id("install-tool")
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .bg(if installed { theme.panel_hover } else { theme.brand })
                                    .hover(|s| s.bg(if installed { theme.border } else { rgb(0x3d66e6) }))
                                    .text_color(if installed { theme.text } else { rgb(0xffffff) })
                                    .text_size(px(11.0))
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
        theme: &Theme,
        mcps: &[McpServerInfo],
        action_busy: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let loading = self.mcps_loading;
        crate::ui::table_container(theme)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_5()
                    .py_3()
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child("MCP 服务器"),
                    )
                    .child(
                        div()
                            .id("mcp-refresh")
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(theme.panel_hover)
                            .hover(|s| s.bg(theme.border))
                            .text_color(theme.text)
                            .text_size(px(11.5))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_detect(DetectKind::Mcp, cx);
                            }))
                            .child(if loading { "检测中…" } else { "刷新" }),
                    ),
            )
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
                    .border_color(theme.border)
                    .child(
                        div()
                            .size(px(6.0))
                            .rounded_full()
                            .bg(if installed { theme.success } else { theme.text_muted }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.5))
                            .text_color(theme.text)
                            .child(m.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_muted)
                            .child(m.package.clone()),
                    )
                    .when(installed, |r| {
                        r.child(
                            div()
                                .id("uninstall-mcp")
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .cursor_pointer()
                                .bg(theme.panel_hover)
                                .hover(|s| s.bg(rgb(0x7f1d1d)))
                                .text_color(theme.text)
                                .text_size(px(11.0))
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
                            div()
                                .id("install-mcp")
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .cursor_pointer()
                                .bg(theme.brand)
                                .hover(|s| s.bg(rgb(0x3d66e6)))
                                .text_color(rgb(0xffffff))
                                .text_size(px(11.0))
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
                s.child(crate::ui::table_empty(theme, "暂无 MCP 数据 — 点击「刷新」"))
            })
            .when(mcps.is_empty() && loading, |s| {
                s.child(crate::ui::table_empty(theme, "检测中…"))
            })
    }

    fn ext_card(
        &self,
        theme: &Theme,
        extensions: &[AiExtension],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let loading = self.ext_loading;
        crate::ui::table_container(theme)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_5()
                    .py_3()
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child("Skills / 扩展"),
                    )
                    .child(
                        div()
                            .id("ext-refresh")
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(theme.panel_hover)
                            .hover(|s| s.bg(theme.border))
                            .text_color(theme.text)
                            .text_size(px(11.5))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_detect(DetectKind::Ext, cx);
                            }))
                            .child(if loading { "扫描中…" } else { "刷新" }),
                    ),
            )
            .when(extensions.is_empty() && !loading, |s| {
                s.child(crate::ui::table_empty(theme, "未发现扩展（点击「刷新」扫描用户目录）"))
            })
            .when(extensions.is_empty() && loading, |s| {
                s.child(crate::ui::table_empty(theme, "扫描中…"))
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
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme.text)
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(theme.text_muted)
                                    .child(SharedString::from(format!("{}/{}", tool, kind))),
                            ),
                    )
                    .when(!desc.is_empty(), |s| {
                        s.child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme.text_muted)
                                .child(desc),
                        )
                    })
            }))
    }
}
