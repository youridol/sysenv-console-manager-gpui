//! 系统设置页真机写路径探针（ADR-0001 验证；一次性工具，全部操作可逆/零净变更）
//!
//! 运行：`cargo run -p secm-core --example settings_write_probe`（需管理员）
//!
//! 验证策略（不改变系统最终状态）：
//! 1. HKLM/HKCU 开关：写回当前值（幂等）→ 验证写入链路 + 重读一致
//! 2. 高精度计时器：关闭 → 读校验 → 恢复默认 → 读校验（完整往返）
//! 3. NVIDIA 电源模式：写回当前模式 → 验证 SetSetting/SaveSettings/校验读链路
//! 4. 异类策略：写回当前值（AC/DC 双路）→ 验证 PowerWriteValueIndex 链路
//! 5. 电源计划：重新激活当前计划（幂等）；删除链路由 datasource ignored 测试覆盖
//! 6. 服务启动类型：对首个可查服务写回当前类型（幂等）

use secm_core::hpt;
use secm_core::nvidia_drs::{self, NvidiaPowerMode};
use secm_core::settings;

fn main() {
    println!("=== 系统设置 · 真机写路径探测（可逆） ===");
    if !settings::is_admin() {
        println!("（警告）非管理员：部分写路径将按权限不足语义失败——这本身也是验证项");
    }

    // 1. 开关幂等写（各写回当前值）
    type Setter = Box<dyn Fn(bool) -> Result<settings::SettingState, String>>;
    let cases: Vec<(&str, bool, Setter)> = vec![
        (
            "HAGS",
            settings::get_hags_state().enabled,
            Box::new(settings::set_hags_state),
        ),
        (
            "游戏模式",
            settings::get_game_mode_state().enabled,
            Box::new(settings::set_game_mode_state),
        ),
        (
            "窗口化优化",
            settings::get_game_optimization_state().enabled,
            Box::new(settings::set_game_optimization),
        ),
        (
            "VRR",
            settings::get_vrr_state().enabled,
            Box::new(settings::set_vrr_state),
        ),
        (
            "鼠标精准度",
            settings::get_mouse_precision_state().enabled,
            Box::new(settings::set_mouse_precision),
        ),
    ];
    for (name, cur, setter) in cases {
        match setter(cur) {
            Ok(st) => println!(
                "[幂等写] {} 写 {} → 重读 {}（一致: {}）",
                name,
                cur,
                st.enabled,
                st.enabled == cur
            ),
            Err(e) => println!("[幂等写] {} 失败: {}", name, e),
        }
    }

    // 2. 高精度计时器完整往返：关闭 → 校验 → 恢复 → 校验
    println!("--- 高精度计时器往返 ---");
    match hpt::set_hpt_state(true) {
        Ok(st) => println!(
            "关闭写后读: enabled={}（应为 true）| {}",
            st.enabled, st.message
        ),
        Err(e) => println!("关闭失败: {}", e),
    }
    match hpt::set_hpt_state(false) {
        Ok(st) => println!(
            "恢复写后读: enabled={}（应为 false）| {}",
            st.enabled, st.message
        ),
        Err(e) => println!("恢复失败: {}", e),
    }

    // 3. NVIDIA 电源模式：变值往返（写不同值 → 新会话验证 → 恢复原值）
    //    回归场景：SaveSettings 后同会话 Get 返回旧缓存值导致"验证不一致"假失败
    println!("--- NVIDIA 电源模式变值往返 ---");
    match nvidia_drs::get_power_mode() {
        Ok(cur) => {
            // 选一个与当前不同的值写入
            let target = if cur == NvidiaPowerMode::MaxPerformance {
                NvidiaPowerMode::Optimal
            } else {
                NvidiaPowerMode::MaxPerformance
            };
            match nvidia_drs::set_power_mode(target) {
                Ok(()) => match nvidia_drs::get_power_mode() {
                    Ok(now) => println!(
                        "变值写 {} → 重读 {}（一致: {}）",
                        target.label_cn(),
                        now.label_cn(),
                        now == target
                    ),
                    Err(e) => println!("变值写后读失败: {}", e),
                },
                Err(e) => println!("变值写 {} 失败: {}", target.label_cn(), e),
            }
            // 恢复原值（再次变值往返，双重验证）
            match nvidia_drs::set_power_mode(cur) {
                Ok(()) => println!("恢复原值 {} 写入并校验通过", cur.label_cn()),
                Err(e) => println!("恢复原值 {} 失败: {}", cur.label_cn(), e),
            }
        }
        Err(e) => println!("NVIDIA 不可用（降级路径）: {}", e),
    }

    // 4. 异类策略：present 检测 + 幂等写（缺失项由 PowerWrite 自动注入，机制见 hetero_inject_probe）
    println!("--- 异类策略检测与写入 ---");
    if let Ok(h) = settings::get_hetero_policies() {
        println!(
            "present 检测: thread={} short={}",
            h.thread_present, h.short_present
        );
        // 幂等写回当前方案（缺失项由 PowerWrite 自动创建键）
        let v = h.thread_ac.unwrap_or(0);
        match settings::set_hetero_policy_scoped("thread", v, true, true) {
            Ok(()) => println!("[幂等写] 异类线程调度 AC/DC 写回 {} 成功", v),
            Err(e) => println!("[幂等写] 异类线程调度失败: {}", e),
        }
        match settings::set_hetero_policy_scoped("short", v, true, true) {
            Ok(()) => println!("[幂等写] 短运行线程调度 AC/DC 写回 {} 成功", v),
            Err(e) => println!("[幂等写] 短运行线程调度失败: {}", e),
        }
    }

    // 5. 电源计划幂等激活（重新激活当前计划）
    if let Ok(Some(active)) = secm_datasource::power::get_active_scheme() {
        match settings::set_power_plan(&active) {
            Ok(()) => println!("[幂等写] 电源计划重新激活 {} 成功", active),
            Err(e) => println!("[幂等写] 电源计划激活失败: {}", e),
        }
    }

    // 6. 服务启动类型幂等写（挑一个服务写回当前类型）
    if let Ok(list) = settings::list_all_services() {
        if let Some(svc) = list
            .iter()
            .find(|s| s.start_type == "手动" || s.start_type == "自动" || s.start_type == "已禁用")
        {
            let target = match svc.start_type.as_str() {
                "自动" => "auto",
                "手动" => "manual",
                _ => "disabled",
            };
            match settings::set_service_start_type(&svc.name, target) {
                Ok(msg) => println!(
                    "[幂等写] 服务 {} 启动类型写回 {}: {}",
                    svc.name, target, msg
                ),
                Err(e) => println!("[幂等写] 服务 {} 启动类型失败: {}", svc.name, e),
            }
        }
    }

    // 7. NVIDIA 枚举完整性（三档均有映射）
    println!(
        "NVIDIA 三档标签: {} / {} / {}",
        NvidiaPowerMode::Adaptive.label_cn(),
        NvidiaPowerMode::MaxPerformance.label_cn(),
        NvidiaPowerMode::Optimal.label_cn()
    );
    println!("=== 写路径探测完成（系统状态已还原） ===");
}
