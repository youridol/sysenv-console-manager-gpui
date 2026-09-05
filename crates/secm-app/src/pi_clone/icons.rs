// pi_clone::icons — 复刻所需线性图标（GPUI 0.2 单色 SVG 渲染）
//
// 约束（见 gpui-api-notes §5）：

#![allow(dead_code)] // 复刻规格预留 API 面（图标资源为可扩展集合），由组件按需取用
//
// svg() 只能走 AssetSource 注册 + Svg::path(资源路径)，
// 单色渲染（颜色取 .text_color）。故资源文件为黑形透明底，颜色由调用处控制。
// 资源目录：crates/secm-app/assets/pi-icons/*.svg

use gpui::{svg, Svg, Styled};
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// FS 资源源：把 assets/pi-icons/xxx.svg 交给 GPUI
#[derive(Clone)]
pub struct PiAssets {
    base: PathBuf,
}

impl PiAssets {
    pub fn new() -> Self {
        Self {
            base: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"),
        }
    }
}

impl Default for PiAssets {
    fn default() -> Self {
        Self::new()
    }
}

impl gpui::AssetSource for PiAssets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let full = self.base.join(path);
        // 顶层接口只接受相对路径（防止绝对路径越界；资源均为内部静态文件）
        let rel = Path::new(path);
        if rel.is_absolute() {
            return Ok(None);
        }
        match std::fs::read(&full) {
            Ok(bytes) => Ok(Some(Cow::Owned(bytes))),
            Err(_) => Ok(None),
        }
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<gpui::SharedString>> {
        let dir = self.base.join(path);
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    out.push(name.to_string().into());
                }
            }
        }
        Ok(out)
    }
}

/// 图标枚举：资源文件名（black 形状、单色）
#[derive(Debug, Clone, Copy)]
pub enum Icon {
    Search,
    Close,
    Plus,
    Dots,
    ChevronDown,
    FolderOpen,
    Folder,
    Panel,
    Sidebar,
    Sun,
    Moon,
    File,
    More,
    Trash,
    Cpu,
    Wand,
    Puzzle,
    Settings,
    // 工具页导航图标
    Gauge,
    Wifi,
    Sliders,
    Server,
    Box,
    HardDrive,
    Terminal,
    Info,
    // 窗口控制（无标题栏自绘）
    WinMin,
    WinMax,
    WinRestore,
    WinClose,
}

impl Icon {
    fn path(self) -> &'static str {
        match self {
            Icon::Search => "pi-icons/search.svg",
            Icon::Close => "pi-icons/x.svg",
            Icon::Plus => "pi-icons/plus.svg",
            Icon::Dots => "pi-icons/dots.svg",
            Icon::ChevronDown => "pi-icons/chevron-down.svg",
            Icon::FolderOpen => "pi-icons/folder-open.svg",
            Icon::Folder => "pi-icons/folder.svg",
            Icon::Panel => "pi-icons/panel.svg",
            Icon::Sidebar => "pi-icons/sidebar.svg",
            Icon::Sun => "pi-icons/sun.svg",
            Icon::Moon => "pi-icons/moon.svg",
            Icon::File => "pi-icons/file.svg",
            Icon::More => "pi-icons/more.svg",
            Icon::Trash => "pi-icons/trash.svg",
            Icon::Cpu => "pi-icons/cpu.svg",
            Icon::Wand => "pi-icons/wand.svg",
            Icon::Puzzle => "pi-icons/puzzle.svg",
            Icon::Settings => "pi-icons/settings.svg",
            Icon::Gauge => "pi-icons/activity.svg",
            Icon::Wifi => "pi-icons/wifi.svg",
            Icon::Sliders => "pi-icons/sliders.svg",
            Icon::Server => "pi-icons/server.svg",
            Icon::Box => "pi-icons/box.svg",
            Icon::HardDrive => "pi-icons/hard-drive.svg",
            Icon::Terminal => "pi-icons/terminal.svg",
            Icon::Info => "pi-icons/info.svg",
            Icon::WinMin => "pi-icons/win-min.svg",
            Icon::WinMax => "pi-icons/win-max.svg",
            Icon::WinRestore => "pi-icons/win-restore.svg",
            Icon::WinClose => "pi-icons/win-close.svg",
        }
    }
}

/// 构建一个给定像素尺寸的单色 svg 图标
pub fn icon(icon: Icon, size: f32) -> Svg {
    svg().path(icon.path()).size(gpui::px(size))
}










