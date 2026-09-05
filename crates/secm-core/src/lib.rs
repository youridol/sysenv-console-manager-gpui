// secm-core — SECM 业务逻辑层（纯 Rust，无 UI/GPUI 依赖）
// 模块结构（ADR-0002/0006）：采集编排、系统操作、日志；数据契约类型集中于此。
// 阶段进度：v2.0.0 骨架 → Phase 1 起逐模块迁入。

pub mod cleanup;
pub mod error;
pub mod lhm;
pub mod logger;
pub mod network;
pub mod sensor;
pub mod sensor_service;
pub mod settings;
