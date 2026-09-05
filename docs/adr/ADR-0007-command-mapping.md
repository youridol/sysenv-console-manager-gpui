# ADR-0007：命令面 → Rust 函数面映射

- 状态：已校准（基于源 v1.19.0 全量 80 命令枚举 + 前端 invoke 面核查）
- 日期：2026-09-05

## 背景

原 80 条 Tauri 命令是 WebView 与 Rust 的 IPC 边界。GPUI 无 IPC——UI 与业务同进程，命令直接变为 **secm-core 的同步/异步函数调用**。这消除了序列化/反序列化层与前端契约层，但需保持原命令的语义边界（含权限检查、日志、取消、错误映射）以保证行为一致。

## 决策

### D1：命令 → 函数映射规则
| 原形态 | 映射 | 示例 |
|--------|------|------|
| 一次性读取（get_*） | `secm_core::…::read_xxx() -> Result<T, CoreError>` | get_sensor_data → sensor::snapshot() |
| 写操作（set_/apply_/clean_/start_/stop_） | `…::set_xxx(...) -> Result<…>`，**保留 is_admin 门禁** | set_hags_state / set_power_plan |
| 流式命令（*_stream） | `…::run_xxx(cancel: CancelToken, sink: impl Fn(StreamEvent))` | ping_stream / traceroute_stream |
| 取消 | `…::cancel(cmd_id)` | cancel_net_cmd → CancelRegistry |
| 无状态工具 | 纯函数 | check_directx / list_disks |

### D2：80 命令按页分组（草案，待全景精确化）
- **Dashboard**：get_sensor_data、driver_status、driver_install、get_app_version、get_chart_history（+ NetIface 侧 get_net_stats/get_link_speeds）
- **Cleanup**：flush_dns、clean_*_cache、clean_temp_files、trim_process_working_set、set_priority、list_processes、clean_shader_cache 族
- **Network**：ping/traceroute/nslookup/detect_nat_type/iperf3_stream、cancel_net_cmd、deep_check_dhcp_servers、check_dhcp_servers
- **NetConfig**：get_network_config、apply_network_config、set_dns、get/set_doh_config、set_network_mac
- **Settings**：get/set_hags_state、game_mode、game_optimization、mouse_precision、vrr、hpt、power_plans 族、hetero_policies、ultimate_performance、nvidia_power_mode
- **Services**：list_all_services、start/stop_service、set_service_start_type
- **Environment**：check_directx、check_vc_runtimes、check_ai_tools、install/uninstall_ai_tool、fetch_ai_latest_versions、check_npm_environment、mcp/extensions 族、get_game_presets、get_system_info
- **Hardware**：list_disks、get_disk_smart
- **Logs**：get_logs、clear_logs、export_logs、get_log_capacity、get_debug_logs、append_frontend_log（→ 废弃，前端日志已在本进程）
- **About**：get_app_version

### D3：错误模型与日志不变
- `CollectError`/`String` 错误 → `secm_core::error::CoreError`（保留 thiserror 分级 + 中文消息 + API 名/错误码）
- 用户日志/调试日志体系原样迁入，事件订阅供 Logs 页。

### D4：命令语义审计延续
- 保留 v1.18.2/1.19.0 安全修复：npm 白名单+格式校验、UTF-8 安全截断、无 shell 拼接、注册表写前备份、参数 argv 化。

## 后果
- IPC/序列化层删除 → 减少 ~30% 样板，消除契约漂移。
- 命令与 UI 直连要求线程纪律严格（ADR-0004）；所有阻塞调用必须经 BackgroundExecutor。
