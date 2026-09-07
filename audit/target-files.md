# 目标项目硬件相关文件清单（ADR-0001）

来源：`Y:\sysenv-console-manager-gpui` @ `eedaae79ffc60156856c3e9fa722bd3abba5579c`（main）。
逐文件来源归属与 KEEP/REPLACE/MERGE/REMOVE/FALLBACK 判定在 ADR-0003（audit/target-source-inventory.csv）完成。

## crates/secm-datasource（纯 Rust 底层采集）

| 文件 | 初读职责（待 ADR-0003 确认） |
| --- | --- |
| lib.rs | 模块导出 |
| activation.rs | 激活状态采集（WMI）—— 非硬件 |
| cpu_freq.rs | CPU 频率采集 —— 疑与 LHM CPU clock 重复 |
| disk.rs | 磁盘枚举/SMART（存储 IOCTL）—— 疑与 LHM Storage 重复 |
| disk_io.rs | 磁盘 IO 速度（PDH 性能计数器）—— 与 LiteMonitor PerfCounter 同源 |
| dns.rs | DNS 缓存刷新 —— 非硬件 |
| error.rs | 错误类型 |
| net_io.rs | 网络 IO 速率 —— 与 LiteMonitor NetworkManager 部分同源 |
| netif.rs | 网卡枚举（GetIfTable2 / IP_ADAPTER_ADDRESSES） |
| power.rs | 电源/电池（Win32 Power API）—— 与 LiteMonitor BatteryService 同源 |
| registry.rs | 注册表读取工具 |
| service.rs | Windows 服务查询 —— 非硬件 |

## crates/secm-core（业务编排）

| 文件 | 初读职责（待 ADR-0003 确认） |
| --- | --- |
| hardware.rs | 硬件数据编排 —— 疑似旧采集主入口 |
| lhm.rs | LHM sidecar HTTP 客户端 |
| sensor.rs / sensor_service.rs / sensor_history.rs | 传感器模型/服务/历史 |
| net_info.rs / netif.rs / network.rs | 网络信息编排（多个入口，需归属） |
| proc_util.rs | 进程工具（非硬件） |
| sysinfo.rs | sysinfo crate 系统信息 8 字段（datasource 主路径 + PS 回退） |
| cleanup.rs / environment.rs / game_env.rs / net_config.rs / settings.rs / logger.rs / error.rs | 非硬件或边缘 |

## crates/secm-app（GPUI UI）

| 文件 | 消费内容 |
| --- | --- |
| pages/dashboard.rs | 仪表盘（硬件指标消费） |
| pages/hardware.rs | 硬件页 |
| pages/network.rs | 网络页 |
| pages/settings.rs / services.rs / cleanup.rs / environment.rs / ai_environment.rs / about.rs / net_config.rs | 非硬件消费为主（待确认 settings 硬件引用） |

## sidecar-lhm（.NET 8 隔离进程）

| 文件 | 职责 |
| --- | --- |
| Program.cs | LHM Computer 持有 + HTTP/JSON 输出 |
| sidecar-lhm.csproj | net8.0，win-x64 self-contained，LibreHardwareMonitorLib 0.9.6 |

## third_party 资产

| 文件 | 说明 |
| --- | --- |
| PawnIO/ | PawnIO 相关 |
| WinRing0x64.sys | WinRing0 驱动（OpenLibSys 许可） |
| OpenLibSys-LICENSE.txt | WinRing0 许可 |

## 初始疑似重复来源（→ ADR-0003 归属）

1. CPU 频率：cpu_freq.rs vs LHM clock。
2. CPU 负载：? vs LHM/PerfCounter。
3. 内存：sysinfo.rs vs LHM Memory vs Win32 GlobalMemoryStatusEx。
4. 磁盘：disk.rs（枚举/SMART）、disk_io.rs（PDH）vs LHM Storage。
5. 网络：net_io.rs/netif.rs vs net_info.rs/network.rs vs LHM Network。
6. 电池：power.rs vs LHM Battery。
7. GPU/Motherboard：当前旧链路疑似完全缺失（待确认）。
