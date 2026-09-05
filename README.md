# SysEnv Console Manager (SECM) — GPUI 版

> Windows 10/11 系统环境管理桌面工具 — **纯 Rust + GPUI**（Zed UI 框架）
> 硬件监控 / 清理优化 / 网络诊断 / 系统设置 / 环境检测 一站式平台

[![Version](https://img.shields.io/badge/version-v2.0.0-blue)](CHANGELOG.md)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

> ⚠️ **v2.0.0 为纯 Rust + GPUI 完整重构**。历史 Tauri 2 + React 版本（v1.x）见
> 原仓库：https://github.com/youridol/sysenv-console-manager

## 技术栈

| 层 | 技术 |
|----|------|
| UI | GPUI 0.2（Zed，Apache-2.0，Windows DirectX/blade 渲染） |
| 语言 | Rust（edition 2021，workspace） |
| 采集 | secm-datasource（Win32/注册表/HTTP 纯 Rust 采集） |
| 温度/功耗 | LHM .NET sidecar（MPL-2.0 进程隔离，45980 端口）；WinRing0/ACPI 降级链为后续版本计划 |
| 系统托盘 | tray-icon（后台线程 + win32 消息泵） |

## 项目结构

```
sysenv-console-manager-gpui/
├── crates/
│   ├── secm-datasource/   # 纯 Rust 采集层（注册表/服务/电源/网络/DNS/磁盘…）
│   ├── secm-core/         # 业务逻辑（采集编排/系统操作，无 UI 依赖）
│   └── secm-app/          # GPUI 桌面应用（UI + 装配 + main）
├── sidecar-lhm/           # LHM 温度 sidecar（.NET 8 源码 + MPL-2.0 许可，GPL-2.0 PawnIO 隔离边界）
├── third_party/           # 第三方驱动（WinRing0/PawnIO）源码与许可
├── scripts/publish.ps1    # 一键发布（Rust + sidecar + 许可 → dist/）
├── docs/adr/              # 架构决策记录（重构全案）
├── docs/spec/             # 功能基准（验收依据）
└── LICENSE                # MIT
```

> LHM 温度 sidecar 以进程隔离方式运行（MPL-2.0 边界）；发布时由 scripts/publish.ps1
> dotnet publish 出 `lhm/publish/LhmSidecar.exe`，主程序自动从 exe 同目录定位
> （也可用环境变量 `SECM_LHM_SIDECAR` 指定目录）。第三方驱动见 third_party/ 内 README。

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
