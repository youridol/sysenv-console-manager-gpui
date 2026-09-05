// pi_clone::right_panel — 右侧文件工作台面板
//
// 复刻 AppShell 右面板（reference-spec §3.3）：
//   - tab strip 48px：file tabs（36px、min148、r10、active surface 胶囊）+ 右上 34×28 面板开关
//   - 内容行：预览列（FileViewer 占位 / 空态）+ 文件树列（file-tree 工具栏 44px + 行 27px）
//   - 桌面 ≥960 并排（可拖）；641-959 覆盖层抽屉；≤640 全屏
//
// GPUI 0.2 无 fixed/translateX：compact 抽屉用 absolute 覆盖 + 阴影近似，并伴随
// backdrop（覆盖 Main 区）。移动全屏 = absolute inset 0。

use gpui::{div, px, FontWeight, InteractiveElement, ParentElement, Styled, Window};
use gpui::prelude::*;
use gpui::{Context, SharedString};

use super::data::{self, FileEntry, FileTab};
use super::icons::{self, Icon};
use super::layout;
use super::theme::Palette;
use super::PiShell;

impl PiShell {
    /// Right panel（桌面/覆盖/移动统一进入；宽度与模式已由 AppShell 决策）
    pub fn render_right_panel(
        &mut self,
        _fullscreen: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pal = self.palette();
        let vw = self.viewport_w(window);
        let mobile = layout::is_mobile(vw);
        let compact = layout::is_compact_overlay(vw);

        // 有效宽度
        let w = if mobile {
            vw
        } else if compact {
            // 覆盖层固定 min(560, vw-48)
            vw.min(560.0).max(layout::RIGHT_PANEL_MIN_WIDTH)
        } else {
            self.right_display_w
        };

        // 面板内容（共享，避免重复）
        let content = div()
            .flex_col()
            .size_full()
            .bg(pal.bg)
            .border_l_1()
            .border_color(pal.separator)
            .child(self.tab_strip(&pal, cx))
            .child(self.workbench(&pal, window, cx));

        if compact {
            // 覆盖层抽屉（absolute 顶层，右缘）
            let open = self.right_open;
            let inner = if open { w } else { 0.0 };
            return div()
                .id("pi-right-compact")
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .w(px(inner))
                .flex_shrink_0()
                .overflow_hidden()
                .child(content)
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .w(px(1.0))
                        .bg(pal.separator),
                )
                .when(!open, |s| s.hidden())
                .into_any_element();
        }

        // 桌面并排：flex 列直接参与主 row
        div()
            .id("pi-right-panel")
            .flex_shrink_0()
            .h_full()
            .overflow_hidden()
            .w(px(if self.right_open { w } else { 0.0 }))
            .child(content)
            .into_any_element()
    }

    /// tab strip：文件标签 + 右上 action
    fn tab_strip(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .h(px(48.0))
            .flex_shrink_0()
            .px(px(10.0))
            .gap(px(8.0))
            .bg(pal.surface_muted)
            .child(
                // 文件标签区
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .flex_1()
                    .overflow_x_hidden()
                    .child(
                        if self.file_tabs.is_empty() {
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .h(px(30.0))
                                .text_size(px(11.5))
                                .text_color(pal.text_dim)
                                .child(icons::icon(Icon::File, 14.0).text_color(pal.text_dim))
                                .child(SharedString::from("Files"))
                                .into_any_element()
                        } else {
                            // 标签列表：克隆出 owned 字段供闭包使用
                            let active_id = self.active_file_tab_id.clone();
                            let tab_els: Vec<gpui::AnyElement> = self
                                .file_tabs
                                .iter()
                                .map(|ft| {
                                    let is_active = Some(&ft.id) == active_id.as_ref();
                                    let this2 = cx.entity();
                                    let ft_id = ft.id.clone();
                                    let ft_label = ft.label.clone();
                                    let id_shared = SharedString::from(ft.id.clone());
                                    div()
                                        .id(id_shared)
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .h(px(36.0))
                                        .min_w(px(148.0))
                                        .px(px(12.0))
                                        .pr(px(7.0))
                                        .rounded(px(10.0))
                                        .border_1()
                                        .border_color(if is_active {
                                            pal.border
                                        } else {
                                            super::theme::TRANSPARENT
                                        })
                                        .bg(if is_active {
                                            pal.surface
                                        } else {
                                            super::theme::TRANSPARENT
                                        })
                                        .cursor_pointer()
                                        .text_color(pal.text)
                                        .on_click(move |_ev, _w, cx| {
                                            let _ = this2.update(cx, |t, cx| {
                                                t.active_file_tab_id = Some(ft_id.clone());
                                                cx.notify();
                                            });
                                        })
                                        .child(icons::icon(Icon::File, 14.0).text_color(pal.text_muted))
                                        .child(
                                            div()
                                                .text_size(px(13.0))
                                                .font_weight(FontWeight::MEDIUM)
                                                .truncate()
                                                .child(SharedString::from(ft_label)),
                                        )
                                        .into_any_element()
                                })
                                .collect();
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .children(tab_els)
                                .into_any_element()
                        },
                    ),
            )
            // 右上：面板开关（34×28 is-open）
            .child(self.right_toggle_in_strip(pal, cx))
            .child(self.workbench_action_button(Icon::More, "pi-files-more", pal))
    }

    fn right_toggle_in_strip(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        div()
            .id("pi-right-toggle-strip")
            .flex()
            .items_center()
            .justify_center()
            .w(px(34.0))
            .h(px(28.0))
            .rounded(px(7.0))
            .cursor_pointer()
            .text_color(if self.right_open { pal.text } else { pal.text_muted })
            .bg(if self.right_open {
                pal.bg_hover
            } else {
                super::theme::TRANSPARENT
            })
            .hover(|s| s.bg(pal.bg_hover).text_color(pal.text))
            .on_click(move |_ev, _w, cx| {
                let _ = this.update(cx, |t, cx| t.toggle_right(cx));
            })
            .child(icons::icon(Icon::Panel, 16.0).text_color(if self.right_open { pal.text } else { pal.text_muted }))
    }

    fn workbench_action_button(
        &self,
        icon: Icon,
        id: &'static str,
        pal: &Palette,
    ) -> impl IntoElement {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .w(px(34.0))
            .h(px(28.0))
            .rounded(px(7.0))
            .cursor_pointer()
            .text_color(pal.text_muted)
            .hover(|s| s.bg(pal.bg_hover).text_color(pal.text))
            .child(icons::icon(icon, 16.0).text_color(pal.text_muted))
    }

    /// 工作台主体：预览列 + 文件树列
    fn workbench(
        &self,
        pal: &Palette,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_1()
            .min_h(px(0.0))
            .child(
                // 预览列（flex 1）：FileViewer 占位
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex_col()
                    .child(self.preview_placeholder(pal)),
            )
            // 文件树列（固定 300px）
            .child(self.file_tree_column(pal, cx))
    }

    fn preview_placeholder(&self, pal: &Palette) -> impl IntoElement {
        // FileViewer 空态/占位（file-panel-empty-state）
        let active = self
            .active_file_tab_id
            .as_ref()
            .and_then(|id| self.file_tabs.iter().find(|t| &t.id == id));

        div()
            .flex_1()
            .min_h(px(0.0))
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .child(icons::icon(Icon::File, 22.0).text_color(pal.text_dim))
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(pal.text)
                    .child(SharedString::from(
                        active
                            .map(|t| t.label.clone())
                            .unwrap_or_else(|| "未打开文件".to_string()),
                    )),
            )
            .when(active.is_none(), |s| {
                s.child(
                    div()
                        .text_size(px(12.0))
                        .text_color(pal.text_muted)
                        .child(SharedString::from("从左侧文件树选择文件预览")),
                )
            })
    }

    /// 文件树列（44px 工具栏 + 行）
    fn file_tree_column(&self, pal: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let tree = data::file_tree();
        div()
            .id("pi-file-tree")
            .flex_col()
            .flex_shrink_0()
            .w(px(layout::FILE_TREE_DEFAULT_WIDTH))
            .h_full()
            .border_l_1()
            .border_color(pal.border)
            .child(
                // toolbar：filter 输入 + 按钮
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .h(px(44.0))
                    .px(px(10.0))
                    .py(px(5.0))
                    .border_b_1()
                    .border_color(pal.separator)
                    .child(
                        div()
                            .flex_1()
                            .h(px(28.0))
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .rounded(px(8.0))
                            .border_1()
                            .border_color(pal.separator)
                            .bg(pal.surface_muted)
                            .child(icons::icon(Icon::Search, 12.0).text_color(pal.text_dim))
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(pal.text_dim)
                                    .child(SharedString::from("过滤文件")),
                            ),
                    )
                    .child(self.tool_button(Icon::Dots, "pi-tree-filter-more", pal))
                    .child(self.tool_button(Icon::Plus, "pi-tree-filter-upload", pal)),
            )
            // 文件树滚动区
            .child(
                div()
                    .id("pi-file-tree-scroll")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .px(px(4.0))
                    .py(px(3.0))
                    .children(tree.iter().map(|e| self.tree_row(e, 0, pal, cx))),
            )
    }

    fn tool_button(&self, icon: Icon, id: &'static str, pal: &Palette) -> impl IntoElement {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .size(px(26.0))
            .rounded(px(5.0))
            .cursor_pointer()
            .text_color(pal.text_dim)
            .hover(|s| s.bg(pal.bg_hover).text_color(pal.text))
            .child(icons::icon(icon, 13.0).text_color(pal.text_dim))
    }

    /// 递归文件树行（27px）；点击文件 → 打开标签
    fn tree_row(
        &self,
        entry: &FileEntry,
        depth: usize,
        pal: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let indent = px(8.0 + depth as f32 * 14.0);
        let children: Vec<gpui::AnyElement> = if entry.is_dir {
            entry
                .children
                .iter()
                .map(|c| self.tree_row(c, depth + 1, pal, cx).into_any_element())
                .collect()
        } else {
            Vec::new()
        };

        // 点击文件：打开一个标签并激活右面板
        let is_file = !entry.is_dir;
        let name = entry.name;
        let this = cx.entity();

        let row = div()
            .id(entry.name)
            .flex()
            .items_center()
            .h(px(layout::FILE_TREE_ROW_HEIGHT))
            .pl(indent)
            .pr(px(7.0))
            .gap(px(5.0))
            .rounded(px(7.0))
            .cursor_pointer()
            .text_color(pal.text_muted)
            .hover(|s| s.bg(pal.bg_hover).text_color(pal.text))
            .on_click(move |_ev, _w, cx| {
                if !is_file {
                    return;
                }
                let tab_id = format!("file:{}", name);
                let label = name.to_string();
                let _ = this.update(cx, |t, cx| {
                    // 已存在则激活，否则加入
                    if !t.file_tabs.iter().any(|f| f.id == tab_id) {
                        t.file_tabs.push(FileTab {
                            id: tab_id.clone(),
                            label: label.clone(),
                            file_path: label,
                        });
                    }
                    t.active_file_tab_id = Some(tab_id);
                    t.right_open = true;
                    if t.right_display_w <= 0.0 {
                        t.right_display_w = t.right_panel.width;
                    }
                    cx.notify();
                });
            })
            .child(if entry.is_dir {
                icons::icon(Icon::Folder, 14.0).text_color(pal.text_dim)
            } else {
                icons::icon(Icon::File, 14.0).text_color(pal.text_dim)
            })
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_size(px(12.0))
                    .child(SharedString::from(entry.name)),
            );

        div().flex_col().child(row).children(children)
    }
}




