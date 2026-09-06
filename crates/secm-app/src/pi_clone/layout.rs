// pi_clone::layout — 布局常量与面板宽度计算（对齐 lib/panel-layout.ts + 实测 CSS）
//
// 复刻基准常量：
//   MOBILE_MAX_WIDTH=640, SPLIT_PANEL_MIN_WIDTH=960
//   SIDEBAR 默认 260 / min 180 / max 480
//   RIGHT_PANEL fallback 560 / min 300 / max 1200 / 默认 clamp(0.42vw, 360, 640)
//   中央列保底宽度：紧凑 320 / 桌面 420

#![allow(dead_code)] // 复刻规格预留 API 面（图标/布局常量/面板方法/主题 token/mock 字段），由组件按需取用

/// 断点
pub const MOBILE_MAX_WIDTH: f32 = 640.0;
pub const SPLIT_PANEL_MIN_WIDTH: f32 = 960.0;

/// Sidebar 面板宽度（px）
pub const SIDEBAR_DEFAULT_WIDTH: f32 = 260.0;
pub const SIDEBAR_MIN_WIDTH: f32 = 180.0;
pub const SIDEBAR_MAX_WIDTH: f32 = 480.0;

/// 右侧文件面板宽度（px）
pub const RIGHT_PANEL_FALLBACK_WIDTH: f32 = 560.0;
pub const RIGHT_PANEL_MIN_WIDTH: f32 = 300.0;
pub const RIGHT_PANEL_MAX_WIDTH: f32 = 1200.0;

/// 中央列（Main）保底宽度
const COMPACT_MAIN_MIN_WIDTH: f32 = 320.0;
const DESKTOP_MAIN_MIN_WIDTH: f32 = 420.0;

/// 文件树宽度
pub const FILE_TREE_DEFAULT_WIDTH: f32 = 300.0;
pub const FILE_TREE_MIN_WIDTH: f32 = 220.0;
pub const FILE_TREE_MAX_WIDTH: f32 = 520.0;

/// 顶栏高度（native-theme.css 48px !important 覆盖 JSX 36+safe-area）
pub const TOP_BAR_HEIGHT: f32 = 48.0;
/// 顶栏里 sidebar 展开按钮尺寸（AppShell TOP_BAR_ICON_BUTTON_SIZE=36）
pub const TOP_BAR_ICON_BUTTON_SIZE: f32 = 36.0;
/// 右面板开关按钮 34×28（fixed top10 right10）
pub const RIGHT_PANEL_TOGGLE_W: f32 = 34.0;
pub const RIGHT_PANEL_TOGGLE_H: f32 = 28.0;
/// 文件树行高
pub const FILE_TREE_ROW_HEIGHT: f32 = 27.0;

/// clamp：不越 [min,max] 并取整
pub fn clamp_width(width: f32, min_width: f32, max_width: f32) -> f32 {
    let effective_max = max_width.max(min_width);
    width.max(min_width).min(effective_max).round()
}

/// 右面板默认宽度 = clamp(viewport*0.42, 360, 640)（仅在桌面分栏语义下使用）
pub fn default_right_panel_width(viewport_width: f32) -> f32 {
    clamp_width(viewport_width * 0.42, 360.0, 640.0)
}

/// Sidebar 可拖拽的最大宽度（只扣右侧栏已占宽度，避免两栏重叠；
/// Main 自适应伸缩，无独立保底 —— 拉左栏只压缩 Main，右栏不动）
pub fn sidebar_max_width(viewport_width: f32, right_panel_open: bool, right_panel_width: f32) -> f32 {
    if viewport_width <= MOBILE_MAX_WIDTH {
        return SIDEBAR_MAX_WIDTH;
    }
    let visible_right = if right_panel_open { right_panel_width } else { 0.0 };
    (SIDEBAR_MAX_WIDTH)
        .min(viewport_width - visible_right - 24.0)
        .max(SIDEBAR_MIN_WIDTH)
        .round()
}

/// 右面板可拖拽最大宽度（只扣左侧栏已占宽度，避免两栏重叠；
/// Main 自适应伸缩 —— 拉右栏只压缩 Main，左栏不动）
pub fn right_panel_max_width(viewport_width: f32, sidebar_open: bool, sidebar_width: f32) -> f32 {
    if viewport_width < SPLIT_PANEL_MIN_WIDTH {
        return RIGHT_PANEL_MAX_WIDTH;
    }
    let visible_sidebar = if sidebar_open { sidebar_width } else { 0.0 };
    (RIGHT_PANEL_MAX_WIDTH)
        .min(viewport_width - visible_sidebar - 24.0)
        .max(RIGHT_PANEL_MIN_WIDTH)
        .round()
}

/// 是否为移动断点（<=640）
pub fn is_mobile(viewport_width: f32) -> bool {
    viewport_width <= MOBILE_MAX_WIDTH
}

/// 是否进入并排三栏（>=960）
pub fn is_split_panel(viewport_width: f32) -> bool {
    viewport_width >= SPLIT_PANEL_MIN_WIDTH
}

/// 641-959 的紧凑桌面（右面板变覆盖层抽屉，不挤压中央列）
pub fn is_compact_overlay(viewport_width: f32) -> bool {
    viewport_width > MOBILE_MAX_WIDTH && viewport_width < SPLIT_PANEL_MIN_WIDTH
}






