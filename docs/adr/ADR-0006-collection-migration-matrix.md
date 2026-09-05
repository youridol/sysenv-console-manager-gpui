# ADR-0006：数据采集层迁移矩阵

- 状态：提议
- 日期：2026-09-05
- 关联：ADR-0001、ADR-0002

## 背景

数据采集分为两层：
1. `datasource` crate（P1-P15 原始采集：注册表/服务/电源/网络/DNS/HTTP/激活/CPU 频率/磁盘/SMART/PDH）— 纯 Rust，**零改动迁入**。
2. `src-tauri/src` 业务模块（编排 + 命令 + 日志 + 驱动部署）— 去 tauri 化后迁入 secm-core。

## 迁移矩阵

| 源模块 | 职责 | 迁移方式 | 说明 |
|--------|------|----------|------|
| datasource/*（12 文件） | 原始采集 | 原样复制至 `crates/secm-datasource` | 无 Tauri 依赖；单测（48+）直接延续 |
| sensor.rs | 快照编排（1s CPU/GPU/内存/磁盘 + LHM 温度 + diag） | 重构为 `secm-core::sensor` 纯逻辑 + 后台循环 | 去掉 tauri AppHandle/事件，返回快照结构 |
| lhm.rs | sidecar 生命周期 + HTTP 轮询 | 迁 `secm-core::lhm`，去 tauri | ADR-0005 |
| hardware.rs | HTTP 传感器服务（17860） | **废弃**（原前端已不调用；纯 Rust 版无需本地 HTTP 桥） | 删除 tiny_http 面 |
| driver_install/{mod,pawnio,winring0,acpi}.rs | SCM 驱动部署 + WinRing0 温度/功耗 | 迁 `secm-core::driver_install` | 去 tauri 资源定位 → 相对 exe 定位 |
| cleanup.rs | 缓存清理/进程/优先级/DNS 刷新 | 迁入 secm-core | Command/注册表逻辑保留 |
| network.rs | ping/traceroute/nslookup/NAT/iperf3 | 迁入 secm-core | surge-ping/trust-dns 保留；tokio 依赖按需收敛 |
| net_config.rs + dhcp_probe.rs | netsh 配置 + DHCP 探测 | 迁入 secm-core | netsh argv 调用保留 |
| settings.rs / hpt.rs / nvidia_drs.rs | 系统设置/HAGS/电源/调度/NVIDIA DRS | 迁入 secm-core | 注册表/命令逻辑保留 |
| environment.rs / game_env.rs / sysinfo.rs | DX/VC++/AI 工具/系统信息 | 迁入 secm-core | 外部命令（npm/where/sc）保留白名单校验 |
| net_stats.rs / ip_info.rs / disk_info.rs / ds_util.rs | 网络速率/IP/磁盘详情/错误映射 | 迁入 secm-core | 纯逻辑，去 tauri |
| log.rs / debug.rs / temp_data.rs / cancel.rs / proc_util.rs | 日志/诊断/取消/外部命令 | 迁入 secm-core | 保留，UI 经订阅消费 |
| tray.rs | 系统托盘 | **重写**（GPUI 无托盘） | ADR-0008 |
| commands.rs（80 命令） | IPC 外壳 | **删除**（命令变直接函数调用） | ADR-0007 |
| main.rs / lib.rs | tauri 装配 | **重写**为 gpui main | ADR-0002/0003 |

## 删除清单（旧技术栈）
- 前端整体：src/（React/TS/HTML/CSS）+ vite + tsc + eslint + tailwind + recharts + shadcn + radix + lucide + sonner + react-router
- Tauri 面：tauri* / plugin-* / tauri.conf.json / capabilities / gen / icons 生成物（图标资源本身保留至 assets）
- Node 面：package.json / node_modules / vite.config / index.html
- 平台特定废弃面：hardware.rs 的 tiny_http 服务、cdp 相关、WebView CSP/remote-debug

## 后果
- 采集层 ~90% 逻辑直接复用，重写集中在 UI 与装配。
- 删除面缩小攻击面并去 Node/WebView 运行时。
