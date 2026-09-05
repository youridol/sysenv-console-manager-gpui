# ADR-0001：技术栈与依赖基线

- 状态：提议
- 日期：2026-09-05
- 关联：ADR-0002（项目结构）、ADR-0006（采集层迁移）

## 背景

SECM 现为 Tauri 2 + React 19 应用。目标：移除全部旧技术栈，交付**纯 Rust + GPUI** 桌面应用。

本机已验证（2026-09-05 spike）：
- `gpui = "0.2"`（crates.io 最新 0.2.2，Zed 官方发布）在 **Windows x64 + rustc 1.97** 上 `cargo check` + `cargo build` 通过，exe 启动窗口正常存活（DirectX/blade 渲染）。
- GPUI 0.2 提供 div/styled 布局、文本、滚动、uniform_list、data_table 等控件范式，支持 `cx.spawn` 异步与 `BackgroundExecutor` 后台线程。

## 决策

### D1：UI 框架 — GPUI 0.2.x（crates.io，Apache-2.0）
- 直接依赖 crates.io 发布的 `gpui = "0.2"`（非 git 依赖），锁定可复现。
- 仅启用所需 features；Windows 由 default（含 `windows-manifest`）覆盖。
- GPUI 为立即模式 + 保留式混合框架，UI 全 Rust 声明；无 HTML/CSS/JS。

### D2：保留并复用 `datasource` crate（纯 Rust 采集层）
- `datasource/`（secm-datasource：registry/service/power/netif/dns/http/activation/cpu_freq/disk/disk_io/error）**零 Tauri 依赖**，直接作为 workspace crate 迁入，零改动采集逻辑。
- 许可证 MIT，与主项目一致。

### D3：保留 LHM .NET sidecar（温度/功耗主数据源）
- 用户决策：保留 LibreHardwareMonitor .NET sidecar（进程隔离 + PawnIO 驱动链）。
- GPUI 主程序仅需 HTTP 客户端消费 127.0.0.1:45980 JSON（契约见 ADR-0005），不引入 .NET 技术栈进主程序。

### D4：可复用的纯逻辑 Rust 模块（从 src-tauri/src 迁入）
以下模块仅依赖通用 crate（windows-sys/winreg/ureq/surge-ping/trust-dns/sysinfo），剥离 tauri 后可复用：
- sensor（轮询编排）、lhm（sidecar 客户端）、hardware、driver_install（winring0/acpi/pawnio SCM 部署）
- cleanup、network、net_config、dhcp_probe、settings、hpt、nvidia_drs、environment、game_env
- sysinfo、net_stats、ip_info、disk_info、ds_util、log/debug/temp_data、cancel、proc_util

需剥离/重写的 tauri 专属面：
- command 宏与 IPC（→ 直接函数调用，见 ADR-0007）
- AppHandle/State/事件发射（→ 应用状态结构 + GPUI subscription）
- tauri-plugin-dialog/shell/fs/store（→ 原生替代，见 ADR-0008）
- tray（GPUI 0.2 无内建托盘 → tray-icon crate，见 ADR-0008）

### D5：第三方 UI 组件库
- 不引入重型 UI 库；用 GPUI 原语构建主题化控件集（Button/Card/Toggle/Select 等），对齐原 shadcn 视觉。
- 图标：复用源 assets 中 PNG/SVG（GPUI 支持 SVG 渲染）。

## 后果
- 正面：单一语言（Rust）、无 WebView2/Node 依赖、启动快、类型安全贯穿。
- 成本：全 UI 自绘（无浏览器控件），复杂交互（富表格/可编辑网格）需 GPUI 原语组合实现。
- 风险：GPUI 0.2 较新、API 演进快；需锁版本并定期跟随。
