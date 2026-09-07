# 统一 HardwareSnapshot 架构设计（ADR-0005）

## 1. 决策

在 `secm-core::sensor` 建立统一快照模型 v2（`Metric<T>`），UI 只消费 `SensorService::snapshot()`，不接触 LHM/PDH/Win32/sysinfo。对应数据流：

```text
Collector 层（secm-datasource 叶子 + sidecar-lhm）
    ↓
secm-core::sensor_service（1s 编排线程，唯一写入者）
    → Normalized HardwareSnapshot（Metric<T>：value/source/updated_at/error）
    ↓
secm-core（sensor_history 趋势 / net_info 本地链路 / hardware SMART 按需域）
    ↓
GPUI（dashboard / shell 侧栏 / hardware 页，只读快照）
```

## 2. Source 类型（对齐 ADR-0005 枚举）

```rust
pub enum Source {           // 序列化为字符串契约
    Lhm,                    // LiteMonitorLhm       — sidecar LibreHardwareMonitorLib
    PerfCounter,            // LiteMonitorPerfCounter — PDH（Utility/PhysicalDisk/Processor Performance）
    NativeNetwork,          // LiteMonitorNativeNetwork — GetIfTable2/GetAdaptersAddresses
    DriveInfo,              // LiteMonitorDriveInfo — sysinfo(GetDiskFreeSpaceExW) 等价
    Battery,                // LiteMonitorBattery   — sidecar Battery + GetSystemPowerStatus
    Smart,                  // LiteMonitorSmart     — SMART 独立按需域
    Unavailable,
}
```

## 3. Metric<T> 模型

```rust
pub struct Metric<T> {
    pub value: Option<T>,        // None = 不可用（禁止 0/默认值冒充）
    pub source: Source,
    pub updated_at_ms: u64,      // 本值采集时间（unix ms）
    pub error: Option<String>,   // 最近一次失败诊断（core 保留，UI 可隐藏）
}
```

- **字段级 Metric 用于可能不可用的测量值**：温度/功耗/电压/风扇/时钟/磁盘活动/磁盘温度/GPU 全域/电池全域/网络速率。
- **裸类型保留给结构稳定的基础域**：CPU usage、per_core、内存 total/used/available/pct、磁盘容量、网卡名/IP（GlobalMemoryStatusEx/GetIfTable2 类 API 恒可用；失败由快照 diag 汇总）。
- fallback_used 语义：`Metric.source` 即实际来源；回退成功时 source 变为回退层（如 CPU.Load 回退 sysinfo 时 source=PerfCounter→降级标注 `error=None, source=NtQuery`——具体以每指标 fallback 链为准，见 metric-mapping.md）。

## 4. 域结构 v2（secm-core::sensor）

| 结构 | 变更 |
| --- | --- |
| CpuData | usage/per_core/core_count/name 保留；`clock_mhz→Metric<f32>`、`temperature→Metric<f32>`、`power_w→Metric<f32>`、新增 `voltage→Metric<f32>`、`fan_rpm→Metric<f32>`（CPU 风扇，FanMapper 语义）、`fan_source` 概念并入 Metric.source |
| GpuData | 全字段 v2：name 保留；load/temp/clock/power/fan → `Metric<f32>`；vram_used/vram_total → `Metric<u64>`；vram_load_pct 由 used/total 计算（Option） |
| MemoryData | total/used/available/usage_percent 保留裸类型；model_name 保留 |
| DiskData | name/capacity 裸类型保留；read/write → `Metric<f32>`（MB/s）；新增 `activity→Metric<f32>`（% Disk Time）、`temp→Metric<f32>`（LHM Storage） |
| MotherboardData | name 保留；sensors 原始列表保留；新增智能匹配产出：`system_temp→Metric<f32>`（LiteMonitor MOBO.Temp 策略）、`cpu_fan/case_fan/cpu_pump→Metric<f32>`（FanMapper 等价） |
| NetSnapshot（新） | `interfaces: Vec<NetIfStat>`（name/description/rx_kbps:Metric/tx_kbps:Metric/link_speed）、`local_ipv4`、`tcp_established`；替代 dashboard 直调 netif |
| BatteryData（新） | percent/power_w/current_a/voltage_v → `Metric<f32>` + `ac_online: bool`（符号修正输入） |

## 5. 采集职责划分（唯一权威来源落地）

| 指标域 | Collector | 归属 |
| --- | --- | --- |
| CPU Load | 新 `secm-datasource::cpu_load`（PDH % Processor Utility；sysinfo 差分回退） | sensor_service |
| CPU Clock | datasource::cpu_freq（既有链） | sensor_service |
| 温度/功耗/电压/风扇/主板/GPU | sidecar-lhm（LHM） | lhm::snapshot |
| 内存 | sysinfo（GlobalMemoryStatusEx） | sensor_service |
| 磁盘 IO/Activity | datasource::disk_io（PDH + % Disk Time 扩展） | sensor_service |
| 磁盘温度/电池 | sidecar-lhm 契约 v3 扩展（Storage/Battery 开启） | lhm::snapshot |
| 网络速率/IP/链路 | datasource::netif（GetIfTable2 差分由 service 计算） | sensor_service |
| 智能匹配策略 | 新 `secm-core::sensor_match`（MOBO.Temp 策略 + FanMapper 等价 + GPU 熔断 + CPU Voltage 排除规则） | sensor_service 调用 |
| SMART/盘型号 | hardware.rs 按需域（不变） | hardware 页 |

## 6. 下线清单（ADR-0006 执行）

- `secm-datasource::net_io::get_net_io_speed_map()`（PDH Network Interface）→ Phase 5 删除；`tcp_connection_count()` 迁至 netif.rs 后整文件删除。
- `sensor_service` 中 sysinfo 频率回退（L92-98）→ Phase 1 移除。
- `sysinfo::System::refresh_cpu_usage` 保留为 CPU Load 回退层（= % Processor Time 语义）。
- dashboard.rs L963-965 直调 netif → 改读 snapshot.net。
- lhm.rs 契约 v2 → v3（新增 storage/battery；CPU voltage 过滤；GPU 熔断在 sidecar 内完成）。

## 7. Gate

- 同一指标唯一 authoritative source（映射表 §4 职责划分为实现基准）。
- UI 层零底层 API 直调（dashboard 直调点清除）。
- 全部不可用语义走 `Metric.value=None + error`，无 0 值污染。
