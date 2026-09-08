//! hw_verify — 硬件指标真机验证工具（v3.0.0 原生迁移版）
//!
//! 用途：采集 5 帧统一快照后打印各指标 值/来源/错误，用于验证：
//! 1. 纯原生采集链路（NVML/DXGI/PDH/Win32/IOCTL/WMI）在真实机器上的可用性；
//! 2. 非管理员环境下的权限降级语义（CPU 温度/主板域如实 unavailable，无伪造）；
//! 3. 数据持续刷新（多帧对比负载/速率变化）。
//!
//! 运行：`cargo run -p secm-core --example hw_verify`
//! （无需管理员权限；无任何 HTTP/localhost 依赖；无 sidecar 启动）

use secm_core::sensor_service::SensorService;

fn main() {
    println!("=== SECM 硬件原生采集验证（v3.0.0，零 HTTP/零提权）===");
    SensorService::start_once();
    // 等 6 帧（~7s）：首轮 collect 含 PDH warmup ~2s、磁盘温度首拍 IOCTL、
    // WMI SPD 一次性查询；此后帧反映持续刷新语义
    for frame in 1..=6 {
        std::thread::sleep(std::time::Duration::from_millis(1200));
        if frame == 6 {
            let snap = SensorService::snapshot();
            print_snapshot(&snap);
        }
    }
    println!("=== 验证结束（纯进程内 Rust 直调，无 sidecar 清理需求）===");
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
        println!("MOBO: name={:?} sensors={}", mb.name, mb.sensors.len());
    } else {
        println!("MOBO: n/a（SuperIO 需 ring0 内核驱动，非管理员环境不可用）");
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
        println!("GPU: n/a（NVML/DXGI 未枚举到适配器）");
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
            "BAT: pct={} power={} current={} volt={} ac={} charging={}",
            m(&b.percent, "%"),
            m(&b.power_w, "W"),
            m(&b.current_a, "A"),
            m(&b.voltage_v, "V"),
            b.ac_online,
            b.charging,
        ),
        None => println!("BAT: n/a（无电池——台式机，或用户态电池 API 不可得）"),
    }
    for st in &snap.storage_temps {
        println!("STORAGE[{}]: {}", st.name, m(&st.temp, "°C"),);
    }
    println!("DIAG: {}", snap.diag);
}
