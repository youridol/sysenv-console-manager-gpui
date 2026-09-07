//! hw_verify — 硬件指标真机验证工具（ADR-0010 指标门禁）
//!
//! 用途：采集 3 帧统一快照后打印各指标 值/来源/错误，用于验证：
//! 1. 管理员 / 普通用户（降权 token）下的指标可用性差异（ADR-0007 权限矩阵）；
//! 2. LHM sidecar 不可用时的降级语义（无伪造 0/默认值）。
//!
//! 环境变量：
//! - `SECM_DISABLE_LHM=1`  跳过 sidecar 启动（模拟"无 sidecar/驱动不可访问"场景）
//! - `SECM_HW_VERIFY_NO_LHM_PROMPT=1` 语义同上（别名，脚本友好）
//!
//! 运行：`cargo run -p secm-core --example hw_verify`（重复 3 帧，间隔 1.2s）

use secm_core::sensor_service::SensorService;

fn main() {
    // 与 sensor_service::ensure_lhm_periodic 的开关保持一致（别名归一）
    if std::env::var("SECM_HW_VERIFY_NO_LHM_PROMPT").as_deref() == Ok("1") {
        std::env::set_var("SECM_DISABLE_LHM", "1");
    }

    SensorService::start_once();
    // 等 15 帧（~18s）：首轮 collect 含 PDH warmup ~2s，sidecar 冷启动 LHM Open
    // 枚举全部硬件需 >6s，失败退避 5s 窗口过后续帧即可见 LHM 域真实值
    for frame in 1..=15 {
        std::thread::sleep(std::time::Duration::from_millis(1200));
        if frame == 15 {
            let snap = SensorService::snapshot();
            print_snapshot(&snap);
        }
    }
    // 退出清理（对齐主程序 on_app_quit → lhm::shutdown：受控退出 + 孤儿清理，
    // 防止 sidecar 继承的 stdout 句柄阻塞父进程管道收尾）
    secm_core::lhm::shutdown();
}

fn print_snapshot(snap: &secm_core::sensor::SensorSnapshot) {
    let m = |mt: &secm_core::sensor::Metric<f32>, unit: &str| match &mt.value {
        Some(v) => format!("{:.1}{} [{}]", v, unit, mt.source.as_str()),
        None => format!("n/a({}) [unavailable]", mt.error.as_deref().unwrap_or("?")),
    };

    println!("=== HardwareSnapshot 验证帧 ===");
    println!(
        "CPU: load={:.1}%[{}] temp={} clock={} power={} volt={}",
        snap.cpu.usage,
        snap.cpu.usage_source.as_str(),
        m(&snap.cpu.temperature, "°C"),
        m(&snap.cpu.clock_mhz, "MHz"),
        m(&snap.cpu.power_w, "W"),
        m(&snap.cpu.voltage, "V"),
    );
    if let Some(mb) = &snap.motherboard {
        println!(
            "MOBO: name={:?} sysTemp={} cpuFan={} pump={} caseFan={} rawSensors={}",
            mb.name,
            m(&mb.system_temp, "°C"),
            m(&mb.cpu_fan_rpm, "RPM"),
            m(&mb.cpu_pump_rpm, "RPM"),
            m(&mb.case_fan_rpm, "RPM"),
            mb.sensors.len(),
        );
    } else {
        println!("MOBO: n/a(LHM 不可用)");
    }
    for g in &snap.gpu {
        println!(
            "GPU[{}]: load={} temp={} clock={} power={} vram={}/{} fan={}",
            g.name,
            m(&g.usage, "%"),
            m(&g.temperature, "°C"),
            m(&g.clock_mhz, "MHz"),
            m(&g.power_w, "W"),
            match g.vram_used.value {
                Some(b) => format!("{:.1}GB", b as f64 / 1024.0 / 1024.0 / 1024.0),
                None => "n/a".into(),
            },
            match g.vram_total.value {
                Some(b) => format!("{:.1}GB", b as f64 / 1024.0 / 1024.0 / 1024.0),
                None => "n/a".into(),
            },
            m(&g.fan_rpm, "RPM"),
        );
    }
    if snap.gpu.is_empty() {
        println!("GPU: n/a(LHM 不可用或无显卡)");
    }
    let mem = &snap.memory;
    println!(
        "MEM: total={:.1}GB used={:.1}GB load={:.0}% spd={:?}",
        mem.total as f64 / 1024.0 / 1024.0 / 1024.0,
        mem.used as f64 / 1024.0 / 1024.0 / 1024.0,
        mem.usage_percent,
        mem.model_name,
    );
    for d in snap.disks.iter().take(4) {
        println!(
            "DISK[{} {}]: cap={:.0}/{:.0}GB read={} write={} act={} temp={}",
            d.drive_key,
            d.name,
            d.used_space as f64 / 1024.0 / 1024.0 / 1024.0,
            d.total_space as f64 / 1024.0 / 1024.0 / 1024.0,
            m(&d.read_mbps, "MB/s"),
            m(&d.write_mbps, "MB/s"),
            m(&d.activity_pct, "%"),
            m(&d.temperature, "°C"),
        );
    }
    println!(
        "NET: ipv4={} tcp={} ifs={}",
        snap.net.local_ipv4,
        snap.net.tcp_established,
        snap.net.interfaces.len(),
    );
    for i in snap.net.interfaces.iter().take(4) {
        println!(
            "  NIC[{}] rx={} tx={} speed={}",
            i.name,
            m(&i.rx_kbps, "KB/s"),
            m(&i.tx_kbps, "KB/s"),
            i.link_speed,
        );
    }
    match &snap.battery {
        Some(b) => println!(
            "BAT: pct={} power={} current={} volt={} ac={}",
            m(&b.percent, "%"),
            m(&b.power_w, "W"),
            m(&b.current_a, "A"),
            m(&b.voltage_v, "V"),
            b.ac_online,
        ),
        None => println!("BAT: n/a(无电池或 LHM 不可用)"),
    }
    for st in &snap.storage_temps {
        println!("STORAGE[{}]: {}", st.name, m(&st.temp, "°C"),);
    }
    println!("DIAG: {}", snap.diag);
}
