# ADR-0009：分阶段实施计划与验收

- 状态：已采纳（实施中）
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

### Phase 1 — 骨架与仪表盘（**已完成** ✅）
- workspace 三 crate 骨架；secm-datasource 迁入并跑通单测（50 测试）✅
- secm-core 迁入 sensor/lhm/log 基础面（去 tauri）✅
- UI：MainLayout + 侧边栏 + Theme + 控件集（Button/Card/Badge）+ 路由枚举 ✅
- 页面：**Dashboard**（传感器摘要卡 2×2 + 1s 轮询 + LHM 温度/GPU + diag 行）✅
- 验收：启动即见实时仪表盘 ✅

### Phase 2 — 系统设置 + 服务（**已完成** ✅）
- secm-core 迁入 settings（HAGS/游戏模式/电源计划）+ services ✅
- 页面：**Settings**（开关组/电源计划）、**Services**（服务表格+启停）✅
- 遗留：Settings 页 VRR/窗口化优化/鼠标精准度/异类策略/卓越性能开关，待后端补充后接入（子代理迁移中）

### Phase 3 — 清理 + 硬件（**进行中**）
- secm-core 迁入 cleanup（进程/优先级/DNS）+ hardware（磁盘 SMART）✅
- 页面：**Hardware**（磁盘清单+SMART 详情展开）✅
- 页面：**Cleanup**（进程表+优先级+DNS 刷新）✅
- 遗留：Cleanup 缓存清理按钮（temp/厂商/DX/Steam）——后端子代理迁移中，接 UI

### Phase 4 — 网络（**已完成** ✅）
- secm-core 迁入 network（DNS/端口/HTTP）+ net_config（netsh/DoH/MAC）+ netif ✅
- 页面：**Network**（网站/端口/DNS 后台诊断）、**NetConfig**（适配器枚举+DHCP/静态/MAC 表单）✅
- 验收：诊断真实执行、网络配置读写命令层迁移（netsh 运行时验证留真实环境回归）

### Phase 5 — 环境 + AI（**进行中**）
- secm-core 迁入 environment/sysinfo/game_env：子代理迁移中
- 页面：**Environment**、**AiEnvironment**：待后端落地后实现

### Phase 6 — 日志 + 关于 + 托盘 + 收尾（**进行中**）
- 页面：**Logs**（环形缓冲+筛选）、**About**（版本/许可）✅
- 系统托盘（显示/退出）✅
- 遗留：单实例锁、关闭到托盘、`cargo build --release`、迁移遗留清理（README/CHANGELOG/版本 v2.0.0 已同步；gpui-spike/tray-spike 在旧仓库 .gitignore 冻结）、仓库推送

## 验收总则
1. 每阶段 `cargo build`（debug）+ 页面级人工冒烟 + 相关 `cargo test` 通过
2. 功能与源版本逐项对照（docs/spec 功能基准清单）
3. 最终 `cargo build --release` 出可分发 exe + `cargo test` 全绿 + 冒烟启动
