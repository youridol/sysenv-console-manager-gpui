# secm-app GPUI 主线程阻塞调用点审计报告

- 审计范围：`crates/secm-app/src/`（app.rs、main.rs、tray.rs、pages/*.rs、ui/*）→ secm-core / secm-datasource 同步函数
- 审计基线：**git HEAD `7a2375a`**（工作区干净）
- 审计时间：2026-09-05
- 结论速览：**A 类 5 处、B 类 7 处、C 类 0 处、D 类（低危说明）3 处**

> ⚠️ 重要时间线说明：审计进行期间，仓库 HEAD 从 `2f022b3` 推进到 `7a2375a`
> （`refactor: Environment/AiEnvironment 页并发化`），本任务点名重点检查的
> `EnvironmentView::new` / `AiEnvironmentView::new` 同步阻塞问题已在该提交中修复
> （见 F1/F2，标记"已OK"）。其余页面不受该提交影响，本报告逐行号基于 HEAD `7a2375a`。

---

## 一、汇总表

| # | 位置（文件:行） | 类别 | 阻塞内容 | 量级 | 当前状态 |
|---|----------------|------|----------|------|----------|
| F1 | pages/environment.rs:43-58（new）/ 61-96（start_static_load）/ 99-124（run_ai_check） | A | 构造时同步调 get_system_info/check_directx/check_vc_runtimes/get_game_presets/check_ai_tools（注册表/WMI/PS 回退/npm 进程） | 秒级（旧代码）；现 0 | **已OK**（7a2375a 起全部 cx.spawn + background_executor，主线程零阻塞） |
| F2 | pages/ai_environment.rs:62-81（new）/ 110-149 / 156-238 | A | 构造时 npm/工具/MCP/扩展检测、安装/卸载命令 | 秒级（旧代码）；现 0 | **已OK**（同上，per-section 独立后台任务 + WeakEntity 回填） |
| F3 | pages/settings.rs:78-88（new，且 86 再调 refresh） | A | 5 个开关注册表读 + get_power_plans（注册表枚举+活动方案）+ get_hetero_policies，**new 内完整算两遍** | 每遍 ~5–50ms，合计 ~10–100ms | **未后台** — AppRoot::new 全量构造时在主线程同步执行 |
| F4 | pages/services.rs:18-26（new，24 再调 refresh） | A | settings::list_all_services() = SCM 全量服务枚举，**new 内枚举两遍** | 单遍 ~20–150ms，两遍 ~40–300ms | **未后台** |
| F5 | pages/cleanup.rs:62-72（new，70 再调 refresh_procs） | A | cleanup::list_processes() = sysinfo System::new_all+refresh_all 全进程枚举，**new 内两遍** | 单遍 ~20–200ms，两遍 ~40–400ms | **未后台** |
| F6 | pages/hardware.rs:22-31（new，29 再调 refresh_disks） | A | hardware::list_disks() = IOCTL 打开 \\.\PhysicalDriveN(0..64) + 查询；失败回退 WMI 枚举，**new 内两遍** | 单遍 ~5–200ms（卡盘/休眠盘可达秒级；WMI 回退 100ms–1s+），两遍翻倍 | **未后台**（仅 read_smart 已后台） |
| F7 | pages/net_config.rs:34-81（new:60） | A | netif::list_adapters()（GetAdaptersAddresses）+ is_admin()，较廉价 | ~1–10ms | 低危（可接受，建议并入懒加载） |
| F8 | pages/net_config.rs:129-156 set_dhcp → net_config::apply_network_config（152）；按钮 563-565 | B | **主线程同步 spawn 多个 netsh.exe 子进程**（set address dhcp + set dns dhcp + set dnsservers dhcp…），Command::output() 逐个阻塞等待 | 每个 netsh ~100–500ms，串行 2–4 个 → **0.2–2s+ 冻结** | **未后台**（同页 apply_mac 已后台，唯独 DHCP/静态未包） |
| F9 | pages/net_config.rs:159-210 apply_static_v4 → apply_network_config（207）；按钮 658-660 | B | 同上，静态 v4 = 1 主地址 + N 附加地址 + DNS 主/附加 = 串行 2–6 个 netsh | **0.3–3s+ 冻结** | **未后台** |
| F10 | pages/net_config.rs:213-242 apply_static_v6 → apply_network_config（239）；按钮 725-727 | B | 同上，IPv6 add/set address + 删/增路由 + DNS = 串行 3–7 个 netsh | **0.3–3.5s+ 冻结** | **未后台** |
| F11 | pages/services.rs:54-73 op_service（按钮 302-304） | B | start/stop/set_start_type（SCM API，ms 级）+ 随后 refresh() 全量服务枚举（20–150ms） | 单次点击 ~30–200ms | **未后台** |
| F12 | pages/settings.rs:111-118 toggle / 121-127 activate_plan / 130-141 set_hetero / 144-153 import_ultimate（按钮 507-509 / 328-331 / 473-477 / 298-300） | B | 注册表写 + SPI_SETMOUSE(广播) / PowerSetActiveScheme / PowerWriteAC/DC + duplicate_scheme，随后 refresh() 又重读 5 开关+计划+hetero | 单次 ~10–100ms（含系统广播） | **未后台**（每次点击主线程全链路同步） |
| F13 | pages/hardware.rs:33-43 refresh_disks/refresh（按钮 129-131） | B | 重新 list_disks() 磁盘 IOCTL 全枚举（同 F6） | ~5–200ms+ | **未后台** |
| F14 | pages/cleanup.rs:74-77 refresh_procs（按钮 367-373"刷新进程列表"） | B | 重新 list_processes() 全进程枚举 | ~20–200ms | **未后台** |
| F15 | pages/cleanup.rs:79-87 flush_dns / 115-123 set_prio | B | DnsFlushResolverCache / OpenProcess+SetPriorityClass | ~1–5ms | 低危（毫秒级，可暂不改） |
| F16 | pages/dashboard.rs:19-26 new + 28-45 schedule_refresh（snapshot 于 22、33） | D | SensorService 全局 OnceLock + parking_lot Mutex<SensorSnapshot> clone；采集重活在 sensor-service 后台线程 | 锁+clone ~µs | **已OK**（仅快照克隆上主线程，µs 级） |
| F17 | pages/logs.rs:20-29 new + 31-48 schedule_refresh（get_all 于 36） | D | LogBuffer::global() parking_lot 锁 + 200 条 Vec clone（500ms/次，主线程） | ~10–100µs | 已OK（低危；渲染里另有同量级过滤） |
| F18 | secm-core lhm STATE / SNAP_CACHE / environment NPM_PREFIX(OnceLock) | D | 主线程均不触碰：lhm 仅 sensor-service 后台线程调用（HTTP 2s 超时在后台）；NPM_PREFIX 初始化跑 `npm prefix -g` 仅出现在已后台化的检测路径 | — | 已OK |
| F19 | app.rs:88-116 AppRoot::new | A(汇总) | 11 个页面 View 全部在启动主线程构造：F3+F4+F5+F6+F7 的同步工作**串在一起**、且每页构造后立刻又重复 refresh 一遍 | 累计 **0.5–2s+ 启动白屏/无响应** | **未后台**（根因：全量构造 + 构造内重复同步枚举） |
| F20 | pages/*.rs render()（environment/settings/services/cleanup/net_config/hardware…） | C | 逐帧仅克隆已缓存字段、构建 div；**未发现 render 内直接调注册表/进程/网络/磁盘** | — | **0 处**（各页每帧大 Vec clone 属 CPU 开销非阻塞 IO，不在本次范围） |

---

## 二、最严重的 5 处

1. **F8/F9/F10 — NetConfig 三按钮主线程串行跑多个 netsh**（net_config.rs:152/207/239）
   点击"切换为 DHCP / 应用静态 IPv4+DNS / 应用静态 IPv6"时，`apply_network_config`
   在点击 handler 内同步 spawn 2–7 个 `netsh.exe` 并逐个 `.output()` 等待，
   单次 **0.2–3.5 秒 UI 完全冻结**。页面头注释宣称"netsh 后台执行"，实际只有
   apply_mac 包了后台。→ **复用同页 apply_mac 的 cx.spawn + background_executor 模式**，
   handler 内 clone 请求后 spawn，`exec.spawn(async move { net_config::apply_network_config(&req) })`，
   完成后 WeakEntity 回 UI 填 steps/status + notify。

2. **F19 + F6 — AppRoot::new 全量构造，HardwareView 同步磁盘枚举且 ×2**
   所有 View 在 app.rs:90-100 于主线程一次建完；HardwareView::new（hardware.rs:24）
   同步开 64 个物理盘 IOCTL（失败再 WMI），随后 :29 又 refresh_disks 重枚举一遍，
   最坏 **数百 ms–1s+**；叠加 F3/F4/F5 后启动无响应可达秒级。
   → 构造时置空列表并 `cx.spawn` 后台枚举（仿已提交的 environment.rs start_static_load /
   hardware.rs read_smart 写法），完成后 WeakEntity 回填 + notify；去掉构造后的重复 refresh。

3. **F4 — ServicesView 服务全量枚举在主线程且 ×2**（services.rs:20 + :24）
   SCM EnumServicesStatusExW 全量 + 逐服务启动类型，单遍 20–150ms，双击两遍。
   启动瞬间卡 UI；点启停后 op_service 末尾又 refresh 一遍（F11）。
   → new 只置空，后台枚举回填；op 完成后再在后台重枚举一次回填（可加短暂防抖）。

4. **F5 — CleanupView 全进程 sysinfo 枚举在主线程且 ×2**（cleanup.rs:64 + :70）
   System::new_all + refresh_all 全进程内存读取 20–200ms/遍。
   → 同 F4：后台枚举回填，点"刷新进程列表"按钮同样走后台。

5. **F3/F12 — SettingsView 开关/电源计划注册表链路全程主线程**（settings.rs:78-88 与各 handler）
   每个 toggle 点击 = 读 + 写 + SPI/电源 API（可能系统广播）+ refresh 整页重读 7 项；
   注册表单项虽 ~ms 级，链路长、new 时还整页算两遍，量变引起启动/操作卡顿。
   → 读路径整包（load_toggles+plans+hetero）放后台一次性回填（同 environment.rs
   StaticEnvData 模式）；写路径 set 动作放后台，完成后后台重读回填。

---

## 三、逐处修复建议速查（一句话）

| # | 建议 |
|---|------|
| F3/F4/F5/F6/F19 | 页面 new() 只填占位/空状态，把"枚举/检测"整体放进 `cx.spawn` → `background_executor.spawn` → `WeakEntity` 回 UI 更新 + notify（照抄 environment.rs:61-96 或 hardware.rs:46-79 既有写法）；删除构造后立即重复的 refresh 调用 |
| F8/F9/F10 | 点击 handler 里 `apply_network_config(&req)` 包 `exec.spawn(async move { … }).await`，其余照 apply_mac（net_config.rs:262-297） |
| F11 | op_service 的 SCM 调用 + 随后的 list_all_services 都搬后台，回 UI 只赋状态 |
| F12 | toggle/activate/set_hetero/import 的写调用与 refresh 重读整体后台化，按钮加 busy 态防连点 |
| F13/F14 | 按钮 handler 内 list_disks()/list_processes() 包 background_executor，回 UI 赋值 |
| F15 | 毫秒级，可接受；如顺手可后台化 |
| F16/F17/F18 | 无阻塞风险，保持现状即可（若追求极致可把 1s/500ms 循环里的 clone 移到后台线程再跨线程回传，非必需） |
| F20 | 无 C 类；留意各 render 每帧大 Vec clone 属 CPU 开销，可视卡顿另行优化（本次不涉及） |

---

## 四、分类计数与结论

- **A 类（构造/启动路径同步阻塞）：5 处**（F3、F4、F5、F6、F7，加汇总 F19）——
  全部源于 AppRoot::new 一次性构造全部页面；其中 4 处在构造内**重复执行同一枚举**，
  是本应用启动卡顿的首要根因。任务点名的 F1/F2 已在 HEAD 7a2375a 修复，不计入。
- **B 类（click/refresh 同步阻塞）：7 处**（F8–F14，F15 低危）——
  最重的是 NetConfig 三个 netsh 应用按钮（F8/F9/F10，秒级冻结）。
- **C 类（render 每帧阻塞）：0 处**。
- **D 类（全局锁/OnceLock 主线程等待）：3 组（F16/F17/F18），均低危已OK**；
  需保持"lhm 与 npm prefix 初始化只出现在后台线程"这条不变量。
- 建议优先级：**F8/F9/F10（秒级冻结）→ F19 启动路径（F3–F6 一起搬后台）→ F11/F13/F14**。

（本报告为只读审计产物，未改动任何源码。）

---

## 五、修复落实追踪（截至并发优化完成）

> 审计后所有 A/B 类阻塞点均已后台化（提交 7a2375a / e0efcf8 / 后续 net_config+hardware 提交）。

| # | 状态 | 修复提交/方式 |
|---|------|--------------|
| F1/F2 | ✅ 已修复 | 7a2375a：Environment/AiEnvironment 构造与检测全 cx.spawn + background_executor + WeakEntity 回填 |
| F3/F12 | ✅ 已修复 | e0efcf8：Settings 初始状态后台整包加载；toggle/激活计划/异类策略/导入卓越后台写 + 后台重读，op_busy 互斥 + 开关乐观 UI |
| F4/F11 | ✅ 已修复 | e0efcf8：Services 服务枚举后台加载；启停/启动类型后台执行 + 800ms 延迟后台刷新 |
| F5/F14/F15 | ✅ 已修复 | e0efcf8：Cleanup 进程枚举后台加载；DNS 刷新/优先级设置后台执行 |
| F6/F13 | ✅ 已修复 | 后续提交：Hardware 磁盘 IOCTL 枚举构造/刷新后台化，loading 态展示 |
| F7 | ✅ 已修复 | 同上：NetConfig 适配器枚举后台加载 |
| F8/F9/F10 | ✅ 已修复 | 同上：DHCP/静态 v4/静态 v6 三按钮 netsh 全部经 run_apply 后台执行（原 0.2-3.5s 主线程冻结），applying 互斥 + 延迟刷新 |
| F19 | ✅ 已修复 | 上列构造后台化后，AppRoot::new 11 页全量构造路径无同步阻塞 IO |
| F16/F17/F18 | ✅ 已核验 | 低危（µs 级锁+clone），保持不变；lhm/npm 初始化仅在后台线程的不变量成立 |
| F20 | ✅ 已核验 | C 类 0 处（render 无阻塞 IO） |

**收尾验证**：`cargo check --workspace` 零警告零错误；全量单测通过；debug/release 构建 + 启动冒烟确认窗口响应。
