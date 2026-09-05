# ADR-0005：温度/功耗传感链 — 保留 LHM .NET sidecar

- 状态：提议
- 日期：2026-09-05
- 决策人：用户（保留 sidecar）

## 背景

SECM 温度/功耗主通道为 LibreHardwareMonitor .NET 8 sidecar（独立进程提权，经 PawnIO 驱动 ring0 读取），降级链 lhm → winring0 → acpi。原 Rust 侧 `lhm.rs` 负责 sidecar 进程生命周期与 HTTP 轮询。

## 决策

### D1：sidecar 二进制与源码继续随项目分发
- 保留 `sidecar-lhm/`（.NET 源码）与构建/打包产物分发逻辑；本重构仅替换主程序 UI/装配层，sidecar 契约不变。
- sidecar 生命周期管理代码（lazy 启动/UAC 提权探测/崩溃重启/健康检查）从 src-tauri/src/lhm.rs 迁入 secm-core 并去 tauri 化（去掉 AppHandle 传参 → 纯路径参数）。

### D2：HTTP 契约复用（45980）
- 复用现有 JSON 契约（/health、/api/lhm/sensors），字段结构（cpu.package_temp_c / power_w / core_temps_c、gpu[]、motherboard、memory）原样保留 → Rust 侧 serde 结构直接复制，前端解析零改动成本。
- HTTP 客户端：现有 ureq（同步阻塞，配 BackgroundExecutor）保留。

### D3：降级链不变
- sensor 编排保持：lhm → winring0（WinRing0x64.sys 端口 IO/MSR）→ acpi → none；
- driver_install（SCM 服务部署 PawnIO/WinRing0）逻辑迁入 secm-core，去掉 Tauri 资源定位 → 直接基于可执行文件相对路径定位 resources。

## 后果
- 主程序保持纯 Rust（.NET 只在 sidecar 进程内），与"仅依赖 Rust+GPUI"目标不冲突（sidecar 是外部数据源进程，等同 WinRing0/PawnIO 驱动属外部组件）。
- 用户接受此方案意味着重构不引入新的温度读取 Rust 实现，风险最低、功能一致性最强。
