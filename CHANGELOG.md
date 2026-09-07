# 更新日志

## [v2.11.0] - 2026-09-08
### 新增（MINOR：LiteMonitor 硬件采集迁移落地 + 硬件信息页布局/图表重制）

- **硬件采集迁移（ADR-0001~0010 全链路落地，见 audit/ 审计文档）**：
  - 统一 HardwareSnapshot v2：`Metric<T>`（value/source/updated_at/error）全域
    不可用语义——真实失败显示"n/a + 诊断"，杜绝 0/默认值冒充（LiteMonitor 历史
    假值 0f/2500MHz/16GB 未迁移）；
  - CPU 负载主路径对齐 LiteMonitor：PDH `% Processor Utility` → `% Processor Time`
    → sysinfo 差分回退；频率链 ntapi→PDH→registry 保留；
  - 网络速率唯一权威来源 = GetIfTable2 差分（LiteMonitor LHM Throughput 等价底层），
    FilterInterface/NotHardware 位域 + QoS/Npcap 等关键词双重过滤虚拟接口；下线
    PDH Network Interface 旧来源（net_io.rs 删除）；
  - 磁盘活动时间 `% Disk Time`（LiteMonitor DISK.Activity 等价）补齐；
  - sidecar 契约 v3：+Storage 磁盘温度（30s 慢刷，首拍立即刷新）/ +Battery 电量/
    功率/电流/电压（AC 符号修正）/ 主板传感器 hw 字段 / CPU 电压排除规则
    （soc/gt/sa/aux）/ GPU 熔断（>6000MHz、>1200W）/ 核显 Shared 显存优先 /
    SPD 取真实 DIMM 型号；
  - 智能匹配等价迁移：MOBO.Temp 智能选择策略（System>Motherboard>Chipset/PCH>
    合理范围最大 + 硬上限）、FanMapper 风扇/水泵匹配（底噪 200RPM、Cooler 优先、
    高转速 Pump 猜想）、电池 AC 符号修正（充电正/放电负）；
  - 双 Token 真机验证：管理员全指标出值；普通用户 USER_SAFE 域全绿；UAC 取消
    返回 available:false + 明确诊断（零伪造值）；`SECM_DISABLE_LHM` 显式降级开关。
- **硬件信息页布局重制**：上行 CPU / 内存 / GPU / 网络速率趋势 **一行四列**；
  下行磁盘存储 · SMART 健康 | 网络流量 **左右两列**。
- **图表美化**：全部趋势图由"等宽柱状 sparkline"改为**波浪线**（Catmull-Rom 平滑
  曲线 + 1.8px 描边 + 曲线下方面积渐隐填充，gpui canvas + PathBuilder 矢量渲染）。

## [v2.10.4] - 2026-09-07
### 修复（PATCH：卡片纵向黏连真根因 —— taffy 0.9.0 纵向 gap 不渲染，改逐块 margin）
- **根因（彩色标记取证法定位）**：给关于页各层临时涂唯一纯色（内容体=绿/页头=蓝/
  信息卡=红）后逐像素分段实测——页头→卡片、卡片→卡片之间 **0px 绿色分离带**
  （三段子块直接黏连），且 `PAGE_GAP` 提到 72px 亦无任何变化 → **容器 `.gap()` 的
  纵向分量（taffy gap.height）在 gpui 0.2.2 + taffy 0.9.0 组合下不渲染**（横向
  gap.width 正常 —— 与"左右间距正常、上下黏连"现象完全吻合）。v2.10.3 的
  "机制正常"结论系测量误判（把卡内 sparkline 区 56px 高度误读为分离带），据此
  调大 gap 数值自然无效
- **修复**：纵向间距弃用容器 gap，改由块级组件自带下外边距承担 ——
  `page_header` / `card` / `banner` 统一 `.mb(PAGE_GAP=24)`（覆盖全部页面的
  页头→卡、卡→卡、卡→区块边界）；page_root / page_body 移除无效的容器 gap；
  dashboard 卡内节奏（统计行/趋势组/网卡速率行/磁盘分组）改显式 `.mt()`；
  settings 异类策略 chips 行补 `.mt()`
- **验证（真机像素级，PrintWindow 清洁取证）**：
  - 硬件信息页：三行卡间出现 **22px / 26px** 页面背景分离带（≈24±描边）
  - 关于页：产品卡↔信息卡 **26~32px** 分离带（x=1030/x=305 两列，差值为圆角）
  - `cargo check` 零警告；`cargo test` 89 通过 0 失败

## [v2.10.3] - 2026-09-07
### 调整（PATCH：页级纵向节奏数值 16 → 20）
- 调整 `PAGE_GAP` 16 → 20、dashboard 行内 `ROW_GAP` 对齐 20
- 注：该版基于"容器 gap 机制正常"的误判结论（把卡内 sparkline 区域 56px 高度
  误读为卡间分离带），数值调整未能解决黏连 —— 真根因与修复见 v2.10.4

## [v2.10.2] - 2026-09-07
### 修复（PATCH：切页后主内容区空白 + 主内容区滚轮失效 —— 布局与滚动机制双根因）
- **修复一：侧边栏切换页面后主显示区空白（全部功能组件丢失）**
  - 根因：v2.10.1 的滚动修复在 `flex_1` 包装层内嵌套 `absolute inset-0` 装载页面，
    taffy 0.9 对该形态的 inset 解析失败 → 页面零尺寸、主区只剩背景
    （真机 TOPMOST 实屏抓取实证：基线与切页后主区均无内容像素）
  - 修复：改用 v2.8.5 日志面板同款模式 —— main 自身 `h_full + relative`（definite
    高度），topbar 留在流内，页面挂载到 `absolute top(TOP_BAR_HEIGHT) bottom_0
    left_0 right_0` 区域；`shell.rs` 内写入三种挂载模式的教训注释
  - 验证（非空断言）：硬件信息 124,019 / 清理优化 64,983 / 关于 49,475 surface
    像素，硬件信息 vs 清理优化差异 79,528 —— 切页渲染全部恢复
- **修复二：主内容区内容超高后滚轮无法滚动（从始至终未工作过）**
  - 根因（滚轮处理器插桩日志实证）：GPUI 0.2 的 `overflow_y_scroll` 需
    `track_scroll(&ScrollHandle)` 绑定才有滚轮驱动，且滚轮改写偏移后不会自动重绘；
    page_root 此前两者皆缺。插桩确认：绑定后事件到达、`max_off=404px`、offset
    每档移动 78px，仅缺实体 notify 重绘（`window.refresh()` 时机不对无效）
  - 修复：page_root 增加 `track_scroll` + `on_scroll_wheel`（事件后实体 notify）；
    页面视图各持有 `ScrollHandle` 并传入
  - 验证：硬件信息页（内容 1265px > 视口 861px，max_off=404）下滚 8 档同位差异
    **27,646 像素** —— 滚动生效；清理优化页内容不足一屏无滚余量，滚轮无位移为
    正确行为（此前的"0 差异"部分为测量页面选择不当所致，已甄别）
- **验证**：`cargo check` 零警告；`cargo test` 89 通过 0 失败；真机交互链路
  （点击导航 → 页面创建 → 渲染 → 滚动）端到端像素级回归通过

## [v2.10.1] - 2026-09-07
### 修复（PATCH：主内容区滚动失效 —— taffy min-content 撑爆滚动容器）
- **根因（滚轮消息注入 + 帧差分实证复现）**：`shell.rs::render_main` 的页面包装层
  为 `flex_1`，taffy 中其 min-content 高度被页面内容撑爆（`min_h(0)` 无法压制，
  v2.8.5 同族缺陷）→ 页面滚动容器高度恒等于内容全高 → `max_offset` 恒 0 →
  内容超出视口后滚轮无效（10 档注入前后主区差异仅 320 像素 = 实时数据噪声）
- **修复**：`render_main` 改用 v2.8.5 已验证的显式高度链 —— `relative` 包裹 +
  `absolute inset-0` 装载页面，页面根容器获得确定的视口高度，内容超界后正常滚动
- **回归验证**：修复后注入 10 档滚轮，前后帧 131,250 个采样点与"内容上移 40px"
  精确匹配、同位置差异 0 —— 滚动生效
### 优化（容器纵向间距节奏统一收紧）
- 页级纵向节奏 `PAGE_GAP` 18 → 16；卡片头内边距 13 → 12；卡片体 14/10 → 12/8，
  构成统一 **16（页）/12（卡头）/8（卡体）** 间距节奏
- 卡内趋势图标签与图表成组（组内 4px），消除"标签悬浮"的松散感；
  dashboard 行内横向间距与页级纵向节奏对齐（16px）

## [v2.10.0] - 2026-09-07
### 修复（硬件信息页卡片容器全部丢失 —— taffy grid 布局缺陷）
- **根因（PrintWindow 清洁截图像素级实证）**：gpui 0.2.2 (taffy 0.9) 的
  `.grid().grid_cols(2)` 网格子树在 `overflow_y_scroll` 滚动容器内**不产出可渲染
  布局**——页头/背景正常渲染，但网格子树（卡片 + 其后内容）零像素（surface 色
  `0x252527` 面积为 0）。首次截图中曾误判为渲染正常，实为 `CopyFromScreen` 抓取
  失效（桌面存在 13 个重叠窗口，DirectComposition 内容抓取被遮挡），换用
  `PrintWindow(PW_RENDERFULLCONTENT)` 后取得干净证据
- **修复**：硬件信息/环境检测/AI 环境三页全部弃用 `.grid()`，改用 flex 等宽两列
  行布局（全应用已验证渲染路径均为 flex）；`ui/page.rs` 明确禁 grid 布局纪律
- **回归验证**：修复后同口径像素统计 surface 面积 0 → 125,903，3 行×2 列卡片
  全部渲染

### 新增（MINOR：硬件监测功能补齐 + 趋势历史持久化）
- **每秒轮询传感器**：保留 SensorService 1s 快照链路（CPU 占用/频率/温度、内存、
  磁盘），并同拍拉取趋势窗口
- **60 秒趋势图（`ui::page::sparkline` 柱状趋势构件）**：CPU/GPU/内存占用趋势 +
  下载/上传速率趋势（总量口径），每秒采样、60s 滚动窗口
- **趋势历史持久化（新增 `secm-core::sensor_history` 模块）**：专职 1s 采样线程 +
  有界序列（主序列 1800 点），JSON 原子落盘
  `%LOCALAPPDATA%\SECM\cache\sensor_history.json`（脏后 10s 落盘 + on_app_quit
  flush 兜底），应用重启后趋势自动恢复；单网卡序列内存态随 UI 采样积累
- **网络流量卡**：总量/各网卡数据源切换；**0.5s–5s 可调采样间隔**（新增
  `netif::if_bytes_map` —— GetIfTable2 累计字节差分，无 PDH ≥1s 间隔限制）；
  **活跃 TCP 连接数**（新增 `net_io::tcp_connection_count` —— GetExtendedTcpTable
  统计 ESTABLISHED）；链路协商速度展示；各网卡实时上下行速率行
- **磁盘存储 + SMART 健康卡**：物理盘枚举 + 型号/容量 + SMART 健康三级状态
  （正常绿/风险关注黄/告警红）+ 温度（NVMe 健康日志 → WMI → ATA 194/190 降级链）
  + 劣化前兆黄色警示（NVMe 寿命 ≥80%、媒体错误、备用空间逼近阈值）
- **验证**：`cargo check` 零警告；`cargo test` 89 通过 0 失败（含新增
  sensor_history 窗口过滤/容量裁剪单测）；真机截图像素回归通过

## [v2.9.0] - 2026-09-07
### 新增（MINOR：统一主内容区布局框架 + 全页面现代化改版 + 明暗主题全链路联动）
- **统一页面布局框架 `ui::page`（新增模块）**：左侧边栏全部页面的主内容区收敛到单一装配
  入口，布局/间距/圆角/描边全应用统一节奏——
  - 骨架：`page_root`（根容器/纵向滚动，id 入参返回 Stateful）、`page_header`
    （页头：标题 + 副标题 + 右侧动作区）
  - 卡片：`card` / `card_header` / `card_header_accent` / `card_divider` / `card_body`
  - 数据表：`table_head` / `table_row` / `table_empty`（`ColWidth::Flex/Px` 列宽规格化，
    修复原表头全 flex 与数据行固定宽错位问题）
  - 反馈与控件：`banner`（Info/Success/Warn/Danger 软底语义状态条）、
    `button` / `button_sm`（Primary/Secondary/Ghost/Danger/Warning 语义按钮，h32/h24）、
    `kv_row_w` / `status_pill` / `badge` / `section_title` / `field_label` / `metric_value`
- **主题全链路联动**：页面内容区此前硬编码深色（`Theme::dark()` 不随壳切换，浅色模式下
  内容区仍是深色），现全部迁移至 `pi_clone::theme::Palette`（明暗双套）；壳
  `PiShell::toggle_theme` 向全部已实例化页面实体同步外观（新增 `set_appearance` 联动口），
  懒加载页构造时取当前外观；旧 `theme.rs` 模块及 Theme 色板删除，色值全部语义化
- **10 页全量改版**（业务逻辑/后台任务/元素 id/交互行为逐行保留，仅呈现层重构）：
  硬件信息、系统设置、服务管理、清理优化、网络诊断、网络配置、环境检测、AI 环境、
  硬件检测、关于——统一页头（标题+副标题+右侧动作/状态徽标）、统一卡片体系、
  清除全部硬编码业务色（rgb(0x…) → 语义色板）、状态消息统一 banner、
  行内操作按钮统一 button_sm 尺寸语义
- **验证**：`cargo check --workspace` 零警告；`cargo test --workspace` 87 通过 / 0 失败

## [v2.8.5] - 2026-09-07
### 修复（PATCH：日志流滚动条消失 + 滚轮拖动全失灵 —— 布局高度链断链）
- **根因（真实壳逐层实测定位）**：GPUI 0.2 (taffy) 纵向 flex 布局中，`flex_1`
  子项会被内容 min-content 高度撑爆——`min_h(0)` / `min_h(1px)` /
  `flex_basis(0px)` 均无法压制该约束。日志流容器 `pi-log-stream` 因此高度恒等
  于内容高度（实测 7575px == 200 行内容高），`max_offset` 恒 0、永不溢出：
  - 滚轮：clamp 区间 [0,0] → 滚动无效
  - 滚动条：`scrollable=false`（v2.8.4 显隐改由 scrollable 控制后直接消失；
    v2.8.2 时代则是"显示但拖动早退"）
  - 拖动：无溢出可滚
  该布局缺陷是 v2.8.0 以来全部滚动失灵的最底层根因（v2.8.4 的符号修复仍必要，
  但被布局断链掩盖）
- **修复**：显式高度链替代 flex_1 弹性链——
  - `pi-log-panel-content` 加 `relative()`（定位上下文）
  - `pi-log-stream-wrap` 改 `absolute top(48) bottom_0 left_0 right_0` 铺满
    header 以下区域（definite 高度）；header 高度抽 `LOG_PANEL_HEADER_HEIGHT`
    常量对齐
  - `pi-log-stream` 改 `h_full + flex_none` 显式占满 wrap
- **验证**：真实 PiShell 壳内实测——修复前 viewport_h=7575/max_off=0/不可滚；
  修复后 viewport_h=852（900−48 header）、max_off=6723（200 行真实溢出）、
  thumb 正常渲染（h=96）、程序化拖动换算被真实 ScrollHandle 精确采纳
  （off_y 0→-1700、thumb_top 实时跟随）
- 注：v2.8.3/v2.8.4 为中间尝试（notify / 符号语义），均被本版布局修复收编

## [v2.8.4] - 2026-09-07
### 修复（PATCH：日志流滚动条完全失灵 —— thumb 显示 + 拖动 + 滚轮）
- **根因（实测 probe 定位）**：项目把 GPUI `ScrollHandle::max_offset()` 的语义
  用反了。GPUI 0.2 中 `max_offset()` 返回 **≥0 的正可滚动量**（内容高 − 视口高），
  `offset().y` 取值区间为 `[-max_offset, 0]`（下滚为负）。而 v2.8.0-2.8.3 三处
  滚动换算（`scrollbar_geometry`、`log_sb_drag_move`、`log_sb_thumb_down`）全部
  假设 `max_offset() < 0` 才可滚、`≥0` 直接早退返回 —— max_offset 恒为正，导致：
  - thumb 几何恒 `(0,0,false)`，滚动条 thumb 永不渲染/拖动换算在早退处
    直接 return，`set_offset` 从不执行 —— "有侧边滚动条但鼠标按住拉动完全
    无反应、实时输出也无法滚动" 的直接原因
  - 上一版 v2.8.3 仅补 `cx.notify()`（set_offset 后需重绘才上屏，次要因素），
    未触及符号根因，故无效
- **修复**：
  - 新增 `pi_clone/scroll_math.rs`：把 thumb 几何 / 拖动换算 / offset 占比抽成
    纯函数，全部按 GPUI 真实语义（max_off ≥ 0、offset ∈ [-max,0]）实现
  - `right_panel.rs::scrollbar_geometry` / `shell.rs::log_sb_drag_move` /
    `log_sb_thumb_down` 改用共用纯函数；拖动路径保留每帧 `cx.notify()`
  - 滚动条显隐改由 `scrollable`（内容是否真超高）控制，行数 >3 不再作为依据
- **验证**：真窗口渲染 probe 实测 —— 120 行内容 max_off=2280 时 thumb 正常渲染
  且可拖；拖动 +60px → offset=-288、+240px → offset=-1440，thumb 实时跟随；
  实时追加 200 行后已下滚位置不被拉回顶（max_off 重算、thumb 重排）
- 注：v2.8.3 为中间尝试（仅补 notify），未发布即在本版合并修正

## [v2.8.2] - 2026-09-06
### 修复（PATCH：日志流真正可滚动 + 滚动条可见贴右缘）
- **日志无法滚动根因一（内容被压缩）**：滚动容器子行 GPUI 默认 `flex_shrink=1`，
  日志行被压缩适配容器高度 → 内容不溢出 → 永远滚不动。行元素加 `.flex_shrink_0()`
  保留自然高度，内容超高即溢出可滚
- **日志无法滚动根因二（实时流被强制拉回顶部）**：每 500ms 拉取新日志都调
  `scroll_to_top_of_item(0)`，用户往下查看旧日志时被不断拽回顶部 → 感觉"滚不了"。
  改为跟随策略：仅当用户停留顶部（offset≥-4px）才置顶新日志，已向下滚动则不打断
- **滚动条可见性**：thumb 改常显高对比 solid 灰（rgb 0x6e6e73），不再用半透
  scroll_thumb（叠底后近不可见）；track hover 高亮；行 <3 隐藏
- **滚动条贴最右**：自绘 scrollbar absolute 于日志流容器 right_0（容器 flex_1 贴
  右侧栏内容区右缘），位于面板最右
- 滚轮滚动后 on_scroll_wheel → cx.notify 重绘，thumb 位置实时跟随 offset

## [v2.8.1] - 2026-09-06
### 修复（PATCH：右侧日志流自绘滚动条）
- **根因**：GPUI 0.2 Windows 平台不绘制任何滚动条（`scrollbar_width` 仅预留布局
  空间、无渲染实现）——上一版设置的 8px scrollbar_width 只留白、滚动条仍不可见
- **修复**：日志流容器右侧叠加**自绘滚动条**（absolute track + thumb）：
  - thumb 高度/位置由 `ScrollHandle` 的 bounds/max_offset/offset 实时换算
  - thumb 可拖动（记录按下点 → 全局 move 换算 offset → set_offset），
    滚动条 thumb 拖动走分隔条同款 shell 根 on_mouse_move/up 全局跟踪
  - 颜色用 palette.scroll_thumb，thumb 圆角胶囊样式
- 日志行数 <3 时滚动条自动隐藏

## [v2.8.0] - 2026-09-06
### 修复+变更（MINOR：日志滚动条/最新优先排序 + 全链路行为打点补全）
- **日志流显示滚动条**：GPUI scrollbar_width 默认 0（Scroll 等同 Hidden、滚动条不可见），
  现设为 8px → 右侧日志流出现滚动条可拖动
- **日志排序改为最新在第一条**：渲染倒序（新→旧），新日志到达自动置顶跟随；
  容量裁剪保留最新
- **全链路行为/返回信息打点补全（不豁免）**：
  - AI 环境：安装/升级/卸载 AI 工具与 MCP 服务器补 触发 + 结果（成功消息/失败原因）
    —— 此前完全无日志
  - 硬件检测：磁盘枚举完成补记枚举块数（此前只记触发无结果）
  - 审计确认：清理优化/网络诊断/网络配置/系统设置/服务管理/环境检测/硬件信息
    各页按钮触发与后台返回结果均已逐操作打点（cleanup 10 / net_config 20 /
    settings 9 / network 3 / services 4 / environment 5 等），全部汇入右栏日志流

## [v2.7.0] - 2026-09-06
### 变更（MINOR：统一产品名 SysEnv Console Manager + 补齐窗口标题/图标）
- **产品名统一全称 SysEnv Console Manager**：侧栏品牌行、Main 顶栏标题、
  关于页名称与版权署名、窗口标题、托盘 tooltip 全部用全称（SECM 简写仅保留于
  工程内部命名 secm-app/core/datasource 与存储目录）
- **补齐任务栏/窗口标题**：此前无 titlebar 配置导致任务栏与 Alt-Tab 无标题；
  现 WindowOptions.titlebar.title = "SysEnv Console Manager"（appears_transparent
  保持自绘无边框）——实测 GetWindowText = 全称
- **右栏标题图标增强**：日志流标题 Terminal 图标提升为 accent 高亮色

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
