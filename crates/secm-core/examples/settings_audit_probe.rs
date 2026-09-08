//! 系统设置页真机读路径探针（ADR-0001 验证；一次性工具）
//!
//! 运行：`cargo run -p secm-core --example settings_audit_probe`
//! 只读操作，不修改任何系统状态。管理员/非管理员均可运行，
//! 输出各设置项的真实读值以验证数据链路与降级路径。

use secm_core::hpt;
use secm_core::nvidia_drs;
use secm_core::settings;

fn main() {
    println!("=== 系统设置 · 真机读路径探测 ===");
    println!("is_admin = {}", settings::is_admin());

    let toggles = [
        ("HAGS", settings::get_hags_state()),
        ("游戏模式", settings::get_game_mode_state()),
        ("窗口化游戏优化", settings::get_game_optimization_state()),
        ("VRR", settings::get_vrr_state()),
        ("鼠标精准度", settings::get_mouse_precision_state()),
    ];
    for (name, st) in toggles {
        println!(
            "[{:>12}] enabled={:>5} admin={:>5} | {}",
            name, st.enabled, st.admin_required, st.message
        );
    }

    let hpt = hpt::get_hpt_state();
    println!(
        "[{:>12}] enabled={:>5} admin={:>5} | {}",
        "高精度计时器", hpt.enabled, hpt.admin_required, hpt.message
    );

    println!("--- 电源计划 ---");
    match settings::get_power_plans() {
        Ok(plans) => {
            for p in &plans {
                println!(
                    "{} name=「{}」 guid={} active={}",
                    if p.is_active { "●" } else { " " },
                    p.name,
                    p.guid,
                    p.is_active
                );
            }
        }
        Err(e) => println!("读取失败: {}", e),
    }

    println!("--- 异类调度策略 ---");
    match settings::get_hetero_policies() {
        Ok(h) => println!(
            "supported={} thread(present={} ac={:?} dc={:?}) short(present={} ac={:?} dc={:?})",
            h.supported,
            h.thread_present,
            h.thread_ac,
            h.thread_dc,
            h.short_present,
            h.short_ac,
            h.short_dc
        ),
        Err(e) => println!("读取失败: {}", e),
    }

    println!("--- NVIDIA 电源模式（NVAPI DRS）---");
    match nvidia_drs::get_power_mode() {
        Ok(m) => println!("当前模式 = {}（{:?}）", m.label_cn(), m),
        Err(e) => println!("优雅降级（无 NVIDIA/NVAPI 不可用）: {}", e),
    }

    println!("--- Windows 服务 ---");
    match settings::list_all_services() {
        Ok(list) => {
            let running = list.iter().filter(|s| s.status == "Running").count();
            println!("共 {} 个服务（运行中 {}）", list.len(), running);
            for s in list.iter().take(3) {
                println!(
                    "示例: {} | {} | {} | {}",
                    s.name, s.display_name, s.status, s.start_type
                );
            }
        }
        Err(e) => println!("枚举失败: {}", e),
    }
    println!("=== 探测完成 ===");
}
