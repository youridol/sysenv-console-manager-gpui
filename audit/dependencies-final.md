# 依赖、许可与 sidecar 边界（ADR-0009 最终清单）

## A. 本任务依赖变更

**零新增依赖。** 全部迁移能力用现有依赖实现：

| 能力 | 实现 | 现有依赖 |
| --- | --- | --- |
| CPU Load（PDH % Processor Utility/Time） | 纯 Rust FFI | windows-sys（Performance feature，已有） |
| 磁盘 % Disk Time | 纯 Rust FFI | windows-sys（同上） |
| AC 电源状态 | 纯 Rust FFI | windows-sys（Power feature，已有） |
| GetIfTable2 过滤/差分 | 纯 Rust FFI | windows-sys（IpHelper feature，已有） |
| GetSystemPowerStatus | 纯 Rust FFI | windows-sys（Power feature，已有） |

**删除依赖面**：`secm-datasource::net_io`（PDH Network Interface，非 LiteMonitor 来源）已整文件删除；无 crate 级依赖可删（windows-sys 的 IpHelper/Performance feature 仍被其余模块使用）。

## B. Rust 依赖清单（发布包）

| crate | 版本 | 许可 | 用途 | 备注 |
| --- | --- | --- | --- | --- |
| gpui | 0.2 | Apache-2.0 | UI 框架 | 既有 |
| windows-sys | 0.61 | MIT OR Apache-2.0 WITH Windows-FFI-exception | Win32 FFI（PDH/IpHelper/Power/Registry…） | 既有 |
| winreg | 0.56 | MIT | 注册表 | 既有 |
| wmi | 0.18 | MIT | WMI（activation/SMART 兜底，非硬件指标域） | 既有，非硬件功能保留 |
| sysinfo | 0.30 | MIT | CPU 差分/内存（GlobalMemoryStatusEx）/磁盘容量 | 既有；作为 LiteMonitor 等价 API 层保留 |
| ureq | 2 | MIT | sidecar HTTP / 公网回显 | 既有 |
| parking_lot / serde / thiserror / log / chrono | — | MIT/Apache-2.0 | 基础 | 既有 |

## C. sidecar-lhm（.NET 8）

| 依赖 | 版本 | 许可 | 边界 |
| --- | --- | --- | --- |
| LibreHardwareMonitorLib | 0.9.6 | MPL-2.0 | 仅 sidecar 进程内引用；SECM（MIT）经 HTTP/JSON 消费；**无 LHM 类型跨进程共享**（Rust 端独立 serde 结构） |
| System.Text.Json | 9.x | MIT | 序列化 |

运行形态：win-x64 self-contained（无目标机 .NET runtime 依赖）。

## D. third_party 资产（驱动/内核通道）

| 资产 | 许可 | 角色 |
| --- | --- | --- |
| PawnIO 2.2.0 | GPL-2.0 WITH IOCTL 用户态通信例外（namazso/PawnIO 声明） | LHM 主 ring0 后端（WHQL + 有效时间戳，2026-08-14 核实） |
| WinRing0x64.sys | OpenLibSys BSD-3-style | 回退后端（GlobalSign 商业签名，非 WHQL） |

许可义务：MPL-2.0 要求——LHM 源码随发行包附送（resources/lhm/source/）+ 对 LHM 文件的修改以 MPL 声明；PawnIO 例外条款允许用户态 IOCTL 通信不传染 GPL（sidecar 进程隔离 + 随包附送许可文本）；OpenLibSys 许可文本随包（third_party/OpenLibSys-LICENSE.txt）。

## E. LiteMonitor 能力 → SECM 依赖对照

| LiteMonitor 能力 | SECM 对应实现 | 新增依赖 |
| --- | --- | --- |
| .NET PerformanceCounter（CPU Load/Freq、Memory、Disk） | windows-sys PDH 纯 Rust | 无（C# Runtime 换为系统 pdh.dll） |
| LibreHardwareMonitorLib | sidecar 隔离（既有） | 无 |
| System.Net.NetworkInformation | windows-sys IPHLPAPI（GetIfTable2/GetAdaptersAddresses/GetExtendedTcpTable） | 无 |
| System.IO.DriveInfo | sysinfo（GetDiskFreeSpaceExW 等价） | 无 |
| System.Windows.Forms.SystemInformation.PowerStatus | windows-sys GetSystemPowerStatus | 无 |
| PresentMon/ETW（FPS） | 未迁移（REQUIRES ADMIN） | — |

## ADR-0009 Gate 自检

- [x] audit/dependencies-final.md（本文件）+ 许可清单
- [x] 零新增依赖；sidecar 内外无 LHM 类型共享
- [x] 发行包许可义务清单（MIT + MPL-2.0 附源 + PawnIO 例外 + OpenLibSys）
