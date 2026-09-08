// secm-app::ui::toast — 全局泡泡提示系统（Toast / 右上角）
//
// 设计（用户需求 v3.3.0）：
//   - 全局右上角泡泡提示，对必要操作提供 成功/警告/错误/提醒 四类反馈；
//   - 默认 1.6s 后自动消失（1–2s 区间），支持手动关闭（×）；
//   - 样式/动画/层级/间距全局统一：任何页面经 `toast::success(..)` 等
//     自由函数推送，渲染统一由本模块 `render_stack` 装配（禁止页面级自绘）。
//
// 结构：
//   ToastHost — 提示队列实体（壳持有），变更时 emit ToastEvent 驱动壳重绘；
//   ToastGlobal — GPUI Global（WeakEntity<ToastHost>），任意 Context 可推送；
//   render_stack — 右上角覆盖层装配（入场动画 + 语义色 + 关闭按钮）。

use std::collections::VecDeque;

use gpui::prelude::*;
use gpui::{
    div, px, Animation, AnimationExt, App, Context, Entity, EventEmitter, FontWeight, SharedString,
    WeakEntity,
};

use crate::pi_clone::icons::{self, Icon};
use crate::pi_clone::theme::{current_palette, Palette};
use crate::ui::page::soft;

/// 提示语义类型（色板取 Palette 语义色，明暗随壳联动）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    /// 成功（绿）
    Success,
    /// 警告（黄）
    Warning,
    /// 错误（红）
    Error,
    /// 提醒（中性）
    Info,
}

impl ToastKind {
    /// 语义色（描边/色条/图标/类别标签色）
    fn color(self, pal: &Palette) -> gpui::Rgba {
        match self {
            ToastKind::Success => pal.success,
            ToastKind::Warning => pal.warning,
            ToastKind::Error => pal.danger,
            ToastKind::Info => pal.accent,
        }
    }

    /// 语义标签（类别小字，快速识别）
    fn label(self) -> &'static str {
        match self {
            ToastKind::Success => "成功",
            ToastKind::Warning => "警告",
            ToastKind::Error => "错误",
            ToastKind::Info => "提醒",
        }
    }

    /// 语义图标（toast-* 为本次新增线性图标，info 复用现有资源）
    fn icon(self) -> Icon {
        match self {
            ToastKind::Success => Icon::ToastSuccess,
            ToastKind::Warning => Icon::ToastWarning,
            ToastKind::Error => Icon::ToastError,
            ToastKind::Info => Icon::Info,
        }
    }
}

/// 单条提示
#[derive(Debug, Clone)]
pub struct Toast {
    id: u64,
    kind: ToastKind,
    message: SharedString,
}

/// 队列变更事件（壳订阅后 notify 重绘泡泡层）
#[derive(Debug)]
pub struct ToastEvent;

/// 提示宿主：队列 + 生命周期（自动消失定时器）
pub struct ToastHost {
    toasts: VecDeque<Toast>,
    /// 与 toasts 一一对应的入队时刻（ms 时间戳；自动消失判据）
    borns: VecDeque<u64>,
    next_id: u64,
}

/// 单条展示时长（ms）：需求 1–2s 自动消失，取 1.6s 居中
const TOAST_TTL_MS: u64 = 1600;
/// 同屏最大条数（超出丢弃最旧，防刷屏遮内容）
const TOAST_CAP: usize = 4;
/// 入场动画时长（ms）：淡入 + 轻微下滑
const ENTRANCE_MS: u64 = 170;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Default for ToastHost {
    fn default() -> Self {
        Self {
            toasts: VecDeque::new(),
            borns: VecDeque::new(),
            next_id: 1,
        }
    }
}

impl EventEmitter<ToastEvent> for ToastHost {}

impl ToastHost {
    /// 入队一条提示（同屏超限丢最旧；变更后广播事件并调度自动消失）
    fn push(&mut self, kind: ToastKind, message: SharedString, cx: &mut Context<Self>) {
        let id = self.next_id;
        self.next_id += 1;
        self.toasts.push_back(Toast { id, kind, message });
        self.borns.push_back(now_ms());
        while self.toasts.len() > TOAST_CAP {
            self.toasts.pop_front();
            self.borns.pop_front();
        }
        cx.emit(ToastEvent);
        self.schedule_prune(TOAST_TTL_MS, cx);
    }

    /// 手动关闭（点击 ×）
    fn close(&mut self, id: u64, cx: &mut Context<Self>) {
        self.retain_except(id);
        cx.emit(ToastEvent);
        // 关闭后若仍有未到期条目，按最近到期剩余时长续约
        if let Some(&front_born) = self.borns.front() {
            let remain = TOAST_TTL_MS
                .saturating_sub(now_ms().saturating_sub(front_born))
                .max(1);
            self.schedule_prune(remain, cx);
        }
    }

    /// 清理全部到期条目（自动消失定时器回调；空转时不再续约）
    fn prune(&mut self, cx: &mut Context<Self>) {
        let now = now_ms();
        let mut expired = false;
        loop {
            // 逐字段取值避免跨可变借用的迭代器持有（front 值先拷贝再判）
            let (Some(&front_born), has_toast) = (self.borns.front(), !self.toasts.is_empty())
            else {
                break;
            };
            if !has_toast || now.saturating_sub(front_born) < TOAST_TTL_MS {
                break;
            }
            self.toasts.pop_front();
            self.borns.pop_front();
            expired = true;
        }
        if expired {
            cx.emit(ToastEvent);
        }
        // 队列非空说明还有未到期条目 → 按最近到期剩余时长续约
        if let Some(&front_born) = self.borns.front() {
            let remain = TOAST_TTL_MS
                .saturating_sub(now.saturating_sub(front_born))
                .max(1);
            self.schedule_prune(remain, cx);
        }
    }

    /// 按 id 移除（toasts 与 borns 索引严格对齐，同步删两列）
    fn retain_except(&mut self, id: u64) {
        let mut kept = 0usize;
        self.toasts.retain(|t| {
            let keep = t.id != id;
            if keep {
                kept += 1;
            } else {
                self.borns.remove(kept);
            }
            keep
        });
    }

    /// 调度一次到期清理（weak 升级失败 = 壳已释放，静默退出）
    fn schedule_prune(&self, delay_ms: u64, cx: &mut Context<Self>) {
        cx.spawn(
            async move |this: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                gpui::Timer::after(std::time::Duration::from_millis(delay_ms)).await;
                if let Some(host) = this.upgrade() {
                    host.update(cx, |h, cx| h.prune(cx)).ok();
                }
            },
        )
        .detach();
    }
}

// ---------------------------------------------------------------------------
// 全局推送入口（页面侧 API）
// ---------------------------------------------------------------------------

/// GPUI 全局句柄：壳初始化时写入，页面任意 Context 经自由函数推送
pub struct ToastGlobal(WeakEntity<ToastHost>);

impl gpui::Global for ToastGlobal {}

/// 初始化：创建宿主实体并写入全局（壳启动时调用一次）
pub fn init(cx: &mut App) -> Entity<ToastHost> {
    let host = cx.new(|_| ToastHost::default());
    cx.set_global(ToastGlobal(host.downgrade()));
    host
}

/// 统一推送（App 上下文；页面 Context/异步回填处 deref 可直接传）
fn push(kind: ToastKind, message: SharedString, cx: &mut App) {
    let Some(global) = cx.try_global::<ToastGlobal>() else {
        return;
    };
    let weak = global.0.clone();
    let _ = weak.update(cx, |host, cx| host.push(kind, message, cx));
}

/// 成功提示
pub fn success(message: impl Into<SharedString>, cx: &mut App) {
    push(ToastKind::Success, message.into(), cx);
}

/// 警告提示
pub fn warning(message: impl Into<SharedString>, cx: &mut App) {
    push(ToastKind::Warning, message.into(), cx);
}

/// 错误提示
pub fn error(message: impl Into<SharedString>, cx: &mut App) {
    push(ToastKind::Error, message.into(), cx);
}

/// 中性提醒
pub fn info(message: impl Into<SharedString>, cx: &mut App) {
    push(ToastKind::Info, message.into(), cx);
}

// ---------------------------------------------------------------------------
// 渲染（中间显示区右上角覆盖层；页面禁止自绘泡泡）
// ---------------------------------------------------------------------------

/// 泡泡栈覆盖层：由壳挂载到**中间显示区**（Main 列页面区容器，定位上下文）内、
/// 钉其右上角 —— 不覆盖侧栏/右栏/顶栏。作为页面区最后子元素 → 绘制在最上层。
/// 覆盖层本体无事件监听 → 空白处点击穿透至下层内容；仅泡泡关闭钮可交互。
pub fn render_stack(host: &Entity<ToastHost>, cx: &App) -> gpui::Stateful<gpui::Div> {
    let pal = current_palette(cx);
    let toasts: Vec<Toast> = host.read(cx).toasts.iter().cloned().collect();
    let mut layer = div()
        .id("toast-layer")
        .absolute()
        // 页面区内边距节奏：距显示区上/右缘各 12px（页面内容 24px 内边距之内）
        .top(px(12.0))
        .right(px(12.0))
        .flex()
        .flex_col()
        .items_end()
        .gap(px(8.0));
    for toast in toasts {
        layer = layer.child(render_toast(&pal, toast, host.clone()));
    }
    layer
}

/// 单条泡泡（语义色条 + 图标 + 类别 + 消息 + 手动关闭；入场淡入下滑动画）
fn render_toast(pal: &Palette, toast: Toast, host: Entity<ToastHost>) -> impl IntoElement {
    let color = toast.kind.color(pal);
    let label = toast.kind.label();
    let message = toast.message.clone();
    let close_id = SharedString::from(format!("toast-close-{}", toast.id));
    let anim_id = SharedString::from(format!("toast-{}", toast.id));

    div()
        .id(SharedString::from(format!("toast-item-{}", toast.id)))
        .flex()
        .items_center()
        .gap(px(8.0))
        .max_w(px(420.0))
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(10.0))
        .bg(pal.surface_elevated)
        .border_1()
        .border_color(soft(color, 0.45))
        // 左侧语义色条
        .child(
            div()
                .w(px(3.0))
                .h(px(16.0))
                .rounded_full()
                .flex_shrink_0()
                .bg(color),
        )
        .child(
            icons::icon(toast.kind.icon(), 13.0)
                .text_color(color)
                .flex_shrink_0(),
        )
        .child(
            div()
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .flex_shrink_0()
                .child(SharedString::from(label)),
        )
        .child(
            div()
                .min_w(px(0.0))
                .whitespace_nowrap()
                .truncate()
                .text_size(px(12.0))
                .text_color(pal.text)
                .child(message),
        )
        .child(
            // 手动关闭（18px 命中区，弱化色悬停增强）
            div()
                .id(close_id)
                .flex()
                .items_center()
                .justify_center()
                .size(px(18.0))
                .rounded(px(5.0))
                .cursor_pointer()
                .flex_shrink_0()
                .text_color(pal.text_muted)
                .hover(move |s| s.bg(pal.bg_hover).text_color(pal.text))
                .on_click(move |_ev, _window, cx: &mut gpui::App| {
                    let _ = host.update(cx, |h, cx| h.close(toast.id, cx));
                })
                .child(icons::icon(Icon::Close, 9.0)),
        )
        // 入场动画：opacity 0→1 + 上方 10px 下滑归位（easing 取 gpui 内置 quint）
        .with_animation(
            anim_id,
            Animation::new(std::time::Duration::from_millis(ENTRANCE_MS))
                .with_easing(gpui::ease_out_quint()),
            |el, delta| el.opacity(delta).mt(px(2.0 - 10.0 * (1.0 - delta))),
        )
}
