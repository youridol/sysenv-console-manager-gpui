# 缓存、刷新、错误与 Fallback 审计（ADR-0008）

## 缓存层级（实现状态）

| 层 | 实现 | 位置 |
| --- | --- | --- |
| 静态硬件元数据 | sidecar `_memorySpdName`（SPD 一次缓存）、sysinfo Disks 容量准静态 | sidecar / sensor_service |
| 对象/传感器缓存 | LHM 侧 1s 全树 Update + 快照锁；`SNAP_CACHE` 2s TTL（客户端） | sidecar / secm-core::lhm |
| tick 缓存 | SensorSnapshot 单写者 1s 轮询（UI 读 = 缓存读，无采集） | sensor_service |
| last-valid | PDH 层 `last_freq/last_load/speed_map` 沿用上次有效值（间隔不足/瞬时失败不归零）；Metric.value=None 前保留旧值的语义由 PDH 层承担 | cpu_freq/cpu_load/disk_io |
| 失败退避 | lhm `SNAP_FAIL_UNTIL` 5s 退避；sidecar 初始化失败 5s 重试；net 差分计数器回绕丢拍 | lhm / sidecar |

## 刷新策略（按数据源约束，UI 请求不驱动硬件）

| 分类 | 指标 | 频率 |
| --- | --- | --- |
| 快速 | CPU Load（PDH 1s 采样）、GPU Load/Temp、NET 速率（GetIfTable2 差分 1s） | 1s |
| 中速 | Memory、DISK IO/Activity（PDH ≥1s 采样约束） | 1s |
| 慢速 | Motherboard/SuperIO（sidecar 1s 全树 Update；LiteMonitor 为 3s 慢扫——SECM 隔离进程内无 UI 阻塞风险，频率记录为差异）、Storage 温度 | 1s / 30s |
| 静态 | SPD、容量、BIOS/主板名、链路速度 | 启动/变化时 |
| 事件驱动 | 硬件树重扫描（sidecar 异常 → 释放 Computer 下一轮重 Open） | 异常触发 |

UI 每帧只读 `SensorSnapshot`（内存克隆），零硬件访问；GPUI 1s 轮询任务亦仅消费快照（dashboard L140 与网络采样任务均已改造）。

## 错误模型（统一保留诊断）

- datasource：`CollectError::{WinApi, Registry, Http, Parse, NeedsAdmin, NotFound}`（API 名 + 错误码，log::warn 记录）。
- 快照域：`Metric.error` 保留最近一次失败原因；`SensorSnapshot.diag` 汇总帧级诊断（CPU 负载来源 / 频率 / LHM 不可用原因）。
- sidecar：`available:false + error`（UAC 取消 / Ring0 预检失败 / 采集异常，均含可辨识原因）；错误去重日志（`_lastError`）。
- 无裸 `catch{}` 吞错：Rust 侧错误全路径 log；sidecar C# catch 均写日志文件（%TEMP%\secm-lhm-sidecar.log）。

## Fallback 链（全部为 LiteMonitor 原有或严格等价）

| 指标 | Primary → Fallback → Final Failure |
| --- | --- |
| CPU.Load | PDH `% Processor Utility` → PDH `% Processor Time` → sysinfo 内核计数差分（=Time 语义）→ **unavailable**（不再 0f） |
| CPU.Clock | ntapi(CallNtPowerInformation) → PDH `% Processor Performance×~MHz` → registry 标称 → **unavailable**（无 2500 默认值） |
| CPU.Temp/Power/Volt | LHM（sidecar package 匹配 + 电压排除规则）→ **unavailable + 原因** |
| GPU.* | LHM（GPU Core 匹配 + 熔断 Clock>6000/Power>1200）→ **None** |
| MOBO.Temp | System → Motherboard → Chipset/PCH → 合理范围(15–68)最大 → 宽范围(0–95)最大 → 硬上限校验 → **None** |
| CPU.Fan/Pump/Case | FanMapper 等价匹配链 → **None**（不猜不造） |
| MEM.* | GlobalMemoryStatusEx（sysinfo）→ **unavailable**（无 16GB 默认值） |
| DISK.Read/Write/Act | PDH → 沿用上次有效 → **空映射（不可用）** |
| DISK.Temp | LHM Storage（30s 慢速）→ **unavailable** |
| NET 速率 | GetIfTable2 差分（计数器回绕/超 10GB/s 单拍丢弃）→ **上拍值/0** |
| BAT.* | LHM Battery → **None**（台式机无电池 = n/a 非错误）；Power/Current 符号按 AC 强制（充电正/放电负） |

每条 Fallback 最终失败均返回 `value=None + source=Unavailable + error=明确诊断`，禁止失败→0/默认温度/默认频率/默认容量（LiteMonitor 历史缺陷已全部剔除：0f、2500MHz、16GB）。

## ADR-0008 Gate 自检

任何采集失败可回答：哪个指标（Metric 所在域）、哪个来源（Metric.source）、为什么失败（Metric.error/diag）、用了哪个 fallback（source 变化 + fallback 链表）、最终是否可用（value 是否 Some）。✅
