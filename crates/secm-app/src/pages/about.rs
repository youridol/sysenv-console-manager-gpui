// secm-app::pages::about — 关于页（版本 / 技术栈 / 开源 / 许可）
//
// 渲染层统一走 crate::ui::page 布局框架，色板取自 pi_clone::theme::Palette，
// 明暗外观随壳（PiShell::toggle_theme）通过 set_appearance 联动。

use gpui::prelude::*;
use gpui::{div, px, Context, FontWeight, Render, Window};

use crate::pi_clone::theme::{Appearance, Palette};
use crate::ui::page::{badge, card, card_body, kv_row_w, page_body, page_header, page_root};

pub struct AboutView {
    /// 页面外观，随壳主题联动
    appearance: Appearance,
    /// 页面滚动状态（GPUI 0.2 滚轮需 track_scroll 手动驱动，见 ui::page::page_root）
    page_scroll: gpui::ScrollHandle,
}

impl AboutView {
    pub fn new(appearance: Appearance) -> Self {
        log::info!("关于 · 页面已打开（v{}）", env!("CARGO_PKG_VERSION"));
        Self {
            appearance,
            page_scroll: gpui::ScrollHandle::new(),
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
}

impl Render for AboutView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = self.pal();

        // 版本信息行（文案保持原样，仅呈现样式改由统一框架装配）
        let info_rows: &[(&str, &str)] = &[
            (
                "版本",
                concat!("v", env!("CARGO_PKG_VERSION"), " (GPUI 重构版)"),
            ),
            ("UI 框架", "GPUI 0.2 (Zed, Apache-2.0)"),
            (
                "后端语言",
                "Rust (workspace: secm-app / secm-core / secm-datasource)",
            ),
            (
                "温度传感",
                "LHM sidecar (.NET 8, MPL-2.0 进程隔离)；WinRing0/ACPI 降级链为后续版本计划",
            ),
            ("平台", "Windows 10/11 (x64)"),
            ("许可证", "MIT"),
            ("历史版本", "Tauri + React v1.x（见原仓库）"),
        ];

        page_root(&pal, "about-page-root", &self.page_scroll, &cx.entity())
            // 内容体：滚动壳上的容器 gap 会失效（taffy 0.9.0 纵向 gap 缺陷），
            // 全部子项装入 page_body；纵向留白由 page_header/card 的 .mb(PAGE_GAP) 承担
            .child(
                page_body()
                    // 页头：标题 + 副标题（替代原单独 24px 大标题）
                    .child(page_header(&pal, "关于", "版本 · 技术栈 · 开源许可"))
                    // 产品卡：产品名（右侧徽标）+ 描述行
                    .child(
                        card(&pal).child(
                            card_body(&pal)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap_3()
                                        .child(
                                            div()
                                                .text_size(px(17.0))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(pal.text)
                                                .child("SysEnv Console Manager"),
                                        )
                                        .child(badge(&pal, "GPUI 重构版", pal.accent)),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .text_color(pal.text_muted)
                                        .child("Windows 10/11 系统环境管理工具 — 纯 Rust + GPUI"),
                                ),
                        ),
                    )
                    // 信息卡：键值信息行（标签定宽 110）
                    .child(
                        card(&pal).child(card_body(&pal).children(info_rows.iter().map(|(k, v)| {
                            kv_row_w(&pal, 110.0, *k, *v)
                        }))),
                    )
                    // 底部备注（原文案保留）
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(pal.text_muted)
                            .child("架构决策见 docs/adr/ · 功能基准见 docs/spec/ · MIT © 2026 SysEnv Console Manager"),
                    ),
            )
    }
}
