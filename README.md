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
| 温度/功耗 | LHM .NET sidecar（MPL-2.0 进程隔离）→ WinRing0 → ACPI |
| 系统托盘 | tray-icon（后台线程 + win32 消息泵） |

## 项目结构

```
sysenv-console-manager-gpui/
├── crates/
│   ├── secm-datasource/   # 纯 Rust 采集层（注册表/服务/电源/网络/DNS/磁盘…）
│   ├── secm-core/         # 业务逻辑（采集编排/系统操作，无 UI 依赖）
│   └── secm-app/          # GPUI 桌面应用（UI + 装配 + main）
├── docs/adr/              # 架构决策记录（重构全案）
├── docs/spec/             # 功能基准（验收依据）
├── sidecar-lhm/           # LHM sidecar（.NET 源码，随包分发）
└── third_party/           # 第三方驱动依赖 + 许可
```

## 构建

```bash
cargo build            # debug
cargo test             # workspace 全部单测
cargo run -p secm-app  # 运行应用
cargo build --release  # 发布构建
```

## 许可证

MIT License — Copyright (c) 2026 SECM Team
