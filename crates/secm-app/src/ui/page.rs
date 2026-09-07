// secm-app::ui::page — 统一页面布局框架（主内容区现代化排版）
//
// 全部左侧边栏页面的主内容区统一由本模块装配：
//   page_root（根容器）→ page_header（页头）→ card/card_header（卡片）→
//   table_head/table_row（数据表）→ banner（状态反馈）→ button（按钮）
// 颜色一律取自 pi_clone::theme::Palette（明暗双主题，随壳联动），禁止硬编码业务色。
//
// 所有构件返回 gpui::Div，调用方可继续 .id() / .child() / .on_click() 链式装配。

use gpui::prelude::*;
use gpui::{
    canvas, div, point, px, App, Bounds, Div, ElementId, FontWeight, PathBuilder, Pixels, Rgba,
    ScrollHandle, SharedString, Stateful, Window,
};

use crate::pi_clone::theme::{Palette, TRANSPARENT};

// ---------------------------------------------------------------------------
// 布局基准常量（全页面统一节奏）
// ---------------------------------------------------------------------------

/// 页面内边距
pub const PAGE_PADDING: f32 = 24.0;
/// 页面纵向节奏：卡片/卡行/区块之间的统一留白。
/// ⚠ 实现说明（v2.10.4 重要教训）：gpui 0.2.2 + taffy 0.9.0 的容器 `.gap()` 在
/// 纵向（flex_col 主轴 = gap.height）**不生效**（彩色标记实测：三段子块 0 间隙黏连），
/// 仅横向（flex_row 主轴 = gap.width）正常 —— 因此纵向间距一律通过
/// **块级组件的 `.mb(px(PAGE_GAP))` 外边距**实现（page_header/card/banner），
/// 禁止再依赖容器 gap 承担纵向节奏。
pub const PAGE_GAP: f32 = 24.0;
/// 卡片圆角
pub const CARD_RADIUS: f32 = 12.0;
/// 卡片水平内边距
pub const CARD_PADDING: f32 = 20.0;

/// 软色调：同色低透明度（badge/banner/危险强调的底色与描边）
pub fn soft(color: Rgba, alpha: f32) -> Rgba {
    Rgba {
        r: color.r,
        g: color.g,
        b: color.b,
        a: alpha,
    }
}

// ---------------------------------------------------------------------------
// 页面骨架
// ---------------------------------------------------------------------------

/// 页面内容体容器：页面全部子项的统一装载层（非滚动、纵向 flex）。
///
/// ⚠ 纵向间距不使用容器 `.gap()`（taffy 0.9.0 纵向 gap 不生效，见 PAGE_GAP 文档），
/// 由块级组件（page_header/card/banner）的 `.mb(PAGE_GAP)` 承担。页面装配模式：
///   `page_root(...).child(page_body().child(子项A).child(子项B)...)`
pub fn page_body() -> Div {
    div().flex_col().w_full().min_w(px(0.0))
}

/// 页面根容器：统一内边距/超高纵向滚动/页面底色（纵向间隙由 page_body 承担）。
/// id 由调用方传入：滚动交互要求元素持有状态（Stateful），页面级 id 同时作为定位锚点。
/// scroll 由页面视图持有并传入；entity 用于滚轮后 notify 重绘 —— GPUI 0.2 的
/// overflow_y_scroll + track_scroll 原生响应滚轮改写偏移，但不会自动重绘，
/// 必须在事件后调度实体 notify（pi-log-stream 同款机制，window.refresh 时机不对无效）。
pub fn page_root<T: 'static>(
    pal: &Palette,
    id: impl Into<ElementId>,
    scroll: &ScrollHandle,
    entity: &gpui::Entity<T>,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_col()
        .size_full()
        .p(px(PAGE_PADDING))
        // 无 .gap()：纵向（col 主轴）gap 在 taffy 0.9.0 不生效，纵向节奏由
        // page_header/card/banner 的 .mb(PAGE_GAP) 承担（见 PAGE_GAP 文档）
        .bg(pal.bg)
        .overflow_y_scroll()
        .scrollbar_width(px(0.0))
        .track_scroll(scroll)
        .on_scroll_wheel({
            let entity = entity.clone();
            move |_ev: &gpui::ScrollWheelEvent, _window, cx| {
                let _ = entity.update(cx, |_, cx| cx.notify());
            }
        })
}

/// 页头：左侧标题（22px 粗体）+ 副标题（12.5px 弱化）纵向堆叠；
/// 右侧动作区（刷新/运行按钮、状态徽标）由调用方以 `.child(...)` 追加（容器 justify_between）。
/// 自带 `.mb(PAGE_GAP)`：与后续卡片的纵向留白（容器纵向 gap 不可用，见 PAGE_GAP 文档）。
pub fn page_header(
    pal: &Palette,
    title: impl Into<SharedString>,
    subtitle: impl Into<SharedString>,
) -> Div {
    div()
        .flex()
        .items_end()
        .justify_between()
        .gap_3()
        .mb(px(PAGE_GAP))
        .child(
            div()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(22.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(pal.text)
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(pal.text_muted)
                        .child(subtitle.into()),
                ),
        )
}

// ---------------------------------------------------------------------------
// 卡片
// ---------------------------------------------------------------------------

/// 卡片容器：统一圆角/描边/底色/裁切。
/// 自带 `.mb(PAGE_GAP)`：与后续块之间的纵向留白（容器纵向 gap 不可用，见 PAGE_GAP 文档）。
/// 横向并排（flex 行内 flex_1）时 mb 落在行底部，同样承担行间纵向留白。
pub fn card(pal: &Palette) -> Div {
    div()
        .flex_col()
        .rounded(px(CARD_RADIUS))
        .border_1()
        .border_color(pal.border)
        .bg(pal.surface)
        .overflow_hidden()
        .mb(px(PAGE_GAP))
}

/// 卡片头：强调圆点（accent）+ 标题（flex_1 自动把后续 child 推到右侧）
pub fn card_header(pal: &Palette, title: impl Into<SharedString>) -> Div {
    card_header_accent(pal, title, pal.accent)
}

/// 卡片头（自定义强调色圆点，语义分组用：成功绿/警示黄/危险红…）
pub fn card_header_accent(pal: &Palette, title: impl Into<SharedString>, dot: Rgba) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .px(px(CARD_PADDING))
        .py(px(12.0))
        .child(div().size(px(7.0)).rounded_full().flex_shrink_0().bg(dot))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_size(px(14.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(pal.text)
                .child(title.into()),
        )
}

/// 卡片内分隔线
pub fn card_divider(pal: &Palette) -> Div {
    div().h(px(1.0)).w_full().flex_shrink_0().bg(pal.border)
}

/// 卡片内容体：统一水平内边距与纵向节奏。
/// ⚠ 内部 `.gap(8)` 的纵向分量在 taffy 0.9.0 同样不生效（见 PAGE_GAP 文档），
/// 卡内子块的纵向节奏由调用方以 `.mt()` 显式补足（如 dashboard 统计卡）。
pub fn card_body(_pal: &Palette) -> Div {
    div()
        .flex_col()
        .px(px(CARD_PADDING))
        .py(px(12.0))
        .gap(px(8.0))
}

// ---------------------------------------------------------------------------
// 状态横幅
// ---------------------------------------------------------------------------

/// 状态反馈类型（Success/Warn 为框架预留语义位：当前页面反馈以 Info/Danger 为主，
/// 操作成功/部分失败类页面后续按需取用）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum BannerKind {
    /// 中性信息（加载中/就绪提示）
    Info,
    /// 成功
    Success,
    /// 警告
    Warn,
    /// 危险/失败
    Danger,
}

/// 状态横幅：圆点 + 软底色 + 语义描边（操作反馈/加载提示/错误）。
/// 自带 `.mb(PAGE_GAP)`：与后续卡片的纵向留白（容器纵向 gap 不可用，见 PAGE_GAP 文档）。
pub fn banner(pal: &Palette, kind: BannerKind, text: impl Into<SharedString>) -> Div {
    let (dot, bg, border, fg) = match kind {
        BannerKind::Info => (pal.text_muted, pal.bg_subtle, pal.border, pal.text),
        BannerKind::Success => (
            pal.success,
            soft(pal.success, 0.10),
            soft(pal.success, 0.30),
            pal.success,
        ),
        BannerKind::Warn => (
            pal.warning,
            soft(pal.warning, 0.12),
            soft(pal.warning, 0.32),
            pal.warning,
        ),
        BannerKind::Danger => (
            pal.danger,
            soft(pal.danger, 0.10),
            soft(pal.danger, 0.30),
            pal.danger,
        ),
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .px(px(14.0))
        .py(px(9.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(border)
        .bg(bg)
        .mb(px(PAGE_GAP))
        .child(div().size(px(7.0)).rounded_full().flex_shrink_0().bg(dot))
        .child(
            div()
                .min_w(px(0.0))
                .text_size(px(12.0))
                .text_color(fg)
                .child(text.into()),
        )
}

// ---------------------------------------------------------------------------
// 按钮
// ---------------------------------------------------------------------------

/// 按钮语义类型（Primary=主操作/accent 实底；Secondary=次操作/悬停底+描边；
/// Ghost=弱化文本钮；Danger/Warning=软底语义描边）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
    Danger,
    Warning,
}

/// 标准按钮骨架（h32；调用方接 `.id(...).child(文本).on_click(...)`，顺序不可颠倒）
pub fn button(pal: &Palette, kind: ButtonKind) -> Div {
    button_base(pal, kind, 32.0, 14.0, 12.5)
}

/// 行内小按钮（表格行操作；h24）
pub fn button_sm(pal: &Palette, kind: ButtonKind) -> Div {
    button_base(pal, kind, 24.0, 8.0, 11.0)
}

fn button_base(pal: &Palette, kind: ButtonKind, h: f32, pad_x: f32, font: f32) -> Div {
    let (bg, hover, fg, border) = match kind {
        ButtonKind::Primary => (
            pal.accent,
            pal.accent_hover,
            pal.accent_contrast,
            TRANSPARENT,
        ),
        ButtonKind::Secondary => (pal.bg_hover, pal.bg_selected, pal.text, pal.border),
        ButtonKind::Ghost => (TRANSPARENT, pal.bg_hover, pal.text_muted, TRANSPARENT),
        ButtonKind::Danger => (
            soft(pal.danger, 0.14),
            soft(pal.danger, 0.26),
            pal.danger,
            soft(pal.danger, 0.40),
        ),
        ButtonKind::Warning => (
            soft(pal.warning, 0.14),
            soft(pal.warning, 0.26),
            pal.warning,
            soft(pal.warning, 0.40),
        ),
    };
    div()
        .flex()
        .items_center()
        .justify_center()
        .h(px(h))
        .px(px(pad_x))
        .rounded(px(8.0))
        .cursor_pointer()
        .border_1()
        .border_color(border)
        .bg(bg)
        .hover(move |s| s.bg(hover))
        .text_size(px(font))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
}

// ---------------------------------------------------------------------------
// 数据表
// ---------------------------------------------------------------------------

/// 表列宽规格（表头与数据行使用同一规格保证对齐；原 flex_1 列用 Flex，固定宽列用 Px）
#[derive(Debug, Clone, Copy)]
pub enum ColWidth {
    Flex,
    Px(f32),
}

fn col_cell(width: ColWidth) -> Div {
    match width {
        ColWidth::Flex => div().flex_1().min_w(px(0.0)),
        ColWidth::Px(w) => div().flex_none().w(px(w)),
    }
}

/// 数据表表头（列宽规格化；bg_subtle 底 + 底描边；列名为 'static 字面量）
pub fn table_head(pal: &Palette, cols: &[(&'static str, ColWidth)]) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .px(px(CARD_PADDING))
        .py(px(10.0))
        .bg(pal.bg_subtle)
        .border_b_1()
        .border_color(pal.border)
        .children(cols.iter().map(|(label, width)| {
            col_cell(*width)
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(pal.text_muted)
                .child(SharedString::from(*label))
        }))
}

/// 数据表行骨架（px20/py9 + 底描边；调用方按相同 ColWidth 装配单元格，可再接 hover/id/on_click）
pub fn table_row(pal: &Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .px(px(CARD_PADDING))
        .py(px(9.0))
        .border_b_1()
        .border_color(pal.border)
}

/// 表格/卡片空态提示（居中弱化文本）
pub fn table_empty(pal: &Palette, msg: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .px(px(CARD_PADDING))
        .py(px(36.0))
        .child(
            div()
                .text_size(px(12.5))
                .text_color(pal.text_muted)
                .child(msg.into()),
        )
}

// ---------------------------------------------------------------------------
// 信息行 / 徽标 / 小节
// ---------------------------------------------------------------------------

/// 键值信息行（自定义标签宽；空值语义由调用方处理）
pub fn kv_row_w(
    pal: &Palette,
    label_w: f32,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .py(px(3.0))
        .child(
            div()
                .flex_none()
                .w(px(label_w))
                .text_size(px(12.0))
                .text_color(pal.text_muted)
                .child(label.into()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(12.5))
                .text_color(pal.text)
                .child(value.into()),
        )
}

/// 状态点 + 彩色文本（表格状态列/页头徽标；色由调用方语义指定，形参保留以维持签名一致性）
pub fn status_pill(_pal: &Palette, text: impl Into<SharedString>, color: Rgba) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(div().size(px(6.0)).rounded_full().bg(color))
        .child(
            div()
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(color)
                .child(text.into()),
        )
}

/// 软底徽章（语义色 pill：权限标记/可升级标记等；色由调用方语义指定，形参保留以维持签名一致性）
pub fn badge(_pal: &Palette, text: impl Into<SharedString>, color: Rgba) -> Div {
    div()
        .flex()
        .items_center()
        .px(px(8.0))
        .h(px(20.0))
        .rounded_full()
        .border_1()
        .border_color(soft(color, 0.35))
        .bg(soft(color, 0.12))
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(color)
        .child(text.into())
}

/// 卡内小节标题（小圆点 + 13px 文本）
pub fn section_title(pal: &Palette, text: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().size(px(4.0)).rounded_full().bg(pal.accent))
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(pal.text)
                .child(text.into()),
        )
}

/// 表单字段标签（输入框上方 11px 弱化说明）
pub fn field_label(pal: &Palette, text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(11.0))
        .text_color(pal.text_muted)
        .child(text.into())
}

/// 大号指标数值（统计卡主值）
pub fn metric_value(pal: &Palette, text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(28.0))
        .font_weight(FontWeight::BOLD)
        .text_color(pal.text)
        .child(text.into())
}

// ---------------------------------------------------------------------------
// 趋势图 / 进度条
// ---------------------------------------------------------------------------

/// 趋势图高度
pub const CHART_HEIGHT: f32 = 56.0;

/// 迷你趋势图（波浪线 sparkline，v2.11.0）：Catmull-Rom 平滑曲线 + 曲线下方面积
/// 渐隐填充 + 底部语义色基线。输入为按时间升序的数值序列（60s 窗口内采样点）；
/// 量程 = 序列最大值与 1.0 取大。
///
/// 渲染走 gpui canvas + PathBuilder（矢量描边/填充），替代旧"等宽柱状"实现 ——
/// 全部趋势图（CPU/内存/GPU/网络上下行）共用本函数，一处改全线生效。
pub fn sparkline(values: &[f32], color: Rgba) -> Div {
    let data: Vec<f32> = values.to_vec();
    div()
        .w_full()
        .h(px(CHART_HEIGHT))
        .border_b_1()
        .border_color(soft(color, 0.35))
        .child(
            canvas(
                // prepaint：把采样序列带进 paint 阶段（canvas 回调要求 'static）
                move |_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| data,
                move |bounds: Bounds<Pixels>,
                      data: Vec<f32>,
                      window: &mut Window,
                      _cx: &mut App| {
                    draw_wave_chart(bounds, &data, color, window);
                },
            )
            .w_full()
            .h_full(),
        )
}

/// 波浪线绘制：把序列归一化到画布，Catmull-Rom 样条转三次贝塞尔后
/// ① 以 `soft(color, 0.14)` 填充曲线下方面积；② 以 1.8px 描边绘制平滑曲线。
/// 坐标数学统一走 f32 域（Pixels 内部字段 crate 外不可见，px()/除法仅作换算）。
fn draw_wave_chart(bounds: Bounds<Pixels>, values: &[f32], color: Rgba, window: &mut Window) {
    let n = values.len();
    if n == 0 {
        return;
    }
    // Pixels → f32（Div<Pixels> 的 Output = f32）
    let (ox, oy) = (bounds.origin.x / px(1.0), bounds.origin.y / px(1.0));
    let (w, h) = (bounds.size.width / px(1.0), bounds.size.height / px(1.0));
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let pad_top = 4.0;
    let pad_bottom = 3.0;
    let usable_h = (h - pad_top - pad_bottom).max(1.0);

    let max = values.iter().cloned().fold(1.0f32, f32::max).max(1.0);
    let denom = if n > 1 { (n - 1) as f32 } else { 1.0 };
    let x_at = |i: usize| ox + w * (i as f32 / denom);
    let y_at = |v: f32| {
        let frac = (v / max).clamp(0.02, 1.0);
        oy + pad_top + usable_h * (1.0 - frac)
    };
    // 单点：水平拉平为全宽直线
    let pts: Vec<(f32, f32)> = if n == 1 {
        let y = y_at(values[0]);
        vec![(ox, y), (ox + w, y)]
    } else {
        (0..n).map(|i| (x_at(i), y_at(values[i]))).collect()
    };
    let count = pts.len();

    // Catmull-Rom（uniform）→ 三次贝塞尔控制点：段 i 由 pts[i]→pts[i+1]，
    // c1 = P1 + (P2-P0)/6，c2 = P2 - (P3-P1)/6（端点做钳制）
    let seg_controls = |i: usize| -> ((f32, f32), (f32, f32)) {
        let (p0x, p0y) = pts[i.saturating_sub(1)];
        let (p1x, p1y) = pts[i];
        let (p2x, p2y) = pts[(i + 1).min(count - 1)];
        let (p3x, p3y) = pts[(i + 2).min(count - 1)];
        let c1 = (p1x + (p2x - p0x) / 6.0, p1y + (p2y - p0y) / 6.0);
        let c2 = (p2x - (p3x - p1x) / 6.0, p2y - (p3y - p1y) / 6.0);
        (c1, c2)
    };

    // ① 曲线下方面积填充（波浪渐隐）
    let mut fill = PathBuilder::fill();
    fill.move_to(point(px(pts[0].0), px(pts[0].1)));
    if count > 1 {
        for i in 0..count - 1 {
            let ((c1x, c1y), (c2x, c2y)) = seg_controls(i);
            fill.cubic_bezier_to(
                point(px(pts[i + 1].0), px(pts[i + 1].1)),
                point(px(c1x), px(c1y)),
                point(px(c2x), px(c2y)),
            );
        }
    }
    fill.line_to(point(px(ox + w), px(oy + h)));
    fill.line_to(point(px(ox), px(oy + h)));
    fill.close();
    if let Ok(path) = fill.build() {
        window.paint_path(path, soft(color, 0.14));
    }

    // ② 波浪描边（平滑曲线主线）
    let mut stroke = PathBuilder::stroke(px(1.8));
    stroke.move_to(point(px(pts[0].0), px(pts[0].1)));
    if count > 1 {
        for i in 0..count - 1 {
            let ((c1x, c1y), (c2x, c2y)) = seg_controls(i);
            stroke.cubic_bezier_to(
                point(px(pts[i + 1].0), px(pts[i + 1].1)),
                point(px(c1x), px(c1y)),
                point(px(c2x), px(c2y)),
            );
        }
    }
    if let Ok(path) = stroke.build() {
        window.paint_path(path, soft(color, 0.95));
    }
}

/// 空趋势占位（与 sparkline 等高，居中弱化文案）
pub fn sparkline_empty(pal: &Palette, text: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .w_full()
        .h(px(CHART_HEIGHT))
        .border_b_1()
        .border_color(pal.border)
        .child(
            div()
                .text_size(px(11.0))
                .text_color(pal.text_dim)
                .child(text.into()),
        )
}
