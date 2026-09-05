// pi_clone::theme — pi-agent-desktop「原生壳」语义色板（Light/Dark 双套）
//
// 复刻基准：Y:\pi-agent-desktop\app\native-theme.css（最终生效层）。
// 主题切换 = class(.dark) 切换（参考实现），此处等价为枚举 + cx.notify 全树刷新。
// 全部组件颜色只允许取自本结构，禁止硬编码业务色（ADR 复刻硬规则 #7）。

#![allow(dead_code)] // 复刻规格预留 API 面（图标/布局常量/面板方法/主题 token/mock 字段），由组件按需取用

use gpui::Rgba;

/// 透明色（Rgba，供 bg/border 分支同型使用）
pub const TRANSPARENT: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

fn rgba8(hex: u32) -> Rgba {
    // 0xRRGGBB 或 0xRRGGBBAA
    if hex <= 0x00ff_ffff {
        gpui::rgb(hex)
    } else {
        gpui::rgba(hex)
    }
}

/// 明暗外观
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

/// 原生语义色板（对齐 native-theme.css :root / html.dark）
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub bg: Rgba,
    pub bg_panel: Rgba,
    pub bg_hover: Rgba,
    pub bg_selected: Rgba,
    pub border: Rgba,
    pub separator: Rgba,
    pub surface: Rgba,
    pub surface_muted: Rgba,
    pub surface_elevated: Rgba,
    pub text: Rgba,
    pub text_muted: Rgba,
    pub text_dim: Rgba,
    pub text_meta: Rgba,
    pub accent: Rgba,
    pub accent_hover: Rgba,
    pub accent_contrast: Rgba,
    pub accent_soft: Rgba,
    pub accent_border: Rgba,
    pub focus_ring: Rgba,
    pub danger: Rgba,
    pub success: Rgba,
    pub warning: Rgba,
    pub user_bg: Rgba,
    pub tool_bg: Rgba,
    pub bg_subtle: Rgba,
    pub scroll_thumb: Rgba,
}

impl Palette {
    pub fn light() -> Self {
        Self {
            bg: rgba8(0xf7f7f5),
            bg_panel: rgba8(0xf1f1efeb), // rgba(241,241,239,.92)
            bg_hover: rgba8(0x1d1d1f0e), // rgba(29,29,31,.055) ≈ 0x0e
            bg_selected: rgba8(0x1d1d1f14), // rgba(29,29,31,.08) ≈ 0x14
            border: rgba8(0x3c3c4329),    // rgba(60,60,67,.16) ≈ 0x29
            separator: rgba8(0x3c3c431f), // rgba(60,60,67,.12) ≈ 0x1f
            surface: rgba8(0xffffff),
            surface_muted: rgba8(0xffffffa3), // rgba(255,255,255,.64) ≈ 0xa3
            surface_elevated: rgba8(0xffffff),
            text: rgba8(0x1d1d1f),
            text_muted: rgba8(0x68686d),
            text_dim: rgba8(0x7a7a7a),
            text_meta: rgba8(0x85858b),
            accent: rgba8(0x1d1d1f),
            accent_hover: rgba8(0x3a3a3c),
            accent_contrast: rgba8(0xffffff),
            accent_soft: rgba8(0x1d1d1f14),
            accent_border: rgba8(0x1d1d1f38),
            focus_ring: rgba8(0x1d1d1f40),
            danger: rgba8(0xd92d20),
            success: rgba8(0x248a3d),
            warning: rgba8(0xb87503),
            user_bg: rgba8(0x1d1d1f0f),
            tool_bg: rgba8(0x76768014),
            bg_subtle: rgba8(0x3c3c430e),
            scroll_thumb: rgba8(0x3c3c4340),
        }
    }

    pub fn dark() -> Self {
        Self {
            bg: rgba8(0x1c1c1e),
            bg_panel: rgba8(0x242426f0), // rgba(36,36,38,.94)
            bg_hover: rgba8(0xffffff13), // rgba(255,255,255,.075) ≈ 0x13
            bg_selected: rgba8(0xffffff1f), // rgba(255,255,255,.12) ≈ 0x1f
            border: rgba8(0xebebf526),    // rgba(235,235,245,.15) ≈ 0x26
            separator: rgba8(0xebebf51a), // rgba(235,235,245,.10) ≈ 0x1a
            surface: rgba8(0x252527),
            surface_muted: rgba8(0x2c2c2eb8), // rgba(44,44,46,.72) ≈ 0xb8
            surface_elevated: rgba8(0x2c2c2e),
            text: rgba8(0xf5f5f7),
            text_muted: rgba8(0xebebf5a3), // rgba(235,235,245,.64) ≈ 0xa3
            text_dim: rgba8(0xebebf575),   // rgba(235,235,245,.46) ≈ 0x75
            text_meta: rgba8(0xebebf570),  // rgba(235,235,245,.44) ≈ 0x70
            accent: rgba8(0xf5f5f7),
            accent_hover: rgba8(0xffffff),
            accent_contrast: rgba8(0x1d1d1f),
            accent_soft: rgba8(0xf5f5f724),
            accent_border: rgba8(0xf5f5f74d),
            focus_ring: rgba8(0xf5f5f752),
            danger: rgba8(0xff6961),
            success: rgba8(0x30d158),
            warning: rgba8(0xffd60a),
            user_bg: rgba8(0xffffff17),
            tool_bg: rgba8(0xffffff0e),
            bg_subtle: rgba8(0xffffff0e),
            scroll_thumb: rgba8(0xebebf540),
        }
    }

    pub fn for_appearance(appearance: Appearance) -> Self {
        match appearance {
            Appearance::Light => Self::light(),
            Appearance::Dark => Self::dark(),
        }
    }
}



