//! NVIDIA DRS 写入链路诊断（一次性工具；输出各阶段原始 rc 与结构字段）
//!
//! 运行：`cargo run -p secm-core --example nvidia_diag`
//! 写入目标值 1（最高性能优先）后【恢复】为写入前的当前值，最终状态还原。

use secm_core::nvidia_drs;

fn main() {
    println!("=== NVIDIA DRS 诊断 ===");
    let cur = match nvidia_drs::get_power_mode() {
        Ok(m) => {
            println!("当前模式: {}（{:?}）", m.label_cn(), m);
            m as u32
        }
        Err(e) => {
            println!("读取失败: {}", e);
            return;
        }
    };
    let target = if cur == 1 { 5 } else { 1 };
    println!("--- diagnostic_roundtrip(写 {}) ---", target);
    println!("{}", nvidia_drs::diagnostic_roundtrip(target));
    // 恢复：把当前值写回（set_power_mode 内含新会话验证）
    println!("--- 恢复原值 {} ---", cur);
    match nvidia_drs::set_power_mode(
        nvidia_drs::NvidiaPowerMode::from_raw(cur).unwrap_or(nvidia_drs::NvidiaPowerMode::Adaptive),
    ) {
        Ok(()) => println!("恢复成功"),
        Err(e) => println!("恢复失败: {}", e),
    }
    println!("=== 诊断结束 ===");
}
