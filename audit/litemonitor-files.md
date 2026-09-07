# LiteMonitor 硬件文件清单（ADR-0001）

来源：`Y:\_refs\LiteMonitor` @ `169e90a2ccaa68ca29d0b549d10c6c59634c1b49`（master）。
以下职责摘要来自项目结构与文件级初读；逐行级链路审计在 `audit/litemonitor-callgraph.md`（ADR-0002）完成。

## 采集核心（src/System）

| 文件 | 行数 | 初读职责 |
| --- | --- | --- |
| HardwareMonitor.cs | 567 | 硬件总控：持有 LHM `Computer`，组织传感器树、更新节拍、对 UI/网页端输出指标 |
| Program.cs | 139 | 进程入口：单实例、初始化顺序、退出清理 |
| HardwareScanner.cs | 217 | 硬件树扫描/重扫描（子硬件枚举，如 CPU cores、GPU 子设备、存储盘） |
| SensorMap.cs | 275 | 传感器标识 → 指标的映射表 |
| SensorMatcher.cs | 152 | 传感器名匹配/筛选规则实现 |
| HardwareRules.cs | 89 | 硬件选择规则（主板/CPU/GPU 的筛选约束） |
| ComponentProcessor.cs | 173 | 按硬件类别处理传感器值并归一化 |
| HardwareValueProvider.cs | 509 | 对外取值门面：聚合 ComponentProcessor/Network/Disk/Battery/PerfCounter |
| NetworkManager.cs | 493 | 网卡枚举、吞吐（LHM Network / PerfCounter）、活跃网卡选择、IP |
| DiskManager.cs | 230 | 磁盘吞吐/活跃度（PerfCounter）、温度（LHM Storage）、容量（DriveInfo）、选盘与缓存 |
| BatteryService.cs | 76 | 电池电量/状态/电压/电流/功率，AC 修正 |
| PerformanceCounterManager.cs | 291 | Windows PerformanceCounter 封装：CPU Load、% Processor Performance 等 |
| DriverInstaller.cs | 754 | WinRing0/PawnIO 驱动安装与提权（`runas`） |
| FanMapper.cs | 147 | 风扇名映射 |
| FpsCounter.cs | 529 | FPS 采集（PresentMon/ETW 类路径，非 LHM）—— 迁移范围判断项 |
| SystemOptimizer.cs | 100 | 系统优化动作（非采集） |
| AutoStart.cs | 203 | 自启动（非采集） |
| UpdateChecker.cs | 355 | 更新检查（非采集） |

## 消费端

| 文件 | 职责 |
| --- | --- |
| src/Core/MetricItem.cs | 指标模型（key/label/value/unit/category） |
| src/Core/MetricUtils.cs | 指标取值与格式化（UI/任务栏共用） |
| src/Core/MetricLabelResolver.cs | SensorMap 标签解析 |
| src/UI/TaskbarForm.cs、Helpers/* | 任务栏/主窗体按节拍读取 HardwareValueProvider |
| src/System/WebServer/LiteWebServer.cs | 网页端消费（JSON 输出） |
| src/Core/HardwareHistoryLogger.cs | 历史记录（CSV/日志） |

## 权限相关事实

- `app.manifest`：`requireAdministrator`（主程序默认管理员）。
- `DriverInstaller.cs`：包含 `runas` 提权安装 WinRing0/PawnIO 逻辑（ADR-0002 细读确认行为）。
- `resources/assets/driver.zip`、`PawnIO_setup.exe`：随包分发驱动。

## 平台目标

- 仅 Windows（net8.0-windows，x64）。
