//! 异类策略注入机制探针（一次性工具）：验证 PowerWriteACValueIndex 对
//! 「方案下不存在的设置项键」是否自动创建（决定注入实现路线）。
//!
//! 运行：`cargo run -p secm-core --example hetero_inject_probe`
//! 全程操作"电源计划副本"（复制→写→检查→删除副本），不触碰真实计划。

use secm_datasource::power;
use winreg::enums::*;
use winreg::RegKey;

/// 处理器子组（与 settings.rs 一致）
const SUBGROUP: &str = "54533251-82be-4824-96c1-47b60b740d00";
/// 候选设置项 GUID（本机方案中大概率缺失的处理器子组设置）
const CANDIDATES: &[&str] = &[
    "be337238-0d82-4146-a960-4f3749d470c7", // Processor performance boost mode
    "36687f9e-e3a5-4dbf-b1dc-15eb381c6863", // Processor energy performance preference policy
    "4d2b0152-7d5c-498b-88e2-34345392a2c5", // Processor performance increase time
    "8baa4a8a-14c6-4451-8e8b-14bdbd197537", // Autonomous mode
];

fn key_exists(scheme: &str, setting: &str) -> bool {
    let path = format!(
        r"SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes\{}\{}\{}",
        scheme, SUBGROUP, setting
    );
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(&path, KEY_READ | KEY_WOW64_64KEY)
        .is_ok()
}

fn main() {
    println!("=== 异类策略注入机制探针 ===");
    let active = power::get_active_scheme()
        .ok()
        .flatten()
        .expect("无激活电源计划");
    let dup = power::duplicate_scheme(&active).expect("复制电源计划失败");
    println!("副本方案: {}", dup);

    // 找一个副本中不存在的候选设置项
    let mut target: Option<&str> = None;
    for c in CANDIDATES {
        let exists = key_exists(&dup, c);
        println!("候选 {} 存在性: {}", c, exists);
        if !exists && target.is_none() {
            target = Some(c);
        }
    }
    let Some(setting) = target else {
        println!("（所有候选设置项均已存在于副本，PowerWrite 创建语义无法用本机验证）");
        let _ = power::delete_scheme(&dup);
        return;
    };

    // PowerWrite 写入副本中不存在的设置项 → 观察键是否被自动创建
    println!("--- PowerWrite 写入缺失设置项: {} ---", setting);
    match power::write_ac_value(Some(&dup), SUBGROUP, setting, 2) {
        Ok(()) => println!("PowerWriteACValueIndex 返回成功"),
        Err(e) => println!("PowerWriteACValueIndex 失败: {}", e),
    }
    let created = key_exists(&dup, setting);
    println!(
        "写入后键存在性: {}（{}）",
        created,
        if created {
            "PowerWrite 自动创建 ✓"
        } else {
            "PowerWrite 不创建键 ✗"
        }
    );
    if created {
        match power::read_ac_value(Some(&dup), SUBGROUP, setting) {
            Ok(v) => println!("回读 AC 值 = {}（应为 2）", v),
            Err(e) => println!("回读失败: {}", e),
        }
    }

    // 清理副本
    match power::delete_scheme(&dup) {
        Ok(()) => println!("副本已删除（系统状态还原）"),
        Err(e) => println!("副本删除失败（请手动清理）: {} | {}", dup, e),
    }
    println!("=== 探针结束 ===");
}
