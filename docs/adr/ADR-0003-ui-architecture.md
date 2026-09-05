# ADR-0003：UI 架构（GPUI 视图模型 / 导航 / 主题）

- 状态：提议
- 日期：2026-09-05
- 关联：ADR-0002、ADR-0009

## 背景

原 UI：React Router 11 页（路由 `/ /cleanup /network /net-config /settings /services /environment /ai-env /hardware /logs /about`），MainLayout 侧边栏导航 + 共享 NetIfaceContext，shadcn/ui + Tailwind 视觉。GPUI 无路由库，需自建导航模型。

## 决策

### D1：单窗口单根视图 + 页面枚举导航
- `App` 根 View 持 `Page` 枚举与当前选中态；侧边栏按钮切换，内容区渲染对应页面 View。
- 等价路由表（不引入 URL 路由；若需深链再议）：

| Page | 原路由 | UI 形态 |
|------|--------|---------|
| Dashboard | `/` | 实时传感器摘要卡 + 趋势图 + 驱动状态卡 |
| Cleanup | `/cleanup` | 操作按钮 + 进度/结果列表 + 进程表 |
| Network | `/network` | 诊断工具集合（流式输出面板 + 参数表单 + 结果表） |
| NetConfig | `/net-config` | 网络配置编辑器（适配器下拉 + 字段表单） |
| Settings | `/settings` | 开关组/选择器/电源计划/调度策略/NVIDIA 模式 |
| Services | `/services` | 服务表格（搜索/启停/启动类型） |
| Environment | `/environment` | 检测清单卡片（DX/VC++/游戏环境/系统信息） |
| AiEnvironment | `/ai-env` | AI 工具卡片 + npm/MCP/扩展管理 |
| Hardware | `/hardware` | 磁盘清单 + SMART 详情弹窗 |
| Logs | `/logs` | 环形日志视图（筛选/搜索/导出） |
| About | `/about` | 版本/构建/更新日志/第三方许可 |

### D2：主题化控件集（对齐原 shadcn 视觉）
- 新建 `secm-app/ui/`：Button / Card / Badge / Toggle(Switch) / Select / Input / Table / Dialog / Tabs / Progress / ScrollArea / Tooltip。
- 颜色 token 收敛为 `Theme` 结构（深浅两套），全局注入；对齐原 Tailwind 色板（zinc + brand-blue/purple + 状态色）。
- 图标用轻量内联 SVG（GPUI svg 渲染），替换 lucide。

### D3：跨页共享状态
- `SensorSnapshot` 全局：后台 1s 轮询填充，各页订阅（对齐 NetIfaceContext + Dashboard 1s）。
- `LogBus`：200 条环形缓冲 + 事件订阅（对齐 log.rs/log 页）。
- 本地偏好（如网站测试列表）落 `%APPDATA%\SECM\prefs.json`（对齐原 localStorage 语义，去 Web 存储）。

### D4：对话框/弹窗/通知
- GPUI 弹层需自行管理（覆盖层 stack）——建 `ModalHost`；toast 通知自绘（对齐 sonner）。
- 原生文件对话框（导出日志等）走 rfd 或 Windows 通用对话框（ADR-0008）。

## 后果
- 自绘控件集一次投入、全页复用；后续增页成本线性。
- 无 WebView，内存占用与启动延迟显著下降。
