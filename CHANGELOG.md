# 更新日志

## [v2.6.0] - 2026-09-06
### 修复+变更（MINOR：日志清空修复 + 清理优化页现代化排版）
- **修复日志清理无效**：点 X(清除日志) 只清了 UI 列表、未清全局 LogBuffer 环形
  缓冲 → 500ms 轮询把旧日志全量回填，看起来"清不掉"。现清除时连全局缓冲一并
  清空（保留按天落盘文件），随后仅出现一条「日志流已清空」确认
- **清理优化页现代化整理**：
  - 卡片统一现代层次：r12 圆角 + 顶部分区色点标题条 + 细分隔线
  - 缓存清理卡重排：系统临时 / 显卡着色器缓存 两分组子区；底部操作条
    「一键清理全部着色器缓存」（品牌主色）+「修剪工作集」（危险红）
  - 快捷操作卡与进程管理卡拆分：快捷操作（DNS 刷新/进程刷新/搜索）在主色区；
    进程管理带独立标题与实时进程数徽标
  - 清理按钮三态配色现代化（普通中性 / 危险红 / 一键品牌蓝，禁用弱化）

## [v2.5.0] - 2026-09-06
### 变更（MINOR：自适应响应式 + 左右栏独立拉伸 + 日志面板按钮调整 + 清理页两栏）
- **左右侧边栏独立拉伸（Main 自适应）**：解除两侧 max 宽互相扣减的旧约束
  （此前右栏宽时左栏被锁死 180）。现在每栏可拖 max 只扣对方已占宽，拉左只压
  Main、右栏不动，反之亦然（layout.rs `sidebar_max_width` / `right_panel_max_width`）
- **分隔条拖拽全程跟踪**：on_mouse_down 启动 + 全尺寸 shell 根元素 on_mouse_move
  持续接收（指针可离开 12px 分隔条），on_mouse_up/up_out 收尾持久化 —— 替换原
  on_mouse_move 仅 hover 命中触发、拖出即失联的问题
- **日志面板**：右侧栏头部移除收起/展开按钮（开合改由 Main 顶栏右栏开关控制）；
  X 按钮语义 = 清除日志（日志流清屏，保留全局缓冲与落盘）
- **清理优化页左右两栏布局**：左 = 缓存清理 + 结果追溯；右 = 快捷操作 + 进程管理；
  主内容区 <900px 时自动改为上下堆叠（响应式）
- 自适应基础（既有）：flex 布局 + 640/960 断点 + 10 页根滚动防裁切；窗口任意
  缩放布局跟随不崩（1300/1100/1000px 宽实测）

## [v2.4.1] - 2026-09-06
### 修复（MINOR：窗口默认尺寸/拉伸 + 内容区排版 + 文本复制）
- **窗口默认 1600×900 并解除拉伸限制**：默认 bounds 1280×800 → 1600×900；
  `win32::strip_title_bar` 恢复保留 `WS_THICKFRAME`（此前一并移除致窗口不可
  系统边缘拉伸；去 WS_CAPTION|WS_SYSMENU 保留无标题栏外观）
- **修复日志面板单行超宽被截断**：消息列加 `flex_1 + min_w(0)` 自动折行完整
  显示（时间戳列 108→100、对齐 items_start）
- **日志流内容可复制**：点击任意日志行复制整条（级别+时间+消息）到系统剪贴板
- **左侧网络信息卡 IP 可复制**：本地 IPv4 / 公网 4 槽 IP 值可点击复制
  （阻断冒泡避免误触整卡刷新）
- **主内容区排版修复（内容超高被裁切）**：10 个工具页根容器加
  `.id(...) + overflow_y_scroll()` —— 内容超高时整页纵向滚动，不再被外壳
  overflow_hidden 静默裁切；各页固定高表格（进程/服务表等）不受影响

## [v2.4.0] - 2026-09-06
### 新增（MINOR：右栏日志流面板 + 左栏日志页迁移 + 全链路行为打点）
- **右栏整改为日志流面板**（替换原文件工作台）：头部「日志流」+ 级别筛选（全部/Info/
  Warn/Error）+ 清空 + 开关；主体实时滚动日志行，新日志自动跟随到底（ScrollHandle）；
  面板默认展开；数据源 = 全局 LogBuffer（log crate 桥接，与按天落盘同源）
- **左栏「调试日志」页删除**：SecmPage 移除 Logs（10 页导航），旧 logs.rs 删除，
  日志统一在右栏常驻展示
- **全链路行为/操作打点**：10 个工具页 UI 层补操作日志（页面打开/按钮触发/操作结果/
  失败 warn）+ 壳层交互（主题/侧栏/面板/切页/清空/筛选）；与 core/datasource 底层采集
  日志同流汇聚 —— 前端操作 + 后端返回信息一条流完整呈现
- secm-app 增 log 依赖

## [v2.3.2] - 2026-09-06
### 变更（PATCH：主窗体四角改圆角）
- 无边框主窗体启用 DWM 窗口圆角（`DWMWA_WINDOW_CORNER_PREFERENCE` = ROUND）
- 仅 Windows 11（22000+）生效；Win10/旧版 DWM 调用静默忽略（保持直角，不影响功能）
- 最大化时 DWM 自动切换方角、还原自动恢复圆角，无需额外处理
- 圆角半径跟随系统主题（默认约 8px）
- 验证：DWM 查询 corner=2(ROUND) hr=0；像素采样窗口角落为背景过渡非面板色

## [v2.3.1] - 2026-09-06
### 修复（PATCH：侧栏网络信息「公网 v4 · 国内」没正确读取）
- **根因**：公网 IPv4 仅用 `api.ipify.org` 取系统路由当前出口 —— 代理环境下 v4
  出口为国外（如新加坡），归属非 CN → 国内 v4 槽恒空
- **修复**：「公网 v4 · 国内」槽改走国内回显端点
  `members.3322.org/dyndns/getip`（国内 DDNS 服务，实测经国内线路返回中国电信
  CN 出口 106.127.136.216），取回后经 ip-api countryCode 核验归属兜底；
  ipify 出口归属非 CN 时仍填国外槽 —— 两槽可同时出数
- 新增 `extract_ipv4` 文本提取（带单测）+ 端点白名单扩展
- 真机验证：v4 国内=106.127.136.216（CN）、v4 国外=203.27.106.146（SG）、
  v6 国内=240e:...（CN），四槽正确填充

## [v2.3.0] - 2026-09-06
### 修复（MINOR：卡片取色交付修复 + 侧栏网络信息复原）
- **修复卡片背景取侧栏底色未生效**：上轮已将工具页卡片底色 `theme.panel` 对齐侧栏
  `#242426`，但 release/dist 产物未重建导致用户运行旧 exe 看不到变化 —— 本次全量
  重建 release + 重新发布，交付含新色的新产物
- **复原左侧边栏「网络信息」卡（旧版功能迁移）**：
  - 新增 `secm-datasource::net_io`：PDH `Network Interface` 计数器实时上下行速率
    （每网卡 KB/s；物理网卡实例，真机验证出数）
  - 新增 `secm-core::net_info`：编排协商速率 + 本地 IPv4 + 公网 4 槽（国内/国外 ×
    IPv4/IPv6，ipify 回显 + ip-api 归属 countryCode 判定）+ 实时上下行；固定白名单
    URL + 5s 超时 + 逐槽失败降级
  - 侧栏底部卡升级为网络信息卡：已连接网卡（Up + 非 APIPA IPv4）+ 协商速率 +
    ↓下行/↑上行 + 本地 IPv4 + 公网 v4/v6 国内/国外 4 行；每秒轻量刷新速率（不重复
    打公网），点击全量刷新
  - 虚拟网桥（Hyper-V vEthernet）场景：速率取所桥物理网卡最大流量实例，数据真实可用
- `workspace` log 依赖补 `std` feature（修 core 独立测试的 alloc 缺省问题）

## [v2.2.0] - 2026-09-06
### 新增（MINOR：克隆壳桌面 UI 全面重构 + 自绘无边框窗口）
- **UI 壳整体重构**：移除旧 GPUI 导航宿主（app.rs/AppRoot/Page），新宿主
  `PiShell` 三栏 Flex 布局（Sidebar 导航 | Main 工具页 | 右栏文件工作台）成为唯一主界面；
  SECM 11 工具页保留源码、由侧栏分组导航（概览/工具/系统）切换加载
- **无边框自绘窗口 chrome**：主窗口去除系统标题栏/边框（WS_CAPTION/SYSMENU/THICKFRAME），
  内容直达窗口顶；右上角自绘最小化/最大化/关闭按钮（Win32 原生动作）；侧栏品牌行与
  Main 顶栏标题区为窗口拖动热区（window_control_area::Drag）
- **可拖拽面板**：侧栏（默认 260px，180–480）与右栏文件工作台（约 42vw，360–640）支持
  col-resize 拖拽 + 宽度持久化（%LOCALAPPDATA%\SECM\pi-panel-widths.json）；开合带逐帧
  指数缓动动画；640/960 断点下移动抽屉与并排三栏自适应
- **主题**：GPUI Theme Tokens 双套语义色板（Light/Dark，取值对齐参考外壳 native-theme.css），
  组件零硬编码颜色；顶栏/侧栏主题切换按钮即时换肤
- **面板图标修复**：GPUI 0.2 svg 必须自身 text_color 才绘制，全链路图标调用点显式上色
  （此前折叠/展开按钮等图标丢失）；右侧栏开关移入顶栏最右、与自绘窗口控制并排
- **右键底部工具条**：侧栏底部三图标（Gauge/Info/Settings）置底唯一渲染，消除与导航尾部
  重复实例；窗口拖动热区贴顶（消除顶部命中盲区）
- 会话相关功能（项目树/会话树/New Chat/会话搜索等）全链路移除，侧栏/右栏与布局语义
  同步收敛为工具页 + 文件工作台
- 实现与逐项测量记录见 docs/ui-clone/（开发期文档，不随发布包携带）

## [v2.1.1] - 2026-09-05
### 修复（PATCH：启动黑框）
- **修复启动程序弹出命令提示符黑框**：secm-app.exe 此前以 console 子系统链接，
  启动时系统分配控制台窗口与主窗口并存。release 构建声明
  `#![windows_subsystem = "windows"]`（GUI 子系统）后零黑框；debug 构建保留
  控制台供开发期日志。子进程侧此前已全量 CREATE_NO_WINDOW（v2.0.1 起覆盖
  npm/netsh/powershell/taskkill/sidecar），本次为主程序子系统收口
- 回归验证：发行 exe PE 头 Subsystem 字段 = 2（IMAGE_SUBSYSTEM_WINDOWS_GUI）

## [v2.1.0] - 2026-09-05
### 新增（MINOR：全链路图标统一接入 crates/icons 资源）
- **exe 文件图标**：新增 build.rs + winresource，把 `crates/icons/icon.ico` 嵌入
  secm-app.exe 资源段（资源 ID 1）——Explorer/任务栏/Alt-Tab 图标来源
- **窗口图标**：GPUI 0.2 无窗口图标 API，新增 `icons::set_window_icon_from_gpui`：
  经 raw_window_handle 取 Win32 HWND 后 `LoadImageW`（嵌入资源）+ `WM_SETICON`
  挂接大/小图标（标题栏/任务栏）
- **托盘图标**：由程序化蓝色圆点改为解码 `crates/icons/32x32.png`（编译期
  include_bytes! 嵌入，无运行时文件依赖）；PNG 解码失败时降级为占位圆点并记录日志，
  保证托盘可用性
- 新增 `image`（png 解码）、`raw-window-handle`、`winresource` 依赖（与 gpui 内部
  版本对齐）；`icons.rs` 模块收口全部图标接入点
- 发布脚本无需改动：图标已嵌入 exe，dist 不额外携带图标文件

## [v2.0.1] - 2026-09-05
### 修复（PATCH：全链路审计修复批次 1-4，见 docs/audit-report-2026-09-05.md）
- **正确性**：游戏预设"一键套用"鼠标精准度取值反相（推荐"关闭"时反而启用）；
  预设套用改为逐项回传成败并回显失败明细，不再吞错谎报"已应用 N 项"
- **安全**：缓存清理入口对根路径 junction/符号链接整体跳过，空目录回收不再删除顶层根，
  递归删除/只读清除增加 64 层深度上限（防预植 junction 越权删除与深树栈溢出）
- **网络配置**：MAC 修改禁用/启用失败时自动回滚注册表原值并尽力恢复网卡启用态；
  DoH 批量应用合并为单次 PowerShell 进程（原 2N+1 次冷启动），清空路径限定本接口
  DNS 集合（原会误删全机 DoH 记录）；run_ps_result 对"退出码 0 但 stderr 非空"判失败
- **子进程生命周期**：新增带超时进程执行工具（超时 taskkill 杀整棵进程树），接入
  npm 检测/安装/卸载、netsh、PowerShell 全链路，消除后台任务永久悬挂；应用退出
  （on_app_quit）经 sidecar `/api/shutdown` 受控退出 + PID/映像名兜底清理，不再残留
  孤儿 LhmSidecar.exe；sidecar 路径探测移除 cwd 候选与开发机硬编码路径
- **日志**：新增 log crate → LogBuffer 桥接后端 + 按天落盘（%LOCALAPPDATA%\SECM\logs，
  自动清理 7 天前旧文件），修复全库 47+ 处 log::* 无后端静默丢弃、调试日志页空转；
  LHM 传感器拉取失败增加 5s 退避，ensure_running 移独立线程不再冻结 1s 采集线程
- **稳定性**：托盘构建失败经日志页可见（不再静默消失）、线程启动失败不再 panic；
  文本输入控件移除 UI 线程 unwrap/断言崩溃点；sysinfo/environment/network 多线程
  采集 panic 隔离为字段级降级；服务枚举增加 256 轮迭代上限与探测错误码校验
- **设置**：异类调度策略 AC/DC 写入失败自动回滚已写侧；服务查询失败与"未安装"
  区分为"检测失败"
- **体验**：页面懒加载（首次访问才构造，消除启动 13 任务并发风暴）；Dashboard/Logs
  轮询加页面可见性门控（不可见不再空转）；服务页/清理页搜索框接入真实输入框；
  托盘线程期 UI 崩溃点修复
- **清理**：接线 datasource disk_io（磁盘读写速率）与 cpu_freq（频率降级链）至传感器
  服务，清除 read_mbps/write_mbps 占位；DNS 刷新收敛到 datasource::dns（删除内联
  FFI 重复）；日期归一化/标题翻译收敛到 datasource 单一实现；删除死模块
  datasource::http 与死依赖（muda/serde_json/env_logger/datasource-ureq）；
  服务/清理死功能（Toggle* 按钮组、keyword 死字段、_danger 参数）收敛
- **依赖**：统一 winreg 0.52/0.55 双版本 → workspace winreg 0.56；windows-sys 0.59 →
  0.61.2（PDH 句柄/bool 字段/IpHelper 门控适配）；workspace features 裁剪至实际使用面
- **版本治理**：版本号收敛为 Cargo.toml 单点（about/主界面/仪表盘改为
  env!("CARGO_PKG_VERSION")）；publish.ps1 改为始终重发布 sidecar、版本从 Cargo.toml
  解析、发行包补齐 LICENSE/README/CHANGELOG 并扩展驱动/许可清单校验；
  清理测试对系统重启删除队列的真实副作用（测试构建下 mark_delete_on_reboot no-op）
- **文档**：README/About/ADR-0005/ADR-0006 温度降级链声明与代码现状对齐（v2.0.x 仅
  LHM 通道，WinRing0/ACPI 为后续计划）；third_party README 修正 WinRing0 签名口径与
  Defender 拦截现状；sidecar 错误提示改为可操作指引；移除悬空文档引用

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
