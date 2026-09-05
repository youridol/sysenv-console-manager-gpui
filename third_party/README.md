# third_party —— 第三方内核驱动（随 SECM 分发的受控依赖）

> 自研内核驱动 hwmon-driver 已从项目中移除（2026 开源整理），温度/功耗数据源收敛为
> LHM sidecar（LibreHardwareMonitor + PawnIO）→ WinRing0 → ACPI。third_party 仅保留
> 两个第三方驱动依赖。随包分发许可文本见同目录 `OpenLibSys-LICENSE.txt`（WinRing0）与
> `PawnIO/` 内 COPYING（GPL-2.0）。

## WinRing0x64.sys

| 项 | 值 |
|---|---|
| 文件 | `WinRing0x64.sys`（本目录） |
| 版本 | 1.2.0.5 |
| 来源 | 本机 LHM（Libre Hardware Monitor）安装实例提取（`C:\Windows\System32\drivers\WinRing0x64.sys`），实为 OpenLibSys **hotproject 重编译变体**（端口 IO 协议已按此变体 IOCTL 确认：`IOCTL_READ_PORT_DWORD=0x9c4060d4` / `IOCTL_WRITE_PORT_DWORD=0x9c40a0e0`） |
| 签名 | GlobalSign 商业签名，CN=Noriyuki MIYAZAKI（CrystalDiskInfo 作者），`Get-AuthenticodeSignature` = Valid |
| SHA-256 | `11bd2c9f9e2397c9a16e0990e4ed2cf0679498fe0fd418a3dfdac60b5c160ee5` |
| 用途 | WinRing0 温度/功耗通道：LHM sidecar 不可用时的**预留回退**。注意：v2.0.0 GPUI 版**不含任何 WinRing0 使用代码**（原 Tauri 仓 `src-tauri/src/driver_install/winring0.rs` 未迁入本仓，驱动部署引导不存在），当前仅 LHM sidecar（内部经 PawnIO/WinRing0 设备）消费驱动；本目录资产为后续版本迁移预留 |

### ⚠️ 安全风险声明

1. **历史 CVE**：WinRing0 系列存在任意端口 IO 漏洞（CVE-2020-14979 等）。本仓库
   **不含任何加载/调用该驱动的代码**，仅随包保留文件与许可；未来若迁移使用，
   将仅作为**受限数据通道**（端口 IO / MSR 读取最小面），不做任意地址读写。
2. **Defender 现状（2026-09 核实）**：Microsoft Defender 已将 WinRing0 系列列为
   `VulnerableDriver:WinNT/Winring0`，HVCI（内存完整性）开启的机器会直接拦截加载；
   这不影响本仓现状（无使用代码），但意味着未来迁移时需优先 PawnIO 通道。
3. **微软 Vulnerable Driver Blocklist**：自带版本签名早于名单生效；若未来被封锁，
   LHM sidecar 探活失败即降级为无温度数据，不影响系统稳定。

### 校验方法

```bat
certutil -hashfile WinRing0x64.sys SHA256
rem 期望输出: 11bd2c9f9e2397c9a16e0990e4ed2cf0679498fe0fd418a3dfdac60b5c160ee5
```
