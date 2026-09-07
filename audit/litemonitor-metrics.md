# LiteMonitor 指标级审计（ADR-0002）

来源：`Y:\_refs\LiteMonitor` @ `169e90a`，逐文件实读（audit/litemonitor-callgraph.md 为链路总览）。
每条记录生命周期：首次采集 → 预热 → 正常刷新 → tick cache → last-valid cache → reload → 失败退避 → 恢复。

## CPU

| 指标 | 主来源 | API/实现 | 权限 | Fallback 链 | 缓存/刷新 | 错误处理 |
| --- | --- | --- | --- | --- | --- | --- |
| CPU.Load | PerformanceCounter | Processor Information\% Processor Utility\_Total → % Processor Time → Processor 类别 | 普通用户可用 | → LHM "CPU.Load"（Total/package）→ ComponentProcessor 核心平均 → **0f（缺陷）** | SafeRead；tick cache；lastValid；Utility>100 截断 | SafeRead→null；IsInitialized=false 整域回退 LHM |
| CPU.Temp | LHM Cpu sensors | SensorMatcher（package/tctl/tdie/ccd/cores…） | 需 CPU 驱动（PawnIO），普通用户下 LHM CPU 温度节点多数不可用 | ComponentProcessor 核心**最大值** → SensorMap 直配 → **0f（缺陷）** | CPU 每拍 Update；tick cache；lastValid（通用兜底） | 值 NaN→lastValid |
| CPU.Clock | PerformanceCounter | % Processor Performance × 注册表 ~MHz | 普通用户可用 | → CpuCoreCache 核心平均（>400MHz 参与均值；Zen5 Bus Speed 修正）→ maxRaw | tick cache；UpdateMaxRecord 记录峰值 | percent/base 无效→null |
| CPU.Power | LHM Cpu sensor | SensorMatcher（package/cores） | 需 PawnIO 驱动 | 无（仅熔断：>600W 返回 null） | tick cache；UpdateMaxRecord | NaN→null |
| CPU.Voltage | LHM Cpu sensor | SensorMatcher（vcore/vid…，排除 soc/gt/sa/aux） | 需 PawnIO 驱动 | 通用兜底 lastValid | CPU 每拍 Update | NaN→lastValid |
| CPU.Fan | LHM（SuperIO/Cooler） | FanMapper 智能匹配（>200 RPM 底噪过滤；用户优先） | 需 PawnIO + Controller 开启 | 匹配链见 callgraph §6；无传感器→无 key（UI 无值） | Controller 开启时才扫描；UpdateMaxRecord | 无传感器→无映射 |

## GPU（多卡：GetHwPriority 排序，PreferredGpu 锁定，仅更新 CachedGpu）

| 指标 | 主来源 | API/实现 | 权限 | Fallback 链 | 缓存/刷新 | 错误处理 |
| --- | --- | --- | --- | --- | --- | --- |
| GPU.Load | LHM GPU | Load 含 core / d3d 3d；D3D(动态) vs Vendor 冲突时 Vendor 优先 | 需驱动（NvAPI/AMD AGS/IPMI 内建）；普通用户通常可用 | 通用 lastValid | GPU 每拍 Update（仅选中卡） | Update 抛异常→needsReload→2s 后 Reload |
| GPU.Temp | LHM GPU | Temp 含 core/hot spot/soc/vr | 同上 | lastValid | 同上 | NaN→lastValid |
| GPU.Clock | LHM GPU | CachedGpu 现场 Filter（graphics/core/shader） | 同上 | 熔断 >6000MHz→null | tick cache；UpdateMaxRecord | NaN→null |
| GPU.Power | LHM GPU | Filter package/ppt/board/core/gpu | 同上 | 熔断 >1200W→null | tick cache | NaN→null |
| GPU.VRAM | LHM GPU | Used/Total 计算 %（单位自适应 MiB/Bytes）；核显 shared 优先、独显 dedicated | 同上 | → LHM GPU.VRAM.Load sensor | tick cache | used/total 缺→fallback sensor |
| GPU.Fan | LHM GPU | 第一个 Fan → Control sensor | 同上 | 无 | GPU 每拍 Update | 无传感器→无 key |

## Memory

| 指标 | 主来源 | API/实现 | 权限 | Fallback 链 | 缓存/刷新 | 错误处理 |
| --- | --- | --- | --- | --- | --- | --- |
| MEM.Load | PerformanceCounter | Memory\Available MBytes + GlobalMemoryStatusEx 总量 → load% | 普通用户 | → LHM (Used+Available) 计算 → LHM MEM.Load sensor | useCounter 时 LHM 不轮询；tick cache | 总量失败→16GB 假值（缺陷，不迁移） |
| MEM.Total | PerfCounter(GlobalMemoryStatusEx) / LHM Used+Available 推算 | Settings.DetectedRamTotalGB 首次探测后固定 | 普通用户 | — | 启动静态 | 缺→16GB（缺陷） |
| MEM.Used / MEM.Available | LHM Memory | SensorMatcher（used / available 或 free；排除 virtual） | 普通用户 | lastValid | useCounter=false 时每拍 Update | NaN→lastValid |
| SPD（内存条详细信息） | LHM Memory 硬件信息 | LHM Memory 硬件 Name/（SPD 传感器在 LHM 中属 Memory 子树，LiteMonitor 未消费 SPD 明细） | — | — | LiteMonitor 未使用 SPD 明细 → 迁移范围外（记录为未消费） | — |

## Motherboard / SuperIO

| 指标 | 主来源 | API/实现 | 权限 | Fallback 链 | 缓存/刷新 | 错误处理 |
| --- | --- | --- | --- | --- | --- | --- |
| MOBO.Temp | LHM Motherboard/SuperIO | 智能策略 System>Motherboard>Chipset/PCH>合理(15-68)最大>宽(0-95)最大 | 需 PawnIO（SuperIO 芯片访问） | ReadMoboTemperature：无效/超硬上限（Auto 95/Manual 125）→ lastValid（>0 且 ≤上限） | 每 3s UpdateWithSubHardware | 值无效→lastValid→null |
| CASE.Fan / CPU.Fan / CPU.Pump | LHM SuperIO/Cooler | FanMapper | 需 PawnIO + Controller | 见 FanMapper；UpdateMaxRecord | 3s 慢扫 | 无传感器→无 key |
| MOBO.Voltage | LHM SuperIO | SensorMatcher **无主板电压规则**（CPU.Voltage 仅 Cpu 硬件）| 需 PawnIO | — | — | LiteMonitor 未消费主板电压标准键 → 未覆盖项 |

## Disk

| 指标 | 主来源 | API/实现 | 权限 | Fallback 链 | 缓存/刷新 | 错误处理 |
| --- | --- | --- | --- | --- | --- | --- |
| DISK.Read/Write | PerformanceCounter | PhysicalDisk\Disk Read/Write Bytes/sec\_Total | 普通用户 | → LHM Storage Throughput（指定 PreferredDisk 时 LHM 优先）→ lastValid | SafeRead；tick cache | SafeRead→null |
| DISK.Activity | PerformanceCounter | PhysicalDisk\% Disk Time\_Total | 普通用户 | **无 LHM fallback**（仅计数器）| Clamp 0-100 | SafeRead→null |
| DISK.Temp | LHM Storage | FindBestTempSensor（排除 warning/critical，优先 "Temperature"） | 取决于盘（NVMe 温度普通用户可用；SATA 需驱动） | lastValid | 缓存命中即返回（key 含 Temp 恒读） | 无传感器→null |
| DISK.Used（逻辑盘 DISK.C.Used） | DriveInfo | 100 − Free/Total×100 | 普通用户 | 无 | DriveInfo 静态缓存 | IsReady=false→null；异常→**0f（缺陷）** |
| 物理盘选盘 | LHM Storage | 评分 read+write，系统盘 +1e9；LastAutoDisk 记忆 | — | — | 运行时缓存 + 记忆 | 缓存存活检查（防僵尸对象） |
| SMART | **LiteMonitor 未实现 SMART 明细读取**（LHM Storage 有 SMART 但 LiteMonitor 未消费；驱动器健康状态未展示） | — | — | — | — | 未覆盖项 |

## Network

| 指标 | 主来源 | API/实现 | 权限 | Fallback 链 | 缓存/刷新 | 错误处理 |
| --- | --- | --- | --- | --- | --- | --- |
| NET.Up/Down | LHM Network Throughput | GetBestValue 评分选卡（虚拟 -1e9） | 普通用户 | → lastValidMap | 缓存 3s 内直接用；目标卡每拍 Update | NaN→lastValid |
| NET.IP | .NET NetworkInterface（Native） | GetCurrentIP：30s 缓存；策略A 匹配适配器 → 策略B 全系统遍历 | 普通用户 | 静态缓存兜底（旧值） | NetworkAddressChanged 事件触发重置 | 无 IPv4→旧缓存→"" |
| 流量统计（TrafficLogger） | Native GetIPStatistics 主 / LHM 估算备 | 增量累积；SMB 扣除 ×1.2；>10GB 丢弃 | 普通用户 | LHM 估算 | 每拍累积 | 匹配错误自动解绑重试（10s 节流） |
| Link Speed | **LiteMonitor 未消费**（LHM Network 有 Speed 传感器但 SensorMap/NetworkManager 未映射标准键） | — | — | — | — | 未覆盖项 |

## Battery

| 指标 | 主来源 | API/实现 | 权限 | Fallback 链 | 缓存/刷新 | 错误处理 |
| --- | --- | --- | --- | --- | --- | --- |
| BAT.Percent | LHM Battery | Level（优先 Charge；排除 Degradation/Wear） | 普通用户（LHM Battery 走 Win32 电源 API 通道） | 无 | Battery 每拍 Update（配置开启时） | 无传感器→null |
| BAT.Power / BAT.Current | LHM Battery | Power/Current sensor + AcOnline 符号强制 | 普通用户 | 无 | 同上 | 无传感器→null |
| BAT.Voltage | LHM Battery | Voltage sensor | 普通用户 | 无 | 同上 | 无传感器→null |
| AC 状态 | WinForms SystemInformation.PowerStatus | GetPowerStatus 3s 节流缓存 | 普通用户 | — | 3s 缓存 | 异常→旧缓存 |

## 其他

| 指标 | 主来源 | 说明 |
| --- | --- | --- |
| FPS | PresentMon sidecar（LiteMonitorFPS.exe）+ ETW | 需管理员；独立采集域；普通用户不可用 |
| DATA.DayUp/Down | TrafficLogger（会话/每日流量持久化） | 流量统计域，非硬件传感器 |
| UpTime | PerfCounter System\System Up Time | 辅助指标 |
| 显示器（Display） | **LiteMonitor 未实现**（无 monitor 信息采集代码） | 未覆盖项 |
| BIOS/主板型号 | LHM Motherboard Name（硬件树） | 仅硬件扫描列表使用，非标准指标键 |

## 缓存与刷新分类总结（供 ADR-0008 设计）

```text
高频（每拍 ~1s）：CPU.Load/Clock(PerfCounter)、MEM.Load(PerfCounter)、DISK.Read/Write/Activity(PerfCounter)、
                 GPU.*（选中卡 LHM Update）、NET.Up/Down（目标网卡）
中频（3s 慢扫）：Motherboard/SuperIO/Cooler 递归 Update、非目标网卡 Update
低频（10s）：磁盘后台扫描（冷却态）
退避（1min/5min）：磁盘活跃/冷却/深睡三态
事件驱动：网络地址变更 → IP/适配器重置；GPU Update 异常 → 2s 延迟重载
静态：~MHz 基准频率、总内存（GlobalMemoryStatusEx 一次）、DriveInfo 缓存、硬件扫描列表（HardwareScanner 5 类缓存）
兜底链：tick cache（帧内）→ 对象缓存（ISensor.Value）→ lastValidMap（跨帧最后有效值）
```

## ADR-0002 Gate 自检

- [x] audit/litemonitor-callgraph.md
- [x] audit/litemonitor-metrics.md（本文件）
- [x] 逐文件确认（HardwareMonitor/HardwareValueProvider/SensorMap/SensorMatcher/HardwareRules/ComponentProcessor/NetworkManager/DiskManager/BatteryService/PerformanceCounterManager/HardwareScanner/DriverInstaller/FanMapper/FpsCounter/Program/消费端关键路径）
