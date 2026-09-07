# LiteMonitor 全链路调用图（ADR-0002）

来源：`Y:\_refs\LiteMonitor` @ `169e90a2ccaa68ca29d0b549d10c6c59634c1b49`（master），以下全部结论来自逐文件实读，无推测。

## 1. 总体链路

```text
Program.Main
  ├─ 单实例 Mutex（Global\LiteMonitor_SingleInstance_<path>）
  ├─ DriverInstaller.CheckPawnIOBeforeHardware()   ← 启动前检查/安装 PawnIO（runas UAC）
  └─ Application.Run(MainForm)
        └─ HardwareMonitor（单例，构造即异步启动）
             ├─ Computer (LHM)：CPU/GPU/Memory/Network/Storage/Motherboard/Battery = true；PSU=false；
             │   Controller 仅当配置了 CPU.Fan / CPU.Pump / CASE.Fan 时开启（ShouldEnableController）
             ├─ PerformanceCounterManager.InitializeAsync()   ← 后台预热
             ├─ lock{ Computer.Open() → WarmUpMotherboard → WarmUpBattery → SensorMap.Rebuild
             │        → ValueProvider.PreCacheAllSensors }
             ├─ 3s 后 DisableSensorHistory（反射 ValuesTimeWindow=0，禁用 LHM 24h 历史）
             └─ UI 定时器（TaskbarForm Timer，Interval = max(RefreshMs, 60)）
                  └─ UpdateAll()   ← 统一心跳，约 1s 一拍
                       ├─ UpdateTiming：timeDelta（>5s 归 0 防休眠突刺）+ 真实秒累加器
                       ├─ CheckUpdateRequirements：按配置算 NeedXxx；WebServer 开启则 ForceAll；
                       │   useCounter(配置开 && PCM.IsInitialized) 时 MEM 不轮询 LHM
                       ├─ ValueProvider.OnUpdateTickStarted()：清 _tickCache；检测偏好变更→Rebuild/PreCache
                       ├─ lock { SensorMap.EnsureFresh(10min 兜底) → 按 HardwareType 分派 Update }
                       └─ needsReload（GPU Update 抛异常）→ 2s 后异步 ReloadComputerSafe
```

## 2. UpdateAll 分派规则（每拍）

| HardwareType | 更新条件 | 说明 |
| --- | --- | --- |
| Cpu | NeedCpu（配置任一 CPU 项或 ForceAll） | `hw.Update()` |
| GpuNvidia/GpuAmd/GpuIntel | NeedGpu，且只更新 `SensorMap.CachedGpu`（多卡时其他卡跳过） | try/catch；异常置 needsReload |
| Memory | NeedMem && **!useCounter**（计数器可用时跳过 LHM 内存轮询） | `hw.Update()` |
| Battery | NeedBat | `hw.Update()` |
| Network | NeedNet → `NetworkManager.ProcessUpdate(timeDelta, isSlowScanTick)` | 目标网卡每拍 Update；虚拟/启动期跳过；其他网卡 3s 慢扫 |
| Storage | NeedDisk → `DiskManager.ProcessUpdate(isSlowScanTick, needDiskBgScan)` | 目标盘每拍；后台盘按活跃/冷却/深睡退避 |
| Motherboard/SuperIO/Cooler | NeedMobo 且 **isSlowScanTick（3s）** | `UpdateWithSubHardware` 递归子硬件（SuperIO 强制慢速） |

`isSlowScanTick = _secondsCounter % 3 == 0`（3 秒）；`needDiskBgScan = _secondsCounter % 10 == 0`（10 秒）。

## 3. 取值链路（UI → 底层）

```text
UI: MetricUtils/MetricItem → HardwareMonitor.Instance.Get(key)
      ├─ _isOpening → ValueProvider.GetStartupValue(key)
      │     └─ _lastValidMap → PerfCounter（CPU.Load/CPU.Clock/MEM.Load/DISK.Read/Write/Activity）
      └─ ValueProvider.GetValue(key)
            ├─ Monitor.TryEnter(_lock, 10ms) 失败 → _lastValidMap（防 UI 闪烁）
            ├─ _tickCache 命中 → 直接返回（同帧去重）
            ├─ switch(key)：
            │    CPU.Load     : PerfCounter("% Processor Utility") → LHM "CPU.Load"(Total) → ComponentProcessor 核心平均 → 0f(缺陷)
            │    CPU.Temp     : ComponentProcessor(核心最大值) → SensorMap["CPU.Temp"] → 0f(缺陷)
            │    MEM.Load     : PerfCounter(Available MBytes+GlobalMemoryStatusEx) → (Used+Available)计算 → LHM Load sensor
            │    GPU.VRAM     : (Used/Total)计算 → LHM "GPU.VRAM.Load"
            │    GPU.*        : ComponentProcessor.GetCompositeValue（Clock/Power 从 CachedGpu 现场筛传感器 + 熔断阈值）
            │    CPU.Clock    : PerfCounter(% Processor Performance × ~MHz) → CpuCoreCache 核心平均（Zen5 Bus Speed 修正）
            │    CPU.Power    : SensorMap["CPU.Power"]（>600W 熔断）
            │    CPU.Fan/Pump/CASE.Fan/GPU.Fan : SensorMap（FanMapper 匹配）→ UpdateMaxRecord
            │    MOBO.Temp    : ReadMoboTemperature（硬上限 Auto 95 / Manual 125；异常→lastValid）
            │    BAT.*        : BatteryService（LHM sensor + AcOnline 符号修正）
            │    NET.*        : SensorMap → NetworkManager.GetBestValue（缓存→记忆→全盘扫描评分）
            │    DISK.*       : PerfCounter(无指定盘时) → SensorMap → DiskManager.GetBestValue；DISK.C.Used→DriveInfo
            │    FPS          : FpsCounter（PresentMon sidecar，管理员权限）
            │    DATA.DayUp/Down : TrafficLogger（流量统计）
            └─ 通用兜底：SensorMap 值有效→写 _lastValidMap；无效→返回 lastValid
```

三级缓存：

| 缓存 | 生命周期 | 位置 |
| --- | --- | --- |
| `_tickCache` | 每拍开始清空（OnUpdateTickStarted） | ValueProvider |
| `_manualSensorCache`（ISensor 对象缓存） | PreCacheAllSensors 重建（启动/重载/偏好变更） | ValueProvider |
| `_lastValidMap` | 持续更新，重载时保留 | HardwareMonitor（共享引用） |

## 4. SensorMap.Rebuild（映射构建）

1. 全部硬件按 `HardwareRules.GetHwPriority` 排序（Nvidia=0；AMD 具体型号=0 / "AMD Radeon(TM) Graphics"核显=2；Intel Arc 独显=0 / Arc 核显=1 / Iris=2 / UHD=3；"Basic Render"=100 垫底）。
2. GPU：用户 `PreferredGpu`（Identifier 优先，兼容旧名）仍存在时只映射该卡；`GPU.Fan` = 第一个 Fan sensor → fallback 第一个 Control sensor。
3. CPU：收集 (Clock, Load) 核心对（Load 名 EndsWith Clock 名，兼容 AMD "CPU Core #1" vs "Core #1"）；缓存 Bus Speed sensor（Zen5 频率修正 100/bus，系数限 2~10）。
4. 普通传感器：`SensorMatcher.Match` 产出标准 key；冲突时强卡优先（priority 更小禁止覆盖）；同优先级 Vendor（非 D3D）胜 D3D。
5. MOBO.Temp 智能策略：System > Motherboard > Chipset/PCH > 合理范围(15–68℃)最大值 > 宽范围(0–95℃)最大值。
6. 风扇匹配：仅当配置了 CPU.Fan/CPU.Pump/CASE.Fan 才跑 `FanMapper.ScanAndMapFans`（见 §6）。
7. 原子交换（lock）；`EnsureFresh` 仅 10 分钟兜底重建。

## 5. SensorMatcher 标准键（完整规则，来自实读）

| key | 硬件 | 条件 |
| --- | --- | --- |
| CPU.Load | Cpu | Load 含 total/package |
| CPU.Temp | Cpu | Temp 含 package/average/tctl/tdie/ccd/cores；或 (cpu/core 且排除 soc/vrm/fan/pump/liquid/coolant/distance) |
| CPU.Power | Cpu | Power 含 package/cores |
| CPU.Voltage | Cpu | Voltage 含 core/cpu/vcore/vid 且排除 soc/gt/sa/aux |
| GPU.Load | GPU | Load 含 core / "d3d 3d" |
| GPU.Temp | GPU | Temp 含 core/hot spot/soc/vr |
| GPU.VRAM.Used/Total | GPU | SmallData；核显(Intel 非 Arc 独显)优先 shared（fallback 通用 memory）；独显用 dedicated/通用且禁止 shared |
| GPU.VRAM.Load | GPU | Load 含 memory |
| MEM.Load | Memory | Load 含 memory 或名 == "Load"；硬件名含 virtual 整体排除 |
| MEM.Used / MEM.Available | Memory | Data/SmallData 含 used / available 或 free |
| BAT.Percent | Battery | Level（优先含 Charge；排除 Degradation/Wear） |
| BAT.Power / BAT.Voltage / BAT.Current | Battery | Power / Voltage / Current 类型 |
| NET/DISK/BAT 非标准键 | — | NetworkManager/DiskManager/BatteryService 关键词匹配（见 §7/§8） |

## 6. FanMapper（风扇/水泵智能匹配）

- 收集：排除 GPU/CPU/Storage/Memory/Network 类型后全树扫描 Fan 传感器（先 `hw.Update()`），底噪过滤 >200 RPM。
- 优先级：用户 PreferredXxx（格式 `"[HardwareName] SensorName"` 或裸 sensor 名）> Cooler 硬件（Cooler 类型或名含 Kraken/Corsair/Liquid/AIO/Cooler）> 名含 CPU > 第一个。
- Pump：Cooler 硬件名含 Pump/Speed → 其他 Cooler → 名含 Pump/Water/AIO → 转速 >3000 最高者。
- CaseFan：剩余中名含 Rear > Chassis > Sys > Case → 剩余中转速最低者（多扇时最高者给 Pump）。

## 7. NetworkManager（网络双来源决策）

- 显示值 NET.Up/NET.Down：LHM Throughput 传感器。`GetBestValue`：运行时缓存（存活检查）→ `cfg.LastAutoNetwork` 记忆 → 全盘扫描（up+down 流量评分，虚拟网卡 -1e9 罚分）→ lastValidMap。
- 上传/下传关键词：up = upload/up/sent/send/tx/transmit；down = download/down/received/receive/rx。
- 虚拟网卡关键词：virtual/vmware/hyper-v/hyper v/vbox/loopback/tunnel/tap/tun/bluetooth/zerotier/tailscale/wan miniport。
- 流量统计（TrafficLogger）：LHM 速率×seconds 估算 vs Native `NetworkInterface.GetIPStatistics()` 增量；native 有效优先；native 增量=0 但 LHM>51200B 判定匹配错误回退 LHM 并解绑；可扣 SMB（PerfCounter SMB Client/Server×1.2 开销系数）；单次增量 >10GB 丢弃（安全阀）。
- Native 匹配：名称精确 → Description 精确 → 分词模糊（>2 个 token 且 >60%）；失败 10s 重试节流。
- IP：30s 静态缓存；`NetworkChange.NetworkAddressChanged` 事件 → 清缓存+重置适配器匹配；策略A 已匹配适配器 → 策略B 全系统遍历（Up 且非 Loopback 非虚拟）；IPv4 优先 192.168.*，排除 APIPA 169.254.*。
- 每网卡独立 `NetworkState`（NativeAdapter/LastNative*/CachedUp/DownSensor/LastMatchAttempt）。

## 8. DiskManager（磁盘三态退避）

- ProcessUpdate：PreferredDisk 锁定；目标盘每拍 Update；后台盘：活跃(<1min)→3s 慢扫；冷却(1–5min)→10s 后台扫描；深睡(>5min)→不更新；Throughput>1KB/s 重置活跃计时。
- GetBestValue：运行时缓存（存活检查）→ 逻辑盘 key（`DISK.<盘符>.Used` → DriveInfo：Used% = 100 − Free/Total×100，DriveInfo 静态缓存）→ `cfg.LastAutoDisk` 记忆 → 全盘扫描（read+write 评分，系统盘 +1e9）→ lastValidMap。
- DISK.Temp：`FindBestTempSensor`（排除名含 warning/critical；优先名恰为 "Temperature"）。
- Read/Write 关键词：read / write；Activity 仅来自 PerfCounter `% Disk Time`。

## 9. PerformanceCounterManager（Windows 计数器域）

| 计数器 | 类别\计数器\实例 | 回退 |
| --- | --- | --- |
| CPU Load | Processor Information\% Processor Utility\_Total | → 同类别 % Processor Time → Processor\% Processor Time |
| CPU Freq | Processor Information\% Processor Performance\_Total | → Processor 类别同名；频率 = 基准 MHz × percent/100 |
| 基准频率 | 注册表 HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0\~MHz | 失败=2500（历史默认值，ADR-0004 判定为缺陷，不迁移） |
| 内存 | Memory\Available MBytes + GlobalMemoryStatusEx(ullTotalPhys) | 总量失败=16GB（缺陷，不迁移；真实失败应返回 unavailable） |
| 磁盘 | PhysicalDisk\Disk Read/Write Bytes/sec\_Total, % Disk Time\_Total | 无 |
| SMB | SMB Client/Server Shares Read/Write(Received/Sent) Bytes/sec\_Total | 类别缺失→null（禁用扣除） |
| UpTime | System\System Up Time | 无 |

- `CreateCounter` 先检查 `PerformanceCounterCategory.Exists`；`SafeRead` 异常→null；初始化后逐个预热一次 NextValue（计数器第一次采样返回 0 的问题）；任一异常→IsInitialized=false→上层自动回退 LHM。

## 10. BatteryService（电池）

- 全部取自 LHM Battery 硬件传感器（SensorMap 映射的 BAT.Percent/Power/Voltage/Current）。
- BAT.Power/BAT.Current 符号修正：`MetricUtils.GetPowerStatus()`（WinForms `SystemInformation.PowerStatus`，3s 节流缓存）→ AcOnline=充电正数 / 放电强制负绝对值。
- 模拟电池代码存在但 `simulateBattery=false` 硬编码关闭（UI 测试遗留，不迁移）。

## 11. DriverInstaller（驱动/权限模型 — ADR-0007 关键事实）

- 硬件底层访问依赖 **PawnIO 驱动 ≥ 2.2.0.0**（LHM 0.9.6 的内核访问通道；WinRing0 driver.zip 为旧通道资产）。
- 检查：注册表 `SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO` 读版本；Missing/Outdated → 启动时对话框 → `RunPawnIOInstaller`（`ProcessStartInfo.Verb = "runas"` UAC 提权，静默安装，失败转手动）。
- 旧版升级需先卸载+重启；运行中不卸载驱动（防运行态破坏）。
- 下载源：gitee/litemonitor.cn/github 的 driver.zip（校验失败拒绝安装）。
- 结论：LiteMonitor 完整 CPU 温度/电压/功耗/风扇能力 = requireAdministrator 主程序 + PawnIO 驱动；非"普通用户无特权"模型。

## 12. FpsCounter（FPS — 迁移范围判定项）

- 技术路径：PresentMon sidecar 进程（`assets/LiteMonitorFPS.exe`）+ ETW 会话（logman 清理），**需要管理员权限**（启动前显式检查）。
- 多层平滑算法（中位数/抖动过滤）。缺失时经 DriverInstaller 自动下载。
- 属独立采集域（GPU 帧率），非 LHM/PerfCounter 域；侧载进程 + ETW + 管理员 = 普通用户下不可用，迁移评估归 ADR-0004/0006 Phase 6。

## 13. 生命周期 / 并发模型

- 单一 `_lock`（HardwareMonitor 持有，ValueProvider 共享引用）；UI 取值 TryEnter 10ms 超时；UpdateAll 全程 lock；重载异步（Task.Run）+ `_isReloading` 防并发重入。
- ReloadComputerSafe：清 Network/Disk/SensorMap/ValueProvider/HardwareScanner 缓存 → Computer.Accept(空 Visitor)+Close+**手动 Hardware.Clear()**（LHM Close 不清列表，不清会重复）→ Open → WarmUp → DisableSensorHistory → Rebuild → PreCache。
- Dispose：lock 内 Close；释放 ValueProvider/PCM/FpsCounter；清 Network/Disk 缓存。
- 已知历史缺陷（迁移时不照搬）：顶层 `catch {}` 吞异常（UpdateAll/Reload/初始化多处）；CPU.Load/CPU.Temp 失败→0f；PerfCounter 基准频率 2500/内存 16GB 静态兜底；反射禁用历史记录。

## 14. UI 消费路径

- TaskbarForm：WinForms Timer（max(RefreshMs,60)ms）→ Tick → UpdateAll + 逐指标 Get(key)；MetricUtils 负责类型/格式化/状态阈值。
- MetricItem：key/label/value/unit/category 模型；MetricLabelResolver：SensorMap 标签解析。
- WebServer（LiteWebServer）：ForceAll 语义来源（网页端需要全量刷新）。
- HardwareHistoryLogger：历史 CSV（独立功能）。
