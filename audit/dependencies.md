# 依赖清单（ADR-0001 / ADR-0009 共用底稿）

## A. LiteMonitor（上游）依赖

来源：`LiteMonitor.csproj` @ `169e90a`。

| 包 | 版本 | 用途 | 许可 |
| --- | --- | --- | --- |
| LibreHardwareMonitorLib | 0.9.6 | CPU/GPU/主板/内存 SPD/存储温度/电池等传感器 | MPL-2.0 |
| System.Text.Json | 9.0.10 | JSON（网页端/配置） | MIT |

框架：.NET 8（net8.0-windows），WinForms，x64，仅 Windows。

随包二进制资产（许可义务见 ADR-0009）：

- `resources/assets/driver.zip` —— WinRing0 驱动包（OpenLibSys BSD-3 / 军团级遗留许可问题在 ADR-0009 评估）
- `resources/assets/PawnIO_setup.exe` —— PawnIO 安装器
- `resources/LiteMonitor.Updater.exe` —— 更新器

## B. 目标项目 Rust 依赖（硬件相关）

来源：各 Cargo.toml @ `eedaae7`。

### workspace 根

| 依赖 | 版本 | 说明 |
| --- | --- | --- |
| gpui | 0.2 | UI 框架（Zed） |
| serde / serde_json | 1 | 序列化 |
| thiserror | 2 | 错误派生 |
| log | 0.4 (std) | 日志 |
| ureq | 2 | HTTP 客户端（sidecar 通信） |
| winreg | 0.56 | 注册表 |
| windows-sys | 0.61 | Win32 FFI |

### secm-datasource

| 依赖 | 用途 | 硬件相关性 |
| --- | --- | --- |
| windows-sys（多 feature：Performance/Storage/Ioctl/Ndis/IpHelper/Power/Registry/…） | Win32 FFI | 高 —— PDH 磁盘 IO、IP_HELPER 网络、Power 电池、IOCTL SMART |
| winreg | 注册表 | 中（cpu_freq? 待 ADR-0003） |
| wmi 0.18 | WMI 查询 | 低（activation/补丁兜底，非硬件指标） |
| thiserror/log/serde | 基础 | — |

### secm-core

| 依赖 | 用途 | 硬件相关性 |
| --- | --- | --- |
| sysinfo 0.30 | 系统信息 8 字段 | 高 —— 疑似与 LHM 重复的旧来源（ADR-0003 判定） |
| windows-sys | Win32 | 中 |
| winreg / ureq / chrono / parking_lot / encoding_rs | 基础 | — |

### secm-app

| 依赖 | 用途 |
| --- | --- |
| gpui 0.2 | UI |
| tray-icon / image / raw-window-handle / winresource | 托盘/窗口 |
| windows-sys（Foundation/Threading/LibraryLoader/KeyboardAndMouse/WindowsAndMessaging/Dwm） | 窗口桥接（非硬件采集） |

## C. sidecar-lhm（.NET）

| 依赖 | 版本 | 许可 | 边界 |
| --- | --- | --- | --- |
| LibreHardwareMonitorLib | 0.9.6 | MPL-2.0 | 仅 sidecar 进程内引用；SECM（MIT）经 HTTP/JSON 消费；源码随发行包附送（resources/lhm/source/） |

运行形态：win-x64 self-contained（无需目标机 .NET runtime），依赖 DLL 展开。

## D. third_party 资产

| 资产 | 许可 | 用途（待 ADR-0007/0009 确认） |
| --- | --- | --- |
| WinRing0x64.sys | OpenLibSys（BSD-3-style） | LHM 内核驱动（管理员/服务加载） |
| PawnIO/ | PawnIO 许可（见目录） | LHM 0.9.6 支持的更安全驱动替代 |

## E. 待 ADR-0009 决议事项

1. wmi 0.18 是否可由更小依赖替代（当前仅 activation 用）—— 非硬件功能，不动。
2. sysinfo 0.30 若被 LHM snapshot 取代 → 删除依赖。
3. ureq 版本与 sidecar 超时/退避能力确认。
4. 发行包许可义务：MIT + MPL-2.0（sidecar 隔离 + 源码附送）+ OpenLibSys + PawnIO 清单。
