// secm-core — SECM 业务逻辑层（纯 Rust，无 UI/GPUI 依赖）
// 模块结构（ADR-0002/0006；v3.0.0 纯原生零 HTTP）：采集编排、系统操作、日志；数据契约类型集中于此。
// v3.0.0 移除：lhm（sidecar HTTP 客户端）、sensor_match（FanMapper/MOBO.Temp——随 sidecar 域下线）。

pub mod cleanup;
pub mod environment;
pub mod error;
pub mod game_env;
pub mod hardware;
// 高精度计时器（bcdedit 三条目；系统设置页「系统类」开关）
pub mod hpt;
pub mod logger;
pub mod net_config;
pub mod net_diag;
pub mod net_info;
pub mod netif;
pub mod network;
// NVIDIA 显卡电源管理模式（NVAPI DRS；系统设置页三档选择）
pub mod nvidia_drs;
pub mod proc_util;
pub mod sensor;
pub mod sensor_history;
pub mod sensor_service;
pub mod settings;
pub mod sysinfo;
