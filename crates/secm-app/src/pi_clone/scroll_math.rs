// pi_clone::scroll_math — 自绘滚动条纯换算（thumb 几何 + thumb 拖动偏移）
//
// 与 GPUI 0.2 ScrollHandle 语义对齐（实测 probe 确认）：
//   - handle.max_offset().height = (内容高 - 视口高).max(0)，恒 ≥0 的正可滚动量；
//   - handle.offset().y ∈ [-max_offset, 0]（下滚为负）。
// 历史缺陷：v2.8.0-2.8.3 把 max_offset 误当负数（max_off<0 才可滚），导致几何
// 恒 (0,0,false) 且拖动换算在早退处直接 return —— 滚动条 thumb 显示/拖动失灵根因。
// 抽出纯函数供 right_panel(几何) / shell(拖动) / 诊断 probe 共用同一实现。

/// thumb 几何：(top, height, scrollable)
pub fn thumb_geometry(viewport: f32, max_off: f32, offset_y: f32) -> (f32, f32, bool) {
    if viewport <= 0.0 || max_off <= 0.0 {
        return (0.0, 0.0, false);
    }
    let content = viewport + max_off;
    let track = viewport;
    let thumb_h = (track * (viewport / content)).clamp(24.0, track);
    // offset.y ∈ [-max,0] → 归一化占比 [0,1]
    let ratio = ((-offset_y) / max_off).clamp(0.0, 1.0);
    let thumb_top = (track - thumb_h) * ratio;
    (thumb_top, thumb_h, true)
}

/// 拖动换算：由 (按下时指针 y, 按下时 offset 占比) 与当前指针 y 计算新 offset.y。
/// 返回新 offset.y（负值或 0）。
pub fn drag_to_offset(
    viewport: f32,
    max_off: f32,
    start_py: f32,
    start_ratio: f32,
    pointer_y: f32,
) -> Option<f32> {
    if viewport <= 0.0 || max_off <= 0.0 {
        return None;
    }
    let thumb_h = (viewport * (viewport / (viewport + max_off))).clamp(24.0, viewport);
    let track = viewport;
    let dy = pointer_y - start_py;
    let ratio = (start_ratio + dy / (track - thumb_h).max(1.0)).clamp(0.0, 1.0);
    Some(-(ratio * max_off))
}

/// 当前 offset 占比（按下时记录用）[0,1]
pub fn offset_ratio(viewport: f32, max_off: f32, offset_y: f32) -> f32 {
    if viewport > 0.0 && max_off > 0.0 {
        ((-offset_y) / max_off).clamp(0.0, 1.0)
    } else {
        0.0
    }
}
