// secm-app::theme — 主题色板（对齐源 shadcn/Tailwind 视觉：zinc + brand + 状态色）

use gpui::Rgba;

/// 主题（深/浅两套色板；当前以深色为主，浅色为扩展预留）
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg: Rgba,
    pub panel: Rgba,
    pub panel_hover: Rgba,
    pub border: Rgba,
    pub text: Rgba,
    pub text_muted: Rgba,
    pub brand: Rgba,
    pub success: Rgba,
    pub warn: Rgba,
    pub danger: Rgba,
    pub info: Rgba,
}

fn rgba(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

impl Theme {
    /// 深色主题（默认；对齐源深色视觉：zinc-950 背景 + 品牌蓝/紫渐变）
    pub fn dark() -> Self {
        Self {
            bg: rgba(0x0f1015),            // 近似 zinc-950
            panel: rgba(0x1a1c25),         // 卡片/面板
            panel_hover: rgba(0x242736),
            border: rgba(0x2e3348),
            text: rgba(0xe8eaf2),
            text_muted: rgba(0x9aa2b5),
            brand: rgba(0x4f7cff),         // brand-blue
            success: rgba(0x4ade80),
            warn: rgba(0xfbbf24),
            danger: rgba(0xf87171),
            info: rgba(0x38bdf8),
        }
    }
}
