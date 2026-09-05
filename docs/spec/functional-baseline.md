# SECM 功能基准规格（GPUI 重构验收依据）

> 本文档列出重构必须保持的功能行为（对齐 v1.19.0 Tauri 版），作为每阶段验收对照。
> 状态：已校准（基于源 v1.19.0 全量 80 命令枚举 + 前端 invoke 面 + 流式事件 kind 核查）

## 全局行为
- 单实例运行（防 WebView2 冲突 → 防多开；GPUI 下为 Mutex/命名对象锁）
- 关闭窗口隐藏到系统托盘（恢复/退出）
- 1s 传感器轮询仅在仪表盘可见时驱动（可切换页面不泄漏）
- 日志：用户日志环形 200 条 + 落盘 `%APPDATA%\SECM\logs\<date>.log`；调试日志 `diag/debug.log`；诊断目录自动维护
- 权限：普通权限启动，管理员操作由各命令内部 is_admin 检查返回中文错误

## 页面功能基准
| 页 | 验收要点 |
|----|---------|
| Dashboard | CPU 占用/频率（源标记 ntapi/pdh/registry/sysinfo）、温度/功耗（lhm/winring0/acpi/none + 估算角标）、GPU 摘要、内存、磁盘卡片、60s 趋势图跨重启恢复、网络速率卡与活跃连接、驱动状态卡（降级引导 + 一键重试 + 杀软指引）、频率诊断摘要卡 |
| Cleanup | DNS 刷新、临时/各厂商着色器缓存清理（结果追溯面板）、工作集修剪（管理员）、进程表（Top 200/搜索/6 档优先级） |
| Network | ping/traceroute/nslookup（流式 + 参数 + 取消）、网站测试（localStorage 持久化 → prefs.json）、NAT 检测（多 STUN 服务器 + NAT0-4）、端口连通性、iperf3（需系统已装）、DHCP 检测与深度检查 |
| NetConfig | 适配器枚举、MAC/IPv4+DNS/IPv6/DHCP 静态切换、备份恢复、DoH 配置 |
| Settings | HAGS/游戏模式/窗口化优化/鼠标精准度/VRR/高精度计时器开关、电源计划（读/切/删/卓越导入）、异类线程调度策略、NVIDIA 电源模式 |
| Services | 服务全量枚举/搜索/启停/启动类型（自动/手动/禁用） |
| Environment | 系统信息 8 字段、游戏环境 5 款预设对比与一键切换、DirectX 诊断、VC++ 12 项、AI 工具 10 项并行检测 |
| AiEnvironment | npm 环境、AI 工具安装/升级/卸载（白名单）、MCP 13 项管理、Skills/扩展扫描 |
| Hardware | 磁盘清单（型号/容量/健康告警）、SMART 详情弹窗（IOCTL 三级降级链） |
| Logs | 实时推送、200 条环形、级别筛选/统计、搜索、导出 txt、清空；跟随滚动 |
| About | 版本/构建信息、更新日志（内嵌 CHANGELOG）、第三方许可清单、赞助/抖音二维码（保留） |

## 数据契约（结构保留，UI 消费）
- SensorSnapshot（cpu/gpu/memory/disks/motherboard/diag + source 标记）
- 流式 StreamEvent kinds：info / ping / trace-hop / dns-record / stun-step / summary / error
- 图表历史 chart_history.jsonl（60 点/页、跨重启）
- DiskSummary / DiskSmartDetail（health/alert/attributes）
- 服务/电源/网络配置结构体（serde 同构）

## 不被迁移（确认移除）
- WebView/CSP/remote-debug、tiny_http 17860 服务、React Router、localStorage、shadcn/radix/tailwind/recharts、node/vite/tsc 工具链
