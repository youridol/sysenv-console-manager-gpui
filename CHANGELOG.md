# 更新日志

## [v2.0.0] - 2026-09-05
### 变更（MAJOR：纯 Rust + GPUI 完整重构）
- 移除 Tauri 2 / React 19 / TypeScript / Vite / WebView2 / Node 旧技术栈，交付纯 Rust + GPUI 0.2 桌面应用
- UI 全量重写为 GPUI：11 页（仪表盘/清理/网络/网络配置/设置/服务/环境/AI 环境/硬件/日志/关于）+ 系统托盘 + 单实例锁
- 保留并复用纯 Rust 采集层 secm-datasource（注册表/服务/电源/网络/DNS/HTTP/激活/CPU 频率/磁盘/SMART）
- 保留 LHM .NET sidecar（LibreHardwareMonitor，MPL-2.0 进程隔离）作为温度/功耗主数据源；sidecar 源码 + 许可随新仓库管理（sidecar-lhm/）
- 保留 WinRing0/ACPI 温度降级链与第三方驱动依赖（third_party/）
- 项目结构重组为 Cargo workspace（secm-datasource / secm-core / secm-app），架构决策见 docs/adr/
- 业务模块全量迁入 secm-core：cleanup（缓存/进程/服务）· settings（HAGS/游戏模式/VRR/鼠标精准度/异类调度/电源计划）· environment/sysinfo/game_env（DX/VC++/AI 工具/npm/MCP/扩展/系统信息）· net_config/netif（netsh/DoH/MAC/适配器）· hardware（磁盘 SMART）
- 新增 `scripts/publish.ps1`：一键组装便携发布目录（Rust release + sidecar dotnet publish + 许可/源码随包），产物 dist/secm-v2.0.0/

> 本版本为 GPUI 重构首发。历史（Tauri 版 v1.x）见原仓库 youridol/sysenv-console-manager；
> v2.0.0 新仓库：https://github.com/youridol/sysenv-console-manager-gpui
