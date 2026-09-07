# Windows 非管理员策略与真机验证矩阵（ADR-0007）

验证环境：Windows 11 Pro 25H2（Build 26200）/ AMD Ryzen 7 7800X3D / MSI B650M (Nuvoton NCT6687D) / RTX 2080 Ti / NVMe+SATA SSD+HDD / 台式机（无电池）。
验证工具：`cargo run -p secm-core --example hw_verify`（统一快照帧打印）+ sidecar 直连 HTTP 探测。
验证方式：管理员会话直跑；普通用户 = `runas /trustlevel:0x20000` 降权 token；UAC 取消 = 普通权限运行 sidecar（`--elevated-child` 跳过自提权，等价用户拒绝 UAC 后的状态）；LHM 不可用 = `SECM_DISABLE_LHM=1`。

## 权限模型（落地决策）

```text
主程序 secm-app：asInvoker（普通用户）
  ├─ USER_SAFE 域（PDH/IPHLPAPI/GlobalMemoryStatusEx/GetSystemPowerStatus）→ 直接采集
  └─ LHM 域（温度/功耗/电压/风扇/GPU/主板/磁盘温度/电池）
        → sidecar-lhm 启动时 runas 自提权（用户授权）
        ├─ 用户允许 UAC → 提权 sidecar 全量采集
        ├─ 用户取消 UAC → sidecar 普通权限运行 → available:false + 明确错误（零伪造值）
        └─ sidecar 不存在 → ensure_running 诊断 + LHM 域全部 unavailable
```

与 LiteMonitor 的差异（如实声明）：LiteMonitor 主程序 `requireAdministrator` + PawnIO 安装器 `runas`；SECM 主程序保持普通权限，高权限能力隔离在 sidecar 单独提权。

## 真机验证矩阵（2026-07-15 实测）

| 场景 | CPU Load/Clock | CPU Temp/Power/Volt | MEM | DISK IO/Act/Cap | DISK Temp | NET | GPU | MOBO/Fan | BAT | 结论 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Win11 管理员 + sidecar | ✅ 20.1%[perfcounter] / 4426MHz | ✅ 66.8°C / 37.4W / 0.6V [lhm] | ✅ 31.6GB/83% | ✅ [perfcounter] | ✅ 7 盘 36–50°C [lhm] | ✅ IP/TCP/速率 | ✅ 2080Ti 全域 | ✅ sysTemp 54.5°C / CPU 风扇 1652 / Case 1185 | n/a（台式机无电池，非错误） | 全绿 |
| Win11 普通用户（降权 token）+ sidecar 禁用 | ✅ 19.1% / 4411MHz | ✅ unavailable + 诊断 | ✅ | ✅ | unavailable | ✅ IPv4/TCP/速率 | unavailable | unavailable | n/a | USER_SAFE 域全绿，LHM 域诚实不可用 |
| UAC 取消（普通权限 sidecar） | ✅ | ✅ `available:false` + "LHM 需要管理员权限读取传感器（PawnIO 设备仅 SYSTEM/Administrators 可访问）…" | ✅ | ✅ | unavailable | ✅ | `[]` 空 | null | null | 零伪造值，软件继续运行普通用户域 |
| LHM 不可用（SECM_DISABLE_LHM=1） | ✅ | ✅ unavailable + 诊断串 | ✅ | ✅ | unavailable | ✅ | `[]` | 无 | None | 降级语义正确 |
| Win10 普通用户 / 管理员 | — | — | — | — | — | — | — | — | — | **未验证**（本机仅 Win11 25H2；如实记录，不宣称） |

## 指标权限结论（发布说明用）

- **普通用户稳定可用（USER_SAFE）**：CPU 负载/频率、内存总量/占用、磁盘 IO 速率/活动时间/容量/已用、网络 IP/速率/TCP 连接数/链路速度、AC 状态。
- **普通用户部分可用（USER_BEST_EFFORT，依赖硬件/驱动/sidecar 提权）**：CPU 温度/功耗/电压、GPU 全域、主板温度/风扇/水泵、磁盘温度、电池全域、BIOS/主板型号。普通用户允许 sidecar UAC 提权后可用；取消 UAC 或无 PawnIO/WinRing0 驱动 → 结构化 unavailable + 真实原因。
- **必须管理员/受信任驱动（ADMIN_REQUIRED）**：SuperIO/SMBus/Msr 类底层访问（LHM 经 PawnIO 2.2.0 WHQL 或 WinRing0 GlobalSign 商业签名）。
- **完全不可用**：FPS（PresentMon+ETW 需管理员，未迁移）、Display/显示器信息（LiteMonitor 亦未实现）。

## 特检项

| 项 | 结论 |
| --- | --- |
| MSR / SuperIO / SMBus | 经 PawnIO（主）或 WinRing0（回退）内核通道，sidecar 提权后可用；普通用户进程直接访问被 DACL 拒绝（ERROR_ACCESS_DENIED → 明确诊断） |
| PawnIO | 双后端预检（CreateFileW \\.\PawnIO → \\.\WinRing0_1_2_0），错误码映射可读引导 |
| NVAPI/AMD API | LHM 用户态加载显卡驱动 DLL，GPU 域在管理员 sidecar 内实测可用 |
| SMART | 独立 IOCTL 域（hardware.rs 按需读取），USB/部分盘 NeedsAdmin 诚实标注 |
| 温度/风扇/电压/功耗 | 管理员 sidecar 下全部实测出值；普通用户下随 sidecar 提权状态 |
| HDD 休眠保护 | Storage 30s 慢速刷新（LiteMonitor 深睡策略的简化等价，差异已记录） |
