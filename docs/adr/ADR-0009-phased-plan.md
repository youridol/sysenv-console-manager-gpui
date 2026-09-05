# ADR-0009：分阶段实施计划与验收

- 状态：提议
- 日期：2026-09-05
- 关联：所有 ADR

## 背景

重构覆盖 11 页面/80 命令/约 1.5-2 万行 GPUI 代码，一次性交付风险过高；采用可验证的分阶段增量交付，每阶段以"可编译 + 可运行 + 验收通过"为完成标准。

## 阶段计划（每阶段含版本/CHANGELOG/编译验证）

### Phase 0 — 可行性与环境（**已完成** ✅ 2026-09-05）
- GPUI spike：编译通过 + exe 启动存活 ✅
- 中文渲染 spike：cosmic-text/font-kit 中文无崩溃 ✅
- 托盘共存 spike：tray-icon 0.24 后台线程 + win32 消息泵与 GPUI 主循环共存实证
  （窗口枚举见 `tray_icon_app` + `Zed::Window` 并存）✅
- 自绘折线图：GPUI `canvas(prepaint,paint)` element + `PathBuilder`（move_to/line_to/
  stroke）确认可行（painting.rs 先例）✅
- 对话框/隐藏窗口：`Window::prompt`、`Window::hide/activate` 内建确认 ✅

### Phase 1 — 骨架与仪表盘
- workspace 三 crate 骨架；secm-datasource 迁入并跑通单测
- secm-core 迁入 sensor/lhm/log/temp_data 基础面（去 tauri）
- UI：MainLayout + 侧边栏 + Theme + 控件集（Button/Card/Badge）+ 路由枚举
- 页面：**Dashboard**（传感器摘要卡 + 1s 轮询 + 趋势图自绘 + 驱动状态卡）
- 验收：启动即见实时仪表盘

### Phase 2 — 系统设置 + 服务
- secm-core 迁入 settings/hpt/nvidia_drs/power（去 tauri）
- 页面：**Settings**（开关组/电源计划/调度策略/NVIDIA 模式）、**Services**（服务表格+启停+启动类型）
- 验收：开关真实改系统注册表并回读

### Phase 3 — 清理 + 硬件
- secm-core 迁入 cleanup/disk_info
- 页面：**Cleanup**（缓存清理/进程管理/结果追溯）、**Hardware**（磁盘清单+SMART 弹窗）
- 验收：清理操作与 SMART 读取真实生效

### Phase 4 — 网络
- secm-core 迁入 network/net_config/dhcp_probe/net_stats/ip_info
- 页面：**Network**（流式工具集 + 网站测试 + NAT/端口）、**NetConfig**（配置编辑器）
- 验收：ping/traceroute 流式输出与取消、DHCP 检测、网络配置读写

### Phase 5 — 环境 + AI
- secm-core 迁入 environment/game_env/sysinfo/ip_info
- 页面：**Environment**（DX/VC++/游戏/系统信息）、**AiEnvironment**（AI 工具/npm/MCP/扩展）
- 验收：并行检测与 npm 安装/升级链路

### Phase 6 — 日志 + 关于 + 托盘 + 收尾
- 页面：**Logs**（环形缓冲+筛选+导出）、**About**（版本/更新日志/第三方许可/赞助卡）
- 系统托盘、单实例、关闭到托盘
- 迁移遗留清理（旧仓库冻结、README/CHANGELOG/版本 v2.0.0、仓库推送）
- 最终验收：tsc 概念不复存在——以 `cargo build --release`、`cargo test`、人工冒烟为准

## 验收总则
1. 每阶段 `cargo build`（debug）+ 页面级人工冒烟 + 相关 `cargo test` 通过
2. 功能与源版本逐项对照（docs/spec 功能基准清单）
3. 最终 `cargo build --release` 出可分发 exe + `cargo test` 全绿 + 冒烟启动
