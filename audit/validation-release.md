# 验证矩阵、发布门禁与最终审计（ADR-0010）

执行日期：2026-07-15；环境：Windows 11 Pro 25H2（Build 26200）/ x64。

## 1. 编译门禁（全绿）

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ 通过（先 `cargo fmt --all` 统一全仓库格式，独立 chore 提交 `7c5c7fb`） |
| `cargo check --workspace` | ✅ 通过（0 error） |
| `cargo build --workspace` | ✅ 通过（22.65s，0 warning 于任务文件） |
| `cargo test --workspace` | ✅ **96 passed / 0 failed**（core 51 + datasource 45 + app 0；2+6 个 `#[ignore]` 真机测试另跑） |
| `cargo clippy --workspace --all-targets` | ⚠ 仅既有代码警告（项目无 clippy 配置/`-D warnings` 约定，ADR-0010 条件触发条款不适用；任务新增代码 clippy 干净） |
| `dotnet build sidecar-lhm/sidecar-lhm.csproj -c Release` | ✅ 通过（2 个既有 CA 警告，非任务引入） |
| `dotnet publish sidecar-lhm -r win-x64` | ✅ 通过（真机部署验证用） |

## 2. 指标级功能验证（真机，hw_verify 工具）

详见 `audit/non-admin-verification.md` 验证矩阵。要点：

- **管理员 + sidecar（提权）**：CPU Load 20.1%[perfcounter]、Temp 66.8°C[lhm]、Clock 4426MHz[perfcounter]、Power 37.4W、Voltage 0.6V；GPU 2080Ti 全域（17%/59°C/2025MHz/121W/VRAM 2.8/11GB/风扇）；主板系统温度 54.5°C、CPU 风扇 1652RPM、机箱风扇 1185RPM；SPD 实际型号 A-DATA AX5U6400；7 块物理盘温度 36–50°C[lhm]；内存 31.6GB/83%；IP/TCP/网速真实。
- **一致性测试（ADR-0010 §一致性）**：
  1. 同一指标无双来源：NET 速率唯一 GetIfTable2 差分（net_io.rs 已删除）；CPU Load 唯一 PDH 链（sysinfo 仅回退层且 source 标注）；CPU 频率唯一 cpu_freq 链（sysinfo 回退已移除）。✅
  2. 缓存失效对象：网络差分计数器回绕/消失实例自动丢拍；lhm 快照 2s TTL + 存活由 health/HTTP 错误驱动。✅（代码级保障；GPU 拔插重载由 sidecar 采集异常 → Close → 重 Open 覆盖）
  3. sidecar 掉线无假数据：`available:false` → 全 LHM 域 `Metric.value=None`（实测：SECM_DISABLE_LHM 与降权场景）。✅
  4. 重载期间 UI 不崩溃：UI 只读快照（Mutex 克隆），采集线程独立；sidecar HTTP 有 2s 超时 + 5s 退避，不冻结 1s 采集线程（ensure 派发独立线程）。✅
  5. 高权限失败不伪造 0/默认值：UAC 取消实测返回 `available:false` + 错误；全 LHM 域 null。✅
- **性能门禁**：UI 线程零硬件扫描（快照读）；无每帧 LHM 全树扫描（sidecar 1s 采集线程独立）；网络差分唯一采集点（sensor_service）；sidecar HTTP 受 2s 超时 + 5s 退避保护。✅

## 3. 平台验证状态（如实记录）

| 平台 × 权限 | 状态 |
| --- | --- |
| Windows 11 25H2 管理员 | ✅ 已验证 |
| Windows 11 普通用户（降权 token） | ✅ 已验证 |
| Windows 11 UAC 取消 | ✅ 已验证 |
| Windows 10（普通用户/管理员） | **PARTIAL：未验证**（无 Win10 设备；代码层 Win32 API 均为 Win8+/Win10 可用面，PDH Processor Information 为 LiteMonitor 同源要求） |

## 4. 旧数据源残留最终审计（逐项）

| 检查项 | 结果 |
| --- | --- |
| 旧数据源残留 | `net_io.rs` 已删除（PDH Network Interface）；`sensor_service` 的 sysinfo 频率回退已移除；grep 无 `get_net_io_speed_map` 残留 |
| 双重来源 | 每指标唯一 authoritative source（audit/target-source-inventory.csv 归属） |
| 死代码 | net_io 删除；sensor_match/sensor_service 无死代码（clippy dead_code 零告警）；UpdateVisitor/speed_map 等被替换物已清理 |
| 死依赖 | 无 crate 级死依赖（wmi 留给非硬件 activation/SMART 兜底） |
| 重复采集 | 网络差分单一化（sensor_history 不再独立触达 GetIfTable2）；LHM 快照单一缓存（2s TTL） |
| 重复缓存 | 无新增重复缓存（PDH 进程级单例 ×4 各自唯一） |
| UI 直连 datasource | dashboard 直调点（L963-965）已改为纯快照消费 |
| 错误吞掉 | 全路径 log + Metric.error + diag；sidecar 错误文件日志 |
| 默认值污染 | LiteMonitor 三大假值（0f/2500MHz/16GB）未迁移；实测 unavailable 路径 |

## 5. 提交链（Phase 独立可回滚）

```text
7c5c7fb chore(fmt): cargo fmt 全仓库统一格式
09cf0f4 feat(hardware): ADR-0006 数据源层 LiteMonitor 等价能力（含 Phase 5 下线 net_io）
b3b0b5a feat(hardware): ADR-0005/0006 统一 HardwareSnapshot v2 + sidecar 契约 v3
09e33c2 fix(hardware): core::netif tcp_connection_count 改指 datasource::netif
e10644b fix(hardware): 真机验证修复（位域过滤/Storage 首刷/SPD/storage_temps/降级开关/验证工具）
```

## 6. 发布门禁结论

- 完整来源映射完成 ✅；旧默认来源全部下线或转为回退层 ✅
- 普通用户启动/运行验证通过 ✅（Win11）；高权限限制真实记录 ✅
- cargo check/build/test 通过 ✅；sidecar 构建通过 ✅
- Win10 验证 **PARTIAL**（未执行）；许可证/依赖清单完成 ✅
- 结论：**DONE 条件在 Win11 全绿；Win10 项如实标记为未验证，不宣称全覆盖。**
