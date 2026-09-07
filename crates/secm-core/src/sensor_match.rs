// secm-core::sensor_match — LiteMonitor 传感器匹配策略等价实现（ADR-0004/0006）
//
// 对应 LiteMonitor：
// - SensorMap.Rebuild 的 MOBO.Temp 智能选择策略（System > Motherboard > Chipset/PCH
//   > 合理范围(15–68℃)最大值 > 宽范围(0–95℃)最大值）；
// - ReadMoboTemperature 硬上限（Auto 95℃ / Manual 125℃）+ lastValid 语义；
// - FanMapper.ScanAndMapFans 风扇/水泵智能匹配（底噪 >200 RPM、Cooler 硬件优先、
//   Pump 高转速猜想、CaseFan 命名优先级 Rear>Chassis>Sys>Case）；
// - BatteryService 的 AC 符号修正（充电正 / 放电强制负）。
//
// 输入为 sidecar 输出的主板原始传感器列表（name/kind/hw/value）；
// 全部为纯函数，附单元测试。

use crate::sensor::MotherboardSensor;

/// 风扇底噪过滤阈值（RPM；LiteMonitor FanMapper 同值）
const FAN_NOISE_FLOOR_RPM: f32 = 200.0;
/// 水泵高转速猜想阈值（RPM；LiteMonitor 同值）
const PUMP_RPM_GUESS: f32 = 3000.0;
/// 主板温度硬上限：自动选择模式（LiteMonitor AutoMoboTempHardMax）
const MOBO_TEMP_AUTO_MAX: f32 = 95.0;

// ============================================================================
// MOBO.Temp 智能选择（LiteMonitor SensorMap.Rebuild A 段等价）
// ============================================================================

/// 从主板原始传感器列表中选择"系统/主板温度"。
///
/// 策略（LiteMonitor 等价）：
/// 1. 名称含 "System"；2. 含 "Motherboard"；3. 含 "Chipset" 或 "PCH"；
/// 4. 合理范围 (15–68℃) 内最大值；5. 宽范围 (0–95℃) 内最大值；无匹配 → None。
pub fn match_system_temp(sensors: &[MotherboardSensor]) -> Option<f32> {
    let temps: Vec<&MotherboardSensor> = sensors
        .iter()
        .filter(|s| s.kind == "temperature" && s.value > 0.0)
        .collect();
    if temps.is_empty() {
        return None;
    }
    let has = |s: &MotherboardSensor, kw: &str| s.name.contains(kw);
    if let Some(t) = temps.iter().find(|s| has(s, "System")) {
        return Some(t.value);
    }
    if let Some(t) = temps.iter().find(|s| has(s, "Motherboard")) {
        return Some(t.value);
    }
    if let Some(t) = temps.iter().find(|s| has(s, "Chipset") || has(s, "PCH")) {
        return Some(t.value);
    }
    // 兜底：合理范围 (15-68) 最大值优先，其次宽范围 (0-95) 最大值
    // （LiteMonitor：修复 Z790 "Temperature #5" 跳 100+ 误报）
    let mut best_safe: Option<f32> = None;
    let mut best_fallback: Option<f32> = None;
    for t in &temps {
        let v = t.value;
        if v > 0.0 && v < 95.0 && best_fallback.is_none_or(|m| v > m) {
            best_fallback = Some(v);
        }
        if (15.0..=68.0).contains(&v) && best_safe.is_none_or(|m| v > m) {
            best_safe = Some(v);
        }
    }
    best_safe.or(best_fallback)
}

/// 主板温度读取校验（LiteMonitor ReadMoboTemperature 等价）。
///
/// 无效值（<=0 / NaN / Inf / 超硬上限）→ None（调用方保留 lastValid 或显示不可用）。
pub fn validate_mobo_temp(value: f32) -> Option<f32> {
    if !value.is_finite() || value <= 0.0 || value >= MOBO_TEMP_AUTO_MAX {
        return None;
    }
    Some(value)
}

// ============================================================================
// 风扇/水泵智能匹配（LiteMonitor FanMapper 等价）
// ============================================================================

/// 风扇匹配产出（CPU 风扇 / 水泵 / 机箱风扇）
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FanMatch {
    pub cpu_fan: Option<f32>,
    pub cpu_pump: Option<f32>,
    pub case_fan: Option<f32>,
}

/// 判断硬件名是否为"散热设备"（LiteMonitor IsCoolerHardware 等价）：
/// Cooler 类型或名称含 Kraken/Corsair/Liquid/AIO/Cooler。
/// （SECM 侧car 无独立 Cooler 类型字段，按名称启发；hw 名来自 LHM 硬件节点。）
fn is_cooler_hw(hw: &str) -> bool {
    const KW: [&str; 5] = ["Kraken", "Corsair", "Liquid", "AIO", "Cooler"];
    KW.iter().any(|k| hw.contains(k))
}

/// 从主板原始传感器列表匹配风扇/水泵/机箱风扇（LiteMonitor FanMapper 等价）。
///
/// 步骤：
/// 1. 收集全部 fan 类型且 >200 RPM（底噪过滤）；
/// 2. CPU 风扇：Cooler 硬件（非 Pump 名）> 名含 "CPU" > 第一个；
/// 3. 水泵：Cooler 硬件名含 Pump/Speed > 其他 Cooler > 名含 Pump/Water/AIO
///    > 剩余中转速 >3000 最高者；
/// 4. 机箱风扇：剩余中名含 Rear > Chassis > Sys > Case > 转速最低者
///    （多扇时最高者给 Pump——仅当 Pump 仍空）。
pub fn match_fans(sensors: &[MotherboardSensor]) -> FanMatch {
    // 1. 收集活跃风扇（kind=fan，>底噪）
    struct Fan<'a> {
        s: &'a MotherboardSensor,
        rpm: f32,
    }
    let fans: Vec<Fan> = sensors
        .iter()
        .filter(|s| s.kind == "fan" && s.value > FAN_NOISE_FLOOR_RPM)
        .map(|s| Fan { s, rpm: s.value })
        .collect();
    if fans.is_empty() {
        return FanMatch::default();
    }

    let mut used: Vec<usize> = Vec::new();
    // free：该风扇未被更高优先级分支占用
    let free = |used: &Vec<usize>, f: &Fan| -> bool {
        fans.iter()
            .position(|x| std::ptr::eq(x.s, f.s))
            .map(|i| !used.contains(&i))
            .unwrap_or(false)
    };
    // take：占用并在输出标注
    let mut out = FanMatch::default();
    let take = |fans: &Vec<Fan>, used: &mut Vec<usize>, f: &Fan, slot: &mut Option<f32>| {
        *slot = Some(f.rpm);
        if let Some(i) = fans.iter().position(|x| std::ptr::eq(x.s, f.s)) {
            used.push(i);
        }
    };

    // 2. CPU 风扇
    //    Cooler 硬件（非 Pump 名）> 名含 CPU > 第一个未被占用者
    if let Some(f) = fans
        .iter()
        .find(|f| free(&used, f) && is_cooler_hw(&f.s.hw) && !f.s.name.contains("Pump"))
    {
        take(&fans, &mut used, f, &mut out.cpu_fan);
    } else if let Some(f) = fans
        .iter()
        .find(|f| free(&used, f) && f.s.name.contains("CPU"))
    {
        take(&fans, &mut used, f, &mut out.cpu_fan);
    } else if let Some(f) = fans.iter().find(|f| free(&used, f)) {
        take(&fans, &mut used, f, &mut out.cpu_fan);
    }

    // 3. 水泵
    //    Cooler 硬件名含 Pump/Speed > 其他 Cooler > 名含 Pump/Water/AIO
    //   （高转速猜想见下方——先于机箱风扇分配，对齐 LiteMonitor 步骤顺序）
    if out.cpu_pump.is_none() {
        if let Some(f) = fans.iter().find(|f| {
            free(&used, f)
                && is_cooler_hw(&f.s.hw)
                && (f.s.name.contains("Pump") || f.s.name.contains("Speed"))
        }) {
            take(&fans, &mut used, f, &mut out.cpu_pump);
        }
    }
    if out.cpu_pump.is_none() {
        if let Some(f) = fans
            .iter()
            .find(|f| free(&used, f) && is_cooler_hw(&f.s.hw))
        {
            take(&fans, &mut used, f, &mut out.cpu_pump);
        }
    }
    if out.cpu_pump.is_none() {
        if let Some(f) = fans.iter().find(|f| {
            free(&used, f)
                && (f.s.name.contains("Pump")
                    || f.s.name.contains("Water")
                    || f.s.name.contains("AIO"))
        }) {
            take(&fans, &mut used, f, &mut out.cpu_pump);
        }
    }

    // 高转速水泵猜想（LiteMonitor 步骤 4 内：>3000 RPM 最高者，先于机箱风扇分配）
    if out.cpu_pump.is_none() {
        let candidate = fans
            .iter()
            .filter(|f| free(&used, f) && f.rpm > PUMP_RPM_GUESS)
            .max_by(|a, b| {
                a.rpm
                    .partial_cmp(&b.rpm)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        if let Some(f) = candidate {
            take(&fans, &mut used, f, &mut out.cpu_pump);
        }
    }

    // 4. 机箱风扇
    //    剩余中名含 Rear > Chassis > Sys > Case > 转速最低者；
    //    多扇且 Pump 为空时最高转速者补 Pump（LiteMonitor 同语义）
    let leftovers: Vec<&Fan> = fans.iter().filter(|f| free(&used, f)).collect();
    if !leftovers.is_empty() {
        let by_kw = |kw: &str| leftovers.iter().find(|f| f.s.name.contains(kw)).copied();
        match by_kw("Rear")
            .or_else(|| by_kw("Chassis"))
            .or_else(|| by_kw("Sys"))
            .or_else(|| by_kw("Case"))
        {
            Some(f) => {
                take(&fans, &mut used, f, &mut out.case_fan);
            }
            None => {
                let mut sorted: Vec<&&Fan> = leftovers.iter().collect();
                sorted.sort_by(|a, b| {
                    a.rpm
                        .partial_cmp(&b.rpm)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                take(&fans, &mut used, sorted[0], &mut out.case_fan);
                if out.cpu_pump.is_none() && sorted.len() > 1 {
                    out.cpu_pump = Some(sorted[sorted.len() - 1].rpm);
                    if let Some(i) = fans
                        .iter()
                        .position(|x| std::ptr::eq(x.s, sorted[sorted.len() - 1].s))
                    {
                        used.push(i);
                    }
                }
            }
        }
    }

    // 高转速水泵猜想（LiteMonitor：>3000 RPM 最高者）
    if out.cpu_pump.is_none() {
        let candidate = fans
            .iter()
            .filter(|f| free(&used, f) && f.rpm > PUMP_RPM_GUESS)
            .max_by(|a, b| {
                a.rpm
                    .partial_cmp(&b.rpm)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        if let Some(f) = candidate {
            out.cpu_pump = Some(f.rpm);
        }
    }

    out
}

// ============================================================================
// 电池符号修正（LiteMonitor BatteryService 等价）
// ============================================================================

/// 电池功率/电流符号修正：插电（AC Online）= 充电输入 → 正数；
/// 电池供电 = 放电输出 → 强制负绝对值（LiteMonitor 强制符号语义）。
pub fn fix_battery_sign(value: f32, ac_online: bool) -> f32 {
    if ac_online {
        value.abs()
    } else {
        -value.abs()
    }
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, v: f32) -> MotherboardSensor {
        MotherboardSensor {
            name: name.into(),
            kind: "temperature".into(),
            hw: "Board".into(),
            value: v,
        }
    }
    fn fan(name: &str, hw: &str, v: f32) -> MotherboardSensor {
        MotherboardSensor {
            name: name.into(),
            kind: "fan".into(),
            hw: hw.into(),
            value: v,
        }
    }

    #[test]
    fn test_system_temp_priority() {
        // System 优先
        let s = vec![
            temp("CPU", 40.0),
            temp("System", 35.0),
            temp("Chipset", 50.0),
        ];
        assert_eq!(match_system_temp(&s), Some(35.0));
        // Motherboard 次之
        let s = vec![temp("Chipset", 50.0), temp("Motherboard", 33.0)];
        assert_eq!(match_system_temp(&s), Some(33.0));
        // Chipset/PCH 再次
        let s = vec![temp("Chipset", 50.0), temp("VRM", 60.0)];
        assert_eq!(match_system_temp(&s), Some(50.0));
    }

    #[test]
    fn test_system_temp_range_fallback() {
        // 无标准名：合理范围 (15-68) 最大优先（68 上限），>95 排除
        let s = vec![
            temp("Temperature #1", 20.0),
            temp("Temperature #3", 66.0),
            temp("Temperature #5", 110.0),
        ];
        assert_eq!(match_system_temp(&s), Some(66.0));
        // 合理范围缺失 → 宽范围最大
        let s = vec![temp("Temperature #1", 5.0), temp("Temperature #3", 80.0)];
        assert_eq!(match_system_temp(&s), Some(80.0));
        // 全部无效
        let s = vec![temp("Temperature #5", 120.0), temp("T2", 0.0)];
        assert_eq!(match_system_temp(&s), None);
    }

    #[test]
    fn test_validate_mobo_temp() {
        assert_eq!(validate_mobo_temp(45.0), Some(45.0));
        assert_eq!(validate_mobo_temp(0.0), None);
        assert_eq!(validate_mobo_temp(-3.0), None);
        assert_eq!(validate_mobo_temp(f32::NAN), None);
        // ≥95 硬上限拒绝（自动模式）
        assert_eq!(validate_mobo_temp(95.0), None);
    }

    #[test]
    fn test_match_fans_priority() {
        // CPU 风扇名含 CPU；机箱风扇 Rear；Cooler 硬件优先
        let s = vec![
            fan("Fan #1", "Motherboard", 800.0),
            fan("CPU Fan", "Board", 1200.0),
            fan("Rear Fan", "Motherboard", 900.0),
        ];
        let m = match_fans(&s);
        assert_eq!(m.cpu_fan, Some(1200.0));
        assert_eq!(m.case_fan, Some(900.0));
        // Pump 猜想：无 Pump 名且无 >3000 → 无 Pump
        assert_eq!(m.cpu_pump, None);
    }

    #[test]
    fn test_match_fans_pump_guess() {
        // 高转速 >3000 猜想为 Pump
        let s = vec![
            fan("CPU Fan", "Board", 1200.0),
            fan("Fan #2", "Motherboard", 3500.0),
        ];
        let m = match_fans(&s);
        assert_eq!(m.cpu_fan, Some(1200.0));
        assert_eq!(m.cpu_pump, Some(3500.0));
    }

    #[test]
    fn test_match_fans_cooler_hw_first() {
        // Cooler 硬件名优先（Kraken 等）
        let s = vec![
            fan("Fan #1", "Motherboard", 800.0),
            fan("Pump", "Kraken X63", 2800.0),
        ];
        let m = match_fans(&s);
        // Cooler 硬件非 Pump 名的候选被 Pump 名占用 → CPU 风扇落第一个
        assert_eq!(m.cpu_fan, Some(800.0));
        assert_eq!(m.cpu_pump, Some(2800.0));
    }

    #[test]
    fn test_match_fans_noise_floor() {
        // 底噪 ≤200 RPM 全部过滤
        let s = vec![
            fan("CPU Fan", "Board", 0.0),
            fan("Fan #2", "Motherboard", 150.0),
        ];
        assert_eq!(match_fans(&s), FanMatch::default());
    }

    #[test]
    fn test_battery_sign_fix() {
        // 插电：正
        assert_eq!(fix_battery_sign(65.0, true), 65.0);
        assert_eq!(fix_battery_sign(-65.0, true), 65.0);
        // 电池供电：强制负
        assert_eq!(fix_battery_sign(25.0, false), -25.0);
        assert_eq!(fix_battery_sign(-25.0, false), -25.0);
    }
}
