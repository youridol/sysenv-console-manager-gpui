// secm-app::pages::ai_environment — AI 环境页（Phase 5）
// npm 环境 + AI 工具管理（白名单安装/升级/卸载）+ MCP 服务器管理 + Skills/扩展扫描。
// 检测与安装均执行外部 npm 命令 → 一律后台线程执行，操作后自动重扫。

use gpui::prelude::*;
use gpui::{div, px, rgb, SharedString, Window, Context, Render, WeakEntity};
use secm_core::environment::{
    self, AiExtension, AiTool, McpServerInfo, NpmEnvironment,
};

use crate::theme::Theme;

/// 后台操作类型（统一进度/结果反馈）
#[derive(Debug, Clone, PartialEq)]
enum AiOp {
    CheckTools,
    CheckNpm,
    CheckMcp,
    CheckExt,
    InstallTool(String),
    UpgradeTool(String),
    UninstallTool(String),
    InstallMcp(String),
    UninstallMcp(String),
}

impl AiOp {
    fn label(&self) -> String {
        match self {
            Self::CheckTools => "检测 AI 工具".to_string(),
            Self::CheckNpm => "检测 npm 环境".to_string(),
            Self::CheckMcp => "检测 MCP 服务器".to_string(),
            Self::CheckExt => "扫描 Skills/扩展".to_string(),
            Self::InstallTool(p) | Self::UpgradeTool(p) => format!("安装/升级 {}", p),
            Self::UninstallTool(p) => format!("卸载 {}", p),
            Self::InstallMcp(p) => format!("安装 MCP {}", p),
            Self::UninstallMcp(p) => format!("卸载 MCP {}", p),
        }
    }
}

pub struct AiEnvironmentView {
    /// npm 环境（node/npm 版本）
    npm: Option<NpmEnvironment>,
    /// AI 工具（含最新版本与可升级标记）
    tools: Vec<AiTool>,
    /// MCP 服务器
    mcps: Vec<McpServerInfo>,
    /// Skills/扩展
    extensions: Vec<AiExtension>,
    /// 正在执行的操作（None=空闲）
    busy: Option<AiOp>,
    /// 状态/结果消息
    status: String,
}

impl AiEnvironmentView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut v = Self {
            npm: None,
            tools: Vec::new(),
            mcps: Vec::new(),
            extensions: Vec::new(),
            busy: None,
            status: String::new(),
        };
        // 初始并行跑四组检测（后台）
        v.run_op(AiOp::CheckNpm, cx);
        v.run_op(AiOp::CheckTools, cx);
        v.run_op(AiOp::CheckMcp, cx);
        v.run_op(AiOp::CheckExt, cx);
        v
    }

    /// 后台执行操作（检测或安装/卸载；完成后重扫受影响列表）
    fn run_op(&mut self, op: AiOp, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(op.clone());
        self.status = format!("{}…", op.label());
        cx.notify();

        let weak: WeakEntity<Self> = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            let exec = cx.background_executor().clone();
            let result = exec
                .spawn(async move {
                    let op_for_label = op.label();
                    let exec_res: Result<String, String> = match &op {
                        AiOp::CheckTools => {
                            let _ = environment::check_ai_tools();
                            Ok(String::new())
                        }
                        AiOp::CheckNpm => {
                            let _ = environment::check_npm_environment();
                            Ok(String::new())
                        }
                        AiOp::CheckMcp => {
                            let _ = environment::list_mcp_servers();
                            Ok(String::new())
                        }
                        AiOp::CheckExt => {
                            let _ = environment::list_extensions();
                            Ok(String::new())
                        }
                        AiOp::InstallTool(p) => environment::install_or_upgrade_tool(p),
                        AiOp::UpgradeTool(p) => environment::install_or_upgrade_tool(p),
                        AiOp::UninstallTool(p) => environment::uninstall_ai_tool(p),
                        AiOp::InstallMcp(p) => environment::install_mcp_server(p),
                        AiOp::UninstallMcp(p) => environment::uninstall_mcp_server(p),
                    };
                    (op, op_for_label, exec_res)
                })
                .await;

            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    let (done_op, label, res) = result;
                    this.busy = None;
                    this.status = match res {
                        Ok(msg) => {
                            if msg.is_empty() {
                                format!("{}完成", label)
                            } else {
                                msg
                            }
                        }
                        Err(e) => format!("{}失败：{}", label, e),
                    };
                    // 操作后重扫相关列表
                    match done_op {
                        AiOp::CheckTools | AiOp::InstallTool(_) | AiOp::UpgradeTool(_) | AiOp::UninstallTool(_) => {
                            this.tools = environment::check_ai_tools().tools;
                        }
                        AiOp::CheckNpm => {
                            this.npm = Some(environment::check_npm_environment());
                        }
                        AiOp::CheckMcp | AiOp::InstallMcp(_) | AiOp::UninstallMcp(_) => {
                            this.mcps = environment::list_mcp_servers();
                        }
                        AiOp::CheckExt => {
                            this.extensions = environment::list_extensions();
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
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
        let busy = self.busy.is_some();

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
                            .when(busy, |s| s.text_color(theme.text_muted))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !busy {
                                    this.run_op(AiOp::CheckNpm, cx);
                                    this.run_op(AiOp::CheckTools, cx);
                                    this.run_op(AiOp::CheckMcp, cx);
                                    this.run_op(AiOp::CheckExt, cx);
                                }
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
            .child(self.npm_card(&theme, &npm, busy, cx))
            // AI 工具卡
            .child(self.tools_card(&theme, &tools, busy, cx))
            // MCP 卡 + 扩展卡双列
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_4()
                    .child(self.mcp_card(&theme, &mcps, busy, cx))
                    .child(self.ext_card(&theme, &extensions, busy, cx)),
            )
    }
}

impl AiEnvironmentView {
    fn npm_card(
        &self,
        theme: &Theme,
        npm: &Option<NpmEnvironment>,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !busy {
                                    this.run_op(AiOp::CheckNpm, cx);
                                }
                            }))
                            .child("刷新"),
                    ),
            )
            .when(npm.is_none(), |s| {
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
        busy: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !busy {
                                    this.run_op(AiOp::CheckTools, cx);
                                }
                            }))
                            .child("刷新"),
                    ),
            )
            .when(tools.is_empty() && busy, |s| {
                s.child(crate::ui::table_empty(theme, "检测中…（npm 查询）"))
            })
            .when(tools.is_empty() && !busy, |s| {
                s.child(crate::ui::table_empty(theme, "暂无数据 — 点击「全部刷新」"))
            })
            .children(tools.iter().map(|t| {
                let tool = t.clone();
                let pkg = tool.npm_package.clone();
                let name = tool.display_name.clone();
                let installed = tool.installed;
                let version = tool.version.clone();
                let upgradable = tool.upgradable;
                let busy_row = busy;
                let pkg_upgrade = pkg.clone();
                let pkg_op = pkg.clone();
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
                                    if !busy_row {
                                        this.run_op(AiOp::UpgradeTool(pkg_upgrade.clone()), cx);
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
                                        if !busy_row {
                                            if installed {
                                                this.run_op(AiOp::UninstallTool(pkg_op.clone()), cx);
                                            } else {
                                                this.run_op(AiOp::InstallTool(pkg_op.clone()), cx);
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
        busy: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        crate::ui::table_container(theme)
            .child(
                div()
                    .px_5()
                    .py_3()
                    .text_size(px(14.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child("MCP 服务器"),
            )
            .children(mcps.iter().map(|m| {
                let pkg_uninstall = m.package.clone();
                let pkg_install = m.package.clone();
                let installed = m.installed;
                let busy_row = busy;
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
                                    if !busy_row {
                                        this.run_op(AiOp::UninstallMcp(pkg_uninstall.clone()), cx);
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
                                    if !busy_row {
                                        this.run_op(AiOp::InstallMcp(pkg_install.clone()), cx);
                                    }
                                }))
                                .child("安装"),
                        )
                    })
            }))
            .when(mcps.is_empty(), |s| {
                s.child(crate::ui::table_empty(theme, if busy { "检测中…" } else { "暂无 MCP 数据" }))
            })
    }

    fn ext_card(
        &self,
        theme: &Theme,
        extensions: &[AiExtension],
        busy: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !busy {
                                    this.run_op(AiOp::CheckExt, cx);
                                }
                            }))
                            .child("刷新"),
                    ),
            )
            .when(extensions.is_empty(), |s| {
                s.child(crate::ui::table_empty(theme, if busy { "扫描中…" } else { "未发现扩展（扫描用户目录）" }))
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
