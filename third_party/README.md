# third_party —— 第三方内核驱动（随 SECM 分发的受控依赖）

> 自研内核驱动 hwmon-driver 已从项目中移除（2026 开源整理）。**v3.0.0 起硬件采集
> 为纯 Rust 原生（NVML/DXGI/PDH/Win32/IOCTL/WMI），全项目无 ring0 驱动消费者**——
> 原 LHM .NET sidecar（经 PawnIO/WinRing0 设备）已随 v3.0.0 去 HTTP 化改造删除，
> 本目录两个第三方驱动依赖降级为**预留资产**（未来可选 ring0 后端）。随包分发许可
> 文本见同目录 `OpenLibSys-LICENSE.txt`（WinRing0）与 `PawnIO/` 内 COPYING（GPL-2.0）。

## WinRing0x64.sys

| 项 | 值 |
|---|---|
| 文件 | `WinRing0x64.sys`（本目录） |
| 版本 | 1.2.0.5 |
| 来源 | 本机 LHM（Libre Hardware Monitor）安装实例提取（`C:\Windows\System32\drivers\WinRing0x64.sys`），实为 OpenLibSys **hotproject 重编译变体**（端口 IO 协议已按此变体 IOCTL 确认：`IOCTL_READ_PORT_DWORD=0x9c4060d4` / `IOCTL_WRITE_PORT_DWORD=0x9c40a0e0`） |
| 签名 | GlobalSign 商业签名，CN=Noriyuki MIYAZAKI（CrystalDiskInfo 作者），`Get-AuthenticodeSignature` = Valid |
| SHA-256 | `11bd2c9f9e2397c9a16e0990e4ed2cf0679498fe0fd418a3dfdac60b5c160ee5` |
| 用途 | WinRing0 温度/功耗通道：**v3.0.0 起无任何消费者**（原生采集不经 ring0）；本目录资产为未来可选 ring0 后端预留 |

### ⚠️ 安全风险声明

1. **历史 CVE**：WinRing0 系列存在任意端口 IO 漏洞（CVE-2020-14979 等）。本仓库
   **不含任何加载/调用该驱动的代码**，仅随包保留文件与许可；未来若迁移使用，
   将仅作为**受限数据通道**（端口 IO / MSR 读取最小面），不做任意地址读写。
2. **Defender 现状（2026-09 核实）**：Microsoft Defender 已将 WinRing0 系列列为
   `VulnerableDriver:WinNT/Winring0`，HVCI（内存完整性）开启的机器会直接拦截加载；
   这不影响本仓现状（无使用代码），但意味着未来迁移时需优先 PawnIO 通道。
3. **微软 Vulnerable Driver Blocklist**：自带版本签名早于名单生效；若未来被封锁，
   LHM sidecar（v2 时代）探活失败即降级为无温度数据；v3.0.0 起原生采集不使用驱动，未来 ring0 后端若启用同理。

### 校验方法

```bat
certutil -hashfile WinRing0x64.sys SHA256
rem 期望输出: 11bd2c9f9e2397c9a16e0990e4ed2cf0679498fe0fd418a3dfdac60b5c160ee5
```
