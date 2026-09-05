# ADR-0008：系统能力差异与替代方案

- 状态：提议
- 日期：2026-09-05
- 关联：ADR-0001

## 背景

Tauri 提供了 WebView 之外的宿主能力（原生窗口菜单、系统托盘、文件对话框、shell、单实例、权限 capabilities）；GPUI 0.2 是渲染框架，部分宿主能力需自行集成或由 OS API 补充。

本机核实（2026-09-05，gpui 0.2.2）：GPUI 核心**不含**系统托盘 API、文件保存对话框、单实例锁；含窗口管理、输入法/文本输入、SVG 渲染、菜单（内部快捷键系统）与窗口标题栏配置。

## 决策与替代矩阵

| 能力 | 原 Tauri 方案 | GPUI 替代 | 优先级 |
|------|--------------|-----------|--------|
| 系统托盘（右键菜单/关闭到托盘） | tauri-plugin + tray-icon feature | `tray-icon` crate + 原生消息循环线程；经通道转发事件到 GPUI（需 App 侧集成，验证可行性） | 高 |
| 单实例 | tauri-plugin-single-instance | Windows Mutex/命名事件 + `FindWindow`/二次实例信号 | 中 |
| 文件对话框（导出日志/选择文件） | plugin-dialog | `rfd` crate（原生 Windows 对话框，无 webview 依赖） | 中 |
| 关闭到托盘行为 | prevent_close + hide | 窗口关闭事件拦截 + hide（GPUI window API） | 高 |
| 深色/浅色主题 | CSS 变量 | Theme 结构全局注入（ADR-0003 D2） | 中 |
| URL 外开（打开安全中心等） | plugin-shell/opener | `open` crate 或 `ShellExecuteW` | 低 |
| 持久化 | plugin-store / localStorage | serde_json 落盘 `%APPDATA%\SECM\prefs.json` + 日志目录 | 中 |
| 权限模型 | capabilities | 无（本机命令直调；权限由业务代码 is_admin 检查，同现状） | — |

### 托盘可行性处置
- **已验证（2026-09-05 tray-spike）**：tray-icon 0.24 + muda 0.19 在独立后台线程创建并自跑
  win32 消息泵，与 GPUI 0.2 主循环同进程共存通过——进程存活无崩溃，窗口枚举确认
  `tray_icon_app`（托盘消息窗口）与 `Zed::Window`（GPUI 主窗）并存。
  菜单事件经 `MenuEvent::set_event_handler` + std mpsc 回主线程，主线程 `cx.spawn` 收消息更新视图。
  此模式即 ADR-0005/0004 所述"后台线程 + 通道回 UI"的统一实现样板。

### 补充发现（GPUI 0.2 内建能力，减少第三方依赖）
- `Window::hide()` / `Window::activate()`（window.rs example 证实）：关闭到托盘、从托盘恢复可直接用 GPUI API。
- `Window::prompt(PromptLevel, …)`：原生消息框（含非英文按钮），无需 rfd 即可做简单确认框。
- `cx.spawn` + `WeakEntity::upgrade` + `Entity::update(cx)`：后台消息安全回 UI 的官方模式（tray-spike 已验证）。

## 后果
- 系统托盘可行性已实证（tray-spike），非风险项；进入实施直接采用后台线程模式。
- 关闭到托盘用 GPUI 内建 hide/activate，无需额外 crate。
- 文件对话框（导出日志）仍走 rfd 或 `Window::prompt`（纯文本保存路径场景 rfd 更佳）。
