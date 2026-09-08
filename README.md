# SysEnv Console Manager (SECM) — GPUI 版

> Windows 10/11 系统环境管理桌面工具 — **纯 Rust + GPUI**（Zed UI 框架）
> 硬件监控 / 清理优化 / 网络诊断 / 系统设置 / 环境检测 一站式平台

[![Version](https://img.shields.io/badge/version-v3.0.0-blue)](CHANGELOG.md)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

> ⚠️ **v2.0.0 为纯 Rust + GPUI 完整重构**。历史 Tauri 2 + React 版本（v1.x）见
> 原仓库：https://github.com/youridol/sysenv-console-manager
>
> ⚠️ **v3.0.0 硬件采集去 HTTP 化**：Rust 原生采集 + GPUI 直接消费，删除
> LHM .NET sidecar 与全部本地 HTTP 链路（原 45980 端口），进程内 Rust 类型直传。

## 技术栈

| 层 | 技术 |
|----|------|
| UI | GPUI 0.2（Zed，Apache-2.0，Windows DirectX/blade 渲染） |
| 语言 | Rust（edition 2021，workspace） |
| 采集 | secm-datasource（Win32/PDH/IOCTL/WMI/NVML/DXGI 纯 Rust 原生采集，零 HTTP 零提权） |
| GPU | NVML（NVIDIA 用户态实时指标）+ DXGI（全厂商枚举） |
| 温度 | NVMe 健康日志（IOCTL，用户态）；CPU/SATA 温度需管理员 ring0 → 如实 unavailable |
| 系统托盘 | tray-icon（后台线程 + win32 消息泵） |

## 项目结构

```
sysenv-console-manager-gpui/
├── crates/
│   ├── secm-datasource/   # 纯 Rust 原生采集层（注册表/服务/电源/网络/DNS/磁盘/GPU/内存…）
│   ├── secm-core/         # 业务逻辑（采集编排/系统操作，无 UI 依赖；统一 SensorSnapshot）
│   └── secm-app/          # GPUI 桌面应用（UI + 装配 + main）
├── third_party/           # 第三方驱动（WinRing0/PawnIO）源码与许可（v3 起无消费者，预留）
├── scripts/               # 构建/发布脚本
├── docs/adr/              # 架构决策记录（重构全案）
└── LICENSE                # MIT
```

> v3.0.0 硬件数据全部进程内 Rust 直调（Sensor Manager → Native Backend），
> 无 HTTP/localhost/JSON 反序列化链路，无 sidecar 子进程，普通用户可运行；
> ring0 专属指标（CPU 温度/功耗/电压、主板 SuperIO）如实 unavailable。

## 页面

11 个页面全部以 GPUI 实现：硬件信息（Dashboard）、清理优化、网络诊断、网络配置、
系统设置、服务管理、环境检测、AI 环境、硬件检测、调试日志、关于。

## 构建

```bash
cargo build            # debug
cargo test             # workspace 全部单测
cargo run -p secm-app  # 运行应用
cargo build --release  # 发布构建（产物 target/release/secm-app.exe）
```

## 许可证

MIT License — Copyright (c) 2026 SECM Team
