# 审计基线（ADR-0001）

生成时间：2026-07-15（本机时间以执行时为准）
执行方式：只读审计，无功能代码修改。

## 1. 事实来源锁定

| 仓库 | URL | 分支 | Commit SHA | 本地检出 |
| --- | --- | --- | --- | --- |
| LiteMonitor（上游，只读） | https://github.com/Diorser/LiteMonitor | master | `169e90a2ccaa68ca29d0b549d10c6c59634c1b49` | `Y:\_refs\LiteMonitor`（仓库外，不纳入 SECM 版本管理） |
| sysenv-console-manager-gpui（目标） | https://github.com/youridol/sysenv-console-manager-gpui | main | `eedaae79ffc60156856c3e9fa722bd3abba5579c` | `Y:\sysenv-console-manager-gpui` |

目标仓库工作树在审计开始时存在与本任务无关的未提交改动（UI 主题重构：`theme.rs` → `ui/`、`pi_clone/scroll_math.rs` 等）。硬件迁移不改写这些改动；各 Phase 提交时只纳入本任务文件。

## 2. LiteMonitor 关键事实（来自 LiteMonitor.csproj / app.manifest 实读）

- 项目类型：`WinExe`，`net8.0-windows`，WinForms，`PlatformTarget=x64`，版本 1.3.6。
- 依赖：`LibreHardwareMonitorLib 0.9.6`、`System.Text.Json 9.0.10`。无其他 NuGet 包。
- UAC：`app.manifest` 第 19 行 `<requestedExecutionLevel level="requireAdministrator" uiAccess="false" />` —— LiteMonitor 主程序**默认要求管理员**。
- 硬件层入口：`src/System/HardwareMonitor.cs`（567 行）+ `src/System/HardwareServices/*`（14 个文件）。
- 附带资源：`resources/assets/driver.zip`（WinRing0 驱动包）、`resources/assets/PawnIO_setup.exe`（PawnIO 安装器）—— 驱动安装能力由 DriverInstaller 承担。

## 3. 目标项目关键事实（来自仓库实读）

- Rust workspace：`crates/secm-datasource`、`crates/secm-core`、`crates/secm-app`，workspace version 2.10.4，edition 2021，MIT。
- sidecar：`sidecar-lhm/`（`LhmSidecar.exe`，net8.0，win-x64 self-contained，引用 `LibreHardwareMonitorLib 0.9.6`，MPL-2.0 隔离边界，经 HTTP/JSON 消费）。
- 第三方资产：`third_party/`（PawnIO、OpenLibSys-LICENSE.txt、WinRing0x64.sys、README）。
- 目标项目 UI：GPUI（`gpui 0.2`），页面位于 `crates/secm-app/src/pages/*`。

## 4. LiteMonitor 必审文件清单（逐文件审计见 audit/litemonitor-files.md）

```text
src/System/HardwareMonitor.cs                                   567 行
src/System/HardwareServices/HardwareValueProvider.cs            509 行
src/System/HardwareServices/SensorMap.cs                        275 行
src/System/HardwareServices/SensorMatcher.cs                    152 行
src/System/HardwareServices/HardwareRules.cs                     89 行
src/System/HardwareServices/ComponentProcessor.cs               173 行
src/System/HardwareServices/NetworkManager.cs                   493 行
src/System/HardwareServices/DiskManager.cs                      230 行
src/System/HardwareServices/BatteryService.cs                    76 行
src/System/HardwareServices/PerformanceCounterManager.cs        291 行
src/System/HardwareServices/HardwareScanner.cs                  217 行
src/System/HardwareServices/DriverInstaller.cs                  754 行
src/System/HardwareServices/FanMapper.cs                        147 行
src/System/HardwareServices/FpsCounter.cs                       529 行
LiteMonitor.csproj                                               80 行
app.manifest                                                     81 行
```

补充审计对象（消费端 / 生命周期）：

```text
src/System/Program.cs                139 行   进程入口
src/Core/MetricItem.cs                        指标模型
src/Core/MetricUtils.cs                       指标取值/格式化消费
src/Core/MetricLabelResolver.cs               指标标签解析（SensorMap 的 UI 侧映射）
src/UI/TaskbarForm.cs                         任务栏 UI 消费者（刷新节拍）
src/UI/Helpers/MainFormBizHelper.cs           主窗体业务消费
```

## 5. 目标项目硬件相关文件清单（逐文件审计见 audit/target-files.md）

```text
crates/secm-datasource/src/{activation,cpu_freq,disk,disk_io,dns,error,net_io,netif,power,registry,service}.rs
crates/secm-core/src/{hardware,lhm,sensor,sensor_service,sensor_history,net_info,netif,network,proc_util,sysinfo}.rs
crates/secm-app/src/pages/{dashboard,hardware,network}.rs（硬件消费页面）
sidecar-lhm/Program.cs
```

## 6. 疑似重复来源（进入 ADR-0003 逐项归属）

初始标记（待 ADR-0003 确认）：

1. CPU 频率：`secm-datasource::cpu_freq`（Win32 Registry/Power?）与 LHM sidecar CPU clock 并存。
2. 磁盘 IO：`secm-datasource::disk_io`（PDH）与 sidecar/LHM、`secm-core::sensor_service` 的关系。
3. 网络：`secm-datasource::net_io`/`netif` 与 `secm-core::net_info/network` 多入口。
4. SMART：`secm-datasource::disk` 自带 SMART（IOCTL）与 LHM Storage sensors 并存。
5. 电源/电池：`secm-datasource::power`（Win32 Power API）与 LHM Battery。

## 7. ADR-0001 Gate 自检

- [x] audit/baseline.md（本文件）
- [x] audit/litemonitor-files.md
- [x] audit/target-files.md
- [x] audit/dependencies.md

结论：Gate 满足，进入 ADR-0002。
