# 指标级来源映射与权限矩阵（ADR-0004）

LiteMonitor 事实来源：`audit/litemonitor-metrics.md`；目标现状：`audit/target-source-inventory.csv`。
权限枚举：`USER_SAFE`（普通用户稳定可用）/ `USER_BEST_EFFORT`（普通用户通常可用，依赖硬件/驱动）/ `ADMIN_REQUIRED`（底层能力需管理员/特权驱动）/ `UNAVAILABLE`（无可靠等价接口）。

判定总则：
- LiteMonitor 的 PerfCounter 路径 → SECM 纯 Rust PDH（windows-sys）= 严格等价 API。
- LiteMonitor 的 LHM 路径 → sidecar-lhm（LibreHardwareMonitorLib 0.9.6 同库）= 同一来源。
- LiteMonitor 的 .NET Native 路径（NetworkInterface/DriveInfo/PowerStatus）→ IPHLPAPI/GetDiskFreeSpaceExW/GetSystemPowerStatus = 同一底层 Win32 API。
- LiteMonitor 历史缺陷（失败→0、2500MHz、16GB 默认）**不迁移**；真实失败返回 `available=false + error`。

## 矩阵（每指标一行）

| metric_key | litemonitor_source | implementation (SECM) | permission | fallback 链（Primary→…→Final） | cache/refresh | target_owner | remove_sources | status |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CPU.Load | PerfCounter `% Processor Utility`→`% Processor Time`→LHM Total | **P1**: PDH `\Processor Information(_Total)\% Processor Utility` → 回退现 sysinfo 差分（= % Processor Time 等价） | USER_SAFE | PDH Utility → sysinfo Time 差分 → unavailable（不再 0f） | tick 缓存 1s；PDH warmup | secm-core::sensor_service | 无双源（sysinfo 降为回退层） | mapped |
| CPU.Clock | PerfCounter `% Processor Performance × ~MHz`→LHM 核心平均 | cpu_freq 链 ntapi→PDH `% Processor Performance×~MHz`→registry 标称 | USER_SAFE | ntapi → PDH → registry 标称 → unavailable | 1s；进程级 PDH 单例+warmup | secm-datasource::cpu_freq | 移除 sensor_service 中 sysinfo freq 回退 | mapped |
| CPU.Temp | LHM CPU sensors（package/tctl/tdie/ccd…） | sidecar package_temp_c + core_temps_c（Tctl/CPU Package 匹配） | ADMIN_REQUIRED（PawnIO/WinRing0 + sidecar UAC） | lhm → unavailable（available=false + error） | sidecar 1s；客户端 2s TTL | sidecar-lhm + secm-core::lhm | — | mapped |
| CPU.Power | LHM CPU Power | sidecar power_w（Package） | ADMIN_REQUIRED | lhm → unavailable | 同上 | 同上 | — | mapped |
| CPU.Voltage | LHM CPU Voltage（排除 soc/gt/sa/aux） | sidecar voltage_v | ADMIN_REQUIRED | lhm → unavailable | 同上 | 同上 | — | mapped |
| CPU.Fan/Pump, CASE.Fan | LHM SuperIO/Cooler + FanMapper 智能匹配 | sidecar motherboard sensors(fan) + 快照层 FanMapper 等价匹配（底噪>200、Cooler 优先、Pump 猜想） | ADMIN_REQUIRED | 匹配策略产出 cpu_fan_rpm/case_fan_rpm/cpu_pump_rpm；无匹配 → Option::None | sidecar 1s | sidecar-lhm + secm-core::sensor | — | mapped |
| MEM.Total | GlobalMemoryStatusEx（PerfCounter 兜底） | sysinfo refresh_memory（Windows 内部即 GlobalMemoryStatusEx） | USER_SAFE | sysinfo → unavailable | 1s | secm-core::sensor_service | — | mapped |
| MEM.Used / MEM.Load | PerfCounter Available MBytes 计算 / LHM Used+Available | sysinfo used/total 计算（同源内核计数） | USER_SAFE | sysinfo → unavailable | 1s | secm-core::sensor_service | — | mapped |
| MEM.SPD | LHM Memory（LiteMonitor 未消费明细） | sidecar memory.name（SPD 缓存） | ADMIN_REQUIRED | lhm → None | 静态缓存 | sidecar-lhm | — | mapped |
| GPU.Load | LHM GPU Core Load / D3D | sidecar load_percent（GPU Core 匹配） | USER_BEST_EFFORT（NVAPI/ADL 用户态 API，多数普通用户可用；异常驱动不可用） | lhm → per-GPU None（不写 0） | sidecar 1s；仅输出真实值 | sidecar-lhm | — | mapped |
| GPU.Temp | LHM GPU Core Temp（max 兜底） | sidecar temperature_c（GPU Core + max 兜底） | USER_BEST_EFFORT | lhm → None | 同上 | sidecar-lhm | — | mapped |
| GPU.Clock | LHM GPU Clock（熔断>6000） | sidecar core_clock_mhz + 熔断策略 | USER_BEST_EFFORT | lhm → 熔断 None | 同上 | sidecar-lhm | — | mapped |
| GPU.Power | LHM GPU Power（熔断>1200） | sidecar power_w + 熔断策略 | USER_BEST_EFFORT | lhm → 熔断 None | 同上 | sidecar-lhm | — | mapped |
| GPU.VRAM | LHM Dedicated/Shared SmallData | sidecar memory_used/total_bytes（GPU Memory Used/Total；核显 Shared 规则） | USER_BEST_EFFORT | lhm → None | 同上 | sidecar-lhm | — | mapped |
| GPU.Fan | LHM GPU Fan→Control | sidecar fan_rpm | USER_BEST_EFFORT | lhm → None | 同上 | sidecar-lhm | — | mapped |
| MOBO.Temp（系统温度） | LHM 智能策略 System>Motherboard>Chipset/PCH>合理范围最大 | sidecar 全列表 + 快照层等价策略产出 system_temp_c + 保留原始列表 | ADMIN_REQUIRED | 策略产出 → None（+硬上限 Auto95/Manual125 + lastValid 语义） | sidecar 1s | secm-core::sensor（策略） | — | mapped |
| MOBO.Fans / Voltages | LHM Motherboard+SuperIO 列表 | sidecar sensors 列表（原样透传） | ADMIN_REQUIRED | 无匹配 → 空列表 | 同上 | sidecar-lhm | — | mapped |
| DISK.Read/Write | PerfCounter PhysicalDisk Read/Write Bytes/sec（\_Total 或 LHM） | disk_io.rs PDH per-volume（\_\_Total 聚合由消费端求和） | USER_SAFE | PDH → 空映射（沿用上次有效值） | 1s；进程级单例 | secm-datasource::disk_io | — | mapped |
| DISK.Activity | PerfCounter `% Disk Time\_Total` | **新增** PDH PhysicalDisk(*) % Disk Time | USER_SAFE | PDH → 沿用上次 → unavailable | 1s | secm-datasource::disk_io | — | new |
| DISK.Temp | LHM Storage Temperature | **扩展 sidecar** IsStorageEnabled=true，输出 storage[]{name,temp_c}（FindBestTemp 等价：排除 warning/critical） | USER_BEST_EFFORT（NVMe 用户态可读；SATA 依赖驱动） | lhm storage → None | sidecar 1s | sidecar-lhm | — | new |
| DISK.Capacity / Used% | DriveInfo | sysinfo Disks（GetDiskFreeSpaceExW） | USER_SAFE | sysinfo → unavailable | 1s（容量准静态） | secm-core::sensor_service | — | mapped |
| DISK.Model/Serial | （LiteMonitor 无） | disk.rs IOCTL_STORAGE_QUERY_PROPERTY（硬件页按需） | USER_SAFE | IOCTL → WMI → NotFound | 按需 | secm-core::hardware | — | keep（独立域） |
| DISK.SMART | （LiteMonitor 无） | disk.rs NVMe health / ATA PASS_THROUGH（硬件页按需） | USER_BEST_EFFORT（部分盘 NeedsAdmin） | IOCTL → WMI → NeedsAdmin/NotFound | 按需 | secm-core::hardware | — | keep（独立域） |
| NET.Up/Down | LHM Network Throughput（LHM 内部=GetIfEntry 差分） | **Canonical**: netif.rs GetIfTable2 InOctets/OutOctets 差分（1s 节拍由 SensorService 计算） | USER_SAFE | GetIfTable2 → 上次有效 → unavailable；PDH net_io 为非 LiteMonitor 来源 → 下线 | 1s 差分；>10GB/s 单拍增量丢弃（LiteMonitor 安全阀等价） | secm-core::sensor_service | **remove secm-datasource::net_io.rs 速率路径** | replace |
| NET.IPv4 | NetworkInterface.GetAllNetworkInterfaces | netif.rs GetAdaptersAddresses（APIPA 排除等价） | USER_SAFE | 适配器枚举 → 空串 | 事件驱动（IP 变化）+2s 轮询 | secm-core::net_info | — | mapped |
| NET.LinkSpeed | （LiteMonitor 未消费 Speed 键） | netif.rs GetIfTable2 TransmitLinkSpeed | USER_SAFE | 空 → 隐藏 | 准静态（2s） | secm-core::net_info | — | keep |
| NET.TCP 连接数 | （LiteMonitor 无） | net_io.rs GetExtendedTcpTable | USER_SAFE | 失败 → 0（连接数为纯计数，非硬件值，语义允许） | 1s | secm-datasource::net_io | — | keep（随 net_io 保留此函数或迁至 netif） |
| BAT.Percent | LHM Battery Level | **扩展 sidecar** IsBatteryEnabled=true 输出 battery{percent} | USER_BEST_EFFORT（Win32 电池 API，普通用户可用；台式机无电池 → None） | lhm → None（无电池非错误） | sidecar 1s | sidecar-lhm | — | new |
| BAT.Power/Current | LHM Power/Current + AcOnline 符号 | sidecar battery{power_w,current_a} + 快照层符号修正 | USER_BEST_EFFORT | lhm → None | 同上 | sidecar-lhm + sensor | — | new |
| BAT.Voltage | LHM Voltage | sidecar battery{voltage_v} | USER_BEST_EFFORT | lhm → None | 同上 | sidecar-lhm | — | new |
| AC.Status | SystemInformation.PowerStatus（3s 缓存） | **新增** GetSystemPowerStatus（kernel32，纯 Rust） | USER_SAFE | API → 上次缓存 | 3s 节流 | secm-datasource（power.rs 新函数） | — | new |
| AC 状态符号修正 | MetricUtils.GetPowerStatus | 快照层用 AC.Status 修正 BAT.Power/Current 符号 | — | — | — | secm-core::sensor | — | new |
| Display | （LiteMonitor 无） | 无 | UNAVAILABLE | — | — | — | — | not-covered（双方均无，如实记录） |
| BIOS/主板型号 | LHM Motherboard Name | sidecar motherboard.name | ADMIN_REQUIRED | lhm → None | 静态 | sidecar-lhm | — | mapped |
| FPS | PresentMon sidecar + ETW | 不迁移 | ADMIN_REQUIRED | — | — | — | — | suspend（REQUIRES ADMIN；记录真实原因） |
| 流量统计/日流量 | TrafficLogger + SMB 扣除 | 不迁移 | USER_SAFE | — | — | — | — | suspend（非硬件指标域，与 Canonical 无冲突） |

## 权限汇总（真实结论，不夸大）

- **USER_SAFE（普通用户完整可用）**：CPU.Load/Clock、Memory 全域、DISK.Read/Write/Activity/Capacity、NET.Up/Down/IPv4/LinkSpeed、AC.Status。
- **USER_BEST_EFFORT（依赖硬件/驱动）**：GPU 全域（NVAPI/ADL 用户态，异常驱动/虚拟机不可用）、DISK.Temp（NVMe 可/部分 SATA 需驱动）、Battery 全域（无电池=None）、SMART（部分盘/USB 需管理员）。
- **ADMIN_REQUIRED（需要 sidecar 提权 + ring0 驱动）**：CPU.Temp/Power/Voltage、CPU.Fan/Pump、CASE.Fan、MOBO 全域、BIOS/主板型号。
- **UNAVAILABLE / 不迁移**：Display（双方均无）、FPS（需管理员+PresentMon）、主板电压标准键（LiteMonitor 无消费）。

## ADR-0004 Gate 自检

- [x] 所有指标有来源、权限结论、fallback 结论、最终 owner、失败语义
- [x] LiteMonitor 历史假值（0f/2500MHz/16GB）标记为不迁移
- [x] 唯一权威来源无冲突（NET 双源已判 REPLACE）
