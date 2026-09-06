//! secm-datasource — SECM 纯 Rust 驱动采集模块
//!
//! 替换原有 PowerShell / 外部命令（`sc` / `powercfg` / `wmic` / `ipconfig` / `curl`）
//! 数据源，消除外部进程依赖。
//!
//! 模块职责（全部为底层采集，零业务逻辑）：
//! - [`registry`]：注册表采集（补丁 P2 / GPU 厂商 P6 / 通用读写）
//! - [`service`]：服务管理（P10/P11/P12，advapi32）
//! - [`power`]：电源计划（P7/P8/P9，powrprof）
//! - [`netif`]：网络接口（P3 链路速度 / P4 本地 IP，iphlpapi）
//! - [`dns`]：DNS 缓存刷新（P13，dnsapi；由 secm-core::cleanup 调用）
//! - [`activation`]：Windows 激活状态（P1，注册表近似 + LicenseStatus 映射表）
//! - [`cpu_freq`]：CPU 频率降级链（NtAPI/PDH/注册表；由 secm-core::sensor_service 消费）
//! - [`disk`] / [`disk_io`]：磁盘枚举/SMART + 卷 IO 速率（PDH）
//! - [`error`]：统一错误模型 `CollectError`
//!
//! 依赖方向：本 crate 是叶子模块，仅依赖 windows-sys / winreg / wmi / serde；
//! 业务模块（sysinfo / settings / net_stats / ip_info / game_env / cleanup）依赖本 crate。
//! 禁止反向依赖。
//!
//! 线程模型：本 crate 全部为同步阻塞 API，调用方须在 `spawn_blocking` / `thread::scope`
//! 中执行（S8 异步并发多线程：GUI 主线程永不接触采集逻辑）。
//!
//! 历史注记（2026-09-05 审计）：原 `http` 模块（ureq + URL 白名单，P5 公网 IP）
//! 自 v2.0.0 迁移后无任何调用方，已随死依赖（serde_json/env_logger/ureq）一并移除；
//! 网络探测由 secm-core::network 自有 ureq 逻辑承担。

pub mod activation;
pub mod cpu_freq;
pub mod dns;
pub mod disk;
pub mod disk_io;
pub mod error;
pub mod net_io;
pub mod netif;
pub mod power;
pub mod registry;
pub mod service;

pub use error::CollectError;
