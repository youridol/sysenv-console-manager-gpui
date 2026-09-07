# 目标项目采集链路审计（ADR-0003）

来源：`Y:\sysenv-console-manager-gpui` @ `b55e831`（本地 main；ADR-0001 锁定基线 `eedaae7` 之后的 UI 重构提交，硬件采集层未受影响）。逐文件实读结论；逐指标归属见 `audit/target-source-inventory.csv`。

## 1. 现行"采集 → 标准化 → cache → core → UI"路径

```text
secm-datasource（叶子层，纯 Rust）
  ├─ cpu_freq.rs      CPU 频率降级链 ntapi→PDH→registry（诊断完整）
  ├─ disk_io.rs       PDH PhysicalDisk(*) 每卷读写速率
  ├─ net_io.rs        PDH Network Interface(*) 每网卡速率 + GetExtendedTcpTable 连接数
  ├─ netif.rs         GetIfTable2 链路速度/InOctets/OutOctets + GetAdaptersAddresses IP/配置
  ├─ disk.rs          IOCTL 磁盘枚举 + NVMe/ATA SMART（WMI 兜底）
  └─ power/activation/registry/service/dns  非硬件域
        ↓
secm-core（编排层）
  ├─ sensor_service.rs  1s 后台线程：sysinfo(CPU usage/Memory/Disks) + cpu_freq + disk_io
  │                     + lhm::snapshot()（2s TTL 缓存/5s 失败退避）→ SensorSnapshot
  ├─ lhm.rs             sidecar HTTP 客户端（health/启动/受控退出/taskkill 清理）
  ├─ sensor.rs          SensorSnapshot 契约（CpuData/GpuData/MemoryData/DiskData/MotherboardData + diag）
  ├─ sensor_history.rs  60s 趋势采样（1s 节拍）+ 跨重启持久化
  ├─ net_info.rs        侧栏网络信息编排（netif + net_io + 公网回显）
  ├─ netif.rs           datasource 薄封装
  └─ hardware.rs        磁盘清单 + SMART 编排（硬件页）
        ↓
secm-app（GPUI UI）
  ├─ dashboard.rs    SensorService::start_once() + snapshot() 1s 轮询；趋势图；
  │                  ⚠ L963-965 直调 secm_core::netif::{if_bytes_map,tcp_connection_count,link_speeds}
  ├─ hardware.rs     hardware::{list_disks, read_smart}（按需）
  ├─ pi_clone/shell.rs  net_info::collect_net_info / refresh_local_rate（侧栏）
  ├─ net_config.rs   netif::list_adapters()
  ├─ environment.rs  sysinfo::get_system_info()
  └─ main.rs         退出钩子 lhm::shutdown()（P1-3 受控退出）
```

## 2. sidecar-lhm 现状（实读 Program.cs）

- 契约 v2：`available/error/contract_version` + `cpu{package_temp_c,core_temps_c,power_w,fan_rpm,voltage_v}` + `gpu[]{name,temperature_c,core_clock_mhz,power_w,load_percent,memory_used_bytes,memory_total_bytes,fan_rpm}` + `motherboard{sensors[name,type,value]}` + `memory{name(SPD 缓存),total_bytes,used_bytes}`。
- UAC：启动即 `runas` 自重启提权；用户取消 → 普通权限运行 + `available:false` + 明确错误（无伪造值）。
- Ring0 预检：PawnIO（主，WHQL+时间戳有效）→ WinRing0（回退，GlobalSign 签名）；CreateFileW 探测 + 错误码→可读诊断。
- LHM 开关：CPU/GPU/Memory/Motherboard=true；**Storage/Battery/Network/Controller/PSU=false**（→ 磁盘温度/电池/网络吞吐需扩展契约）。
- 值过滤：`<=0 / NaN / Inf` 全部跳过（无伪造 0）；`available` 仅在 package_temp 有效时为 true。
- 1s 采集线程 + 快照锁；初始化失败 5s 退避重试；错误去重日志；/api/shutdown 受控退出。

## 3. 同一指标多来源清单（全部归属）

| 指标 | 来源 A | 来源 B | 归属 |
| --- | --- | --- | --- |
| CPU.Load | sysinfo cpu_usage（% Processor Time 语义） | （无第二来源） | MERGE：对齐 LiteMonitor 链 = PDH `% Processor Utility` 主路径 + 现实现为回退层 |
| CPU.Clock | cpu_freq 降级链 | sysinfo `frequency()`（sensor_service L92-98 回退） | MERGE：cpu_freq 链保留；sysinfo 回退移除（registry 标称层已覆盖同语义，避免双默认） |
| NET.Up/Down | net_io.rs PDH | netif.rs GetIfTable2 差分（dashboard L963 直调） | REPLACE：GetIfTable2 差分 = LiteMonitor LHM Network Throughput 等价底层 API → Canonical；net_io.rs 网络速率路径 Phase5 下线 |
| MEM.Total/Used/Load | sysinfo refresh_memory | （无第二来源） | KEEP：sysinfo Windows 实现即 GlobalMemoryStatusEx = LiteMonitor 同源 API |
| DISK.Read/Write | disk_io.rs PDH | （无第二来源） | KEEP：= LiteMonitor PerfCounter 同源 |
| DISK.Used% | sysinfo Disks | （无第二来源） | KEEP：GetDiskFreeSpaceExW = DriveInfo 同源 |
| 网络侧栏速率 | net_info(net_io PDH) | dashboard(netif 差分) | 统一后同走 Canonical（GetIfTable2 差分），net_info 编排保留但改用统一速率 |

## 4. UI 越层访问（ADR-0005 需修复项）

1. `dashboard.rs` L963-965：直调 `secm_core::netif::{if_bytes_map, tcp_connection_count, link_speeds}`（绕过 snapshot，每帧触达 IPHLPAPI）→ 迁入 SensorSnapshot 网络域。
2. `dashboard.rs` L201/226、`hardware.rs`：`hardware::{list_disks, read_smart}` 属按需功能页（SMART 详情弹窗），非热路径，保留但记录为"低频按需域"。
3. `pi_clone/shell.rs`：`net_info::collect_net_info()` 为 core 编排层调用（内部管理缓存与降级），可接受；统一后其本地链路字段改读 snapshot。

## 5. 非硬件域（不迁移、不改动）

- `sysinfo.rs`（OS 静态 8 字段）、`power.rs`（电源计划管理）、`network.rs`（网络诊断工具）、`net_config.rs`、`cleanup.rs`、`game_env.rs`、`environment.rs`、`activation/registry/service/dns.rs`。
- **修正**：ADR-0001 中"power.rs 与 LiteMonitor BatteryService 同源"为误判——power.rs 是电源计划管理（powercfg 等价），无电池采集；目标项目当前**完全没有电池指标**。

## 6. 缺失能力（对照 LiteMonitor，→ ADR-0004/0006 落实）

- DISK.Activity（% Disk Time）、DISK.Temp（LHM Storage）、Battery 全域（LHM Battery + AC 状态）、CPU.Fan/Pump/CASE.Fan 智能匹配（FanMapper）、MOBO.Temp 智能选择策略、GPU 熔断策略（Clock>6000/Power>1200）、CPU.Voltage 排除规则（soc/gt/sa/aux）、GPU 核显 Shared 显存优先规则。
- 双方均无：Display/显示器信息、主板电压标准键、SMART 实时消费（LiteMonitor 无；SECM 有独立页）。
- LiteMonitor 有但不迁移（权限/成本）：FPS（PresentMon+ETW，REQUIRES ADMIN）、SMB 流量扣除（依赖流量统计域）。

## 7. ADR-0003 Gate 自检

- [x] audit/target-source-inventory.csv（每指标归属：KEEP/REPLACE/MERGE/REMOVE/FALLBACK/NEW/SUSPEND）
- [x] 无未归属来源（§3 全部标注归属）
- [x] UI 越层访问已标记（§4）
