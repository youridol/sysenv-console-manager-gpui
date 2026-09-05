// pi_clone::panel — 可拖拽面板宽度状态机（Sidebar / RightPanel / FileTree 共用）
//
// 复刻基准（useResizablePanel.ts + panel-layout.ts）：
//   - 拖拽：左键按下记录起点，move 更新，up/up_out 结束并持久化
//   - 宽度 clamp 到 [min, max]，max 随窗口宽度与另一面板动态变化
//   - 拖拽期间关闭开合动画（AppShell 依据 is_resizing 开关动画）
//   - 双击 reset 到默认宽度（参考 useResizablePanel 的 resetWidth）

#![allow(dead_code)] // 复刻规格预留 API 面（图标/布局常量/面板方法/主题 token/mock 字段），由组件按需取用
//
// GPUI 0.2 无 pointer capture：on_mouse_move 只在指针位于元素上时回调。
// 为避免拖出命中区丢失跟踪，采用官方 on_drag/on_drag_move 状态机（拖拽一旦开始，
// move 事件持续投递，直到 MouseUp —— 见 gpui-api-notes §1.3）。本结构体维护拖拽
// 语义状态，宿主（AppShell）在其 render 中构造 divider 元素。

/// 面板生长方向：拖动分隔条后宽度增减的方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrowDirection {
    /// 拖向右（左边面板右缘）：dx 增加宽度
    Right,
    /// 拖向左（右边面板左缘）：dx 减少宽度
    Left,
}

/// 一个面板的可变宽度状态
#[derive(Debug, Clone)]
pub struct PanelWidth {
    pub width: f32,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    /// 是否正在拖拽（动画抑制开关）
    pub resizing: bool,
    /// 拖拽起点的指针 x 与宽度
    pub drag_start_x: f32,
    pub drag_start_width: f32,
    /// 本地持久化键
    pub storage_key: &'static str,
}

impl PanelWidth {
    pub fn new(min: f32, max: f32, default: f32, storage_key: &'static str) -> Self {
        let width = load_f32(storage_key).unwrap_or(default);
        Self {
            width: clamp(width, min, max),
            min,
            max,
            default,
            resizing: false,
            drag_start_x: 0.0,
            drag_start_width: 0.0,
            storage_key,
        }
    }

    pub fn set_bounds(&mut self, min: f32, max: f32) {
        self.min = min;
        self.max = max.max(min);
        self.width = clamp(self.width, self.min, self.max);
    }

    /// 把宽度夹到当前 [min,max]
    pub fn reclamp(&mut self) {
        self.width = clamp(self.width, self.min, self.max);
    }

    pub fn begin_drag(&mut self, pointer_x: f32) {
        self.resizing = true;
        self.drag_start_x = pointer_x;
        self.drag_start_width = self.width;
    }

    /// direction=Right：dx 直接相加；Left：dx 取反
    pub fn drag_to(&mut self, pointer_x: f32, dir: GrowDirection) {
        let dx = pointer_x - self.drag_start_x;
        let next = match dir {
            GrowDirection::Right => self.drag_start_width + dx,
            GrowDirection::Left => self.drag_start_width - dx,
        };
        self.width = clamp(next, self.min, self.max);
    }

    pub fn end_drag(&mut self) {
        self.resizing = false;
        self.width = clamp(self.width, self.min, self.max);
        save_f32(self.storage_key, self.width);
    }

    pub fn reset(&mut self) {
        self.width = self.default;
        save_f32(self.storage_key, self.default);
    }

    /// 是否在拖拽中（AppShell 据此关闭开合动画）
    pub fn is_resizing(&self) -> bool {
        self.resizing
    }
}

fn clamp(v: f32, min: f32, max: f32) -> f32 {
    v.max(min).min(max.max(min)).round()
}

/// 宽度持久化文件（参考实现存 localStorage；桌面端等价应用数据目录）
/// 位置：%LOCALAPPDATA%\SECM\pi-panel-widths.json
fn store_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("TEMP").map(std::path::PathBuf::from))?;
    Some(base.join("SECM").join("pi-panel-widths.json"))
}

type Store = std::collections::HashMap<String, f32>;

fn load_store() -> Store {
    let Some(path) = store_path() else {
        return Store::new();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_store(store: &Store) {
    let Some(path) = store_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string(store) {
        let _ = std::fs::write(path, json);
    }
}

fn key(storage_key: &str) -> std::string::String {
    format!("pi-clone::{storage_key}")
}

fn load_f32(storage_key: &str) -> Option<f32> {
    let store = load_store();
    store.get(&key(storage_key)).copied()
}

fn save_f32(storage_key: &str, value: f32) {
    let mut store = load_store();
    store.insert(key(storage_key), value);
    save_store(&store);
}



