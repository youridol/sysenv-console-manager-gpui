//! GPU 原生采集 — NVML（NVIDIA 实时指标）+ DXGI（全厂商适配器枚举），零 HTTP、零提权
//!
//! 数据源与权限语义（v3.0.0 原生迁移，替代 LHM sidecar HTTP 链路）：
//! - NVIDIA 实时指标：NVML（nvml-wrapper **运行时 dlopen** 驱动自带的 nvml.dll）——
//!   温度/功耗/时钟/负载/显存，普通用户可读（NVIDIA 驱动用户态开放）；
//!   无 NVIDIA 驱动 → 整体不可用（优雅降级，不报错不重试）。
//! - 适配器枚举：DXGI `IDXGIFactory1::EnumAdapters1`（普通用户可读）——
//!   名称 + 专用显存总量覆盖全部厂商（NVIDIA/AMD/Intel/其他）。
//! - AMD/Intel 温度/功耗/时钟：无用户态公开 API（ADLX/IGCL 为厂商专有 C++ SDK，
//!   依赖专有运行时且无维护中的 Rust 绑定）→ 指标不可用，由上层
//!   `Metric::unavailable` 如实标注，**不伪造数值**。
//!
//! 合并策略：DXGI 列表为基准（保证 AMD/Intel 核显/独显的名称与显存可见），
//! NVIDIA 适配器按「名称归一匹配 → 剩余设备序位配对」关联 NVML 设备填充实时指标。
//!
//! 线程模型：NVML/DXGI 均为微秒-毫秒级同步调用，调用方在后台采集线程执行（S8）。

use serde::Serialize;
use std::sync::OnceLock;

use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::Nvml;

/// 单卡 GPU 采样（字段与 secm-core::sensor::GpuData 一一对应；None = 该指标无用户态来源）
#[derive(Debug, Clone, Default, Serialize)]
pub struct GpuSample {
    /// 适配器名（DXGI Description；NVIDIA 卡与 NVML 设备名一致）
    pub name: String,
    /// 核心温度 ℃（NVML；None = 非 NVIDIA 或 NVML 不可读）
    pub temperature_c: Option<f32>,
    /// 核心时钟 MHz（NVML）
    pub core_clock_mhz: Option<f32>,
    /// 功耗 W（NVML power_usage 毫瓦 → 瓦）
    pub power_w: Option<f32>,
    /// 核心负载 %（NVML utilization_rates.gpu）
    pub load_percent: Option<f32>,
    /// 显存已用（字节，NVML）
    pub memory_used_bytes: Option<u64>,
    /// 显存总量（字节；NVIDIA 取 NVML 值，其他厂商取 DXGI DedicatedVideoMemory）
    pub memory_total_bytes: Option<u64>,
    /// 风扇转速 RPM（NVML 仅提供风扇占空比百分比、非 RPM → 恒 None，不伪造）
    pub fan_rpm: Option<f32>,
}

/// NVML 单设备实时采样（内部结构）
#[derive(Debug, Clone, Default)]
struct NvmlDeviceSample {
    name: String,
    temperature_c: Option<f32>,
    core_clock_mhz: Option<f32>,
    power_w: Option<f32>,
    load_percent: Option<f32>,
    memory_used_bytes: Option<u64>,
    memory_total_bytes: Option<u64>,
}

/// DXGI 适配器静态信息（内部结构）
#[derive(Debug, Clone)]
struct DxgiAdapterInfo {
    name: String,
    dedicated_video_memory: u64,
}

// ============================================================================
// 公共 API
// ============================================================================

/// 采集全部 GPU（每拍调用；NVML 实时读 + DXGI 静态枚举合并）
///
/// 产出规则：
/// - DXGI 枚举成功：全厂商适配器逐卡输出（名称/显存总量），NVIDIA 卡叠加 NVML 指标；
/// - DXGI 失败（COM 异常，极罕见）：退化为纯 NVML 列表；
/// - 两者皆空：返回空数组（上层按「未检测到 GPU」处理）。
pub fn sample_gpus() -> Vec<GpuSample> {
    let nvml_devices = sample_nvml_devices();
    match dxgi_adapters() {
        Some(adapters) if !adapters.is_empty() => merge_adapters_with_nvml(adapters, nvml_devices),
        _ => {
            // DXGI 不可用：NVML 列表独立成卡（NVIDIA-only 环境兜底）
            nvml_devices
                .into_iter()
                .map(|d| GpuSample {
                    name: d.name,
                    temperature_c: d.temperature_c,
                    core_clock_mhz: d.core_clock_mhz,
                    power_w: d.power_w,
                    load_percent: d.load_percent,
                    memory_used_bytes: d.memory_used_bytes,
                    memory_total_bytes: d.memory_total_bytes,
                    fan_rpm: None,
                })
                .collect()
        }
    }
}

// ============================================================================
// NVML（NVIDIA 实时指标）
// ============================================================================

/// NVML 进程级单例（惰性初始化一次；失败永久降级，避免每拍重试开销）
fn nvml_instance() -> Option<&'static Nvml> {
    static NVML: OnceLock<Option<Nvml>> = OnceLock::new();
    NVML.get_or_init(|| match Nvml::init() {
        Ok(nvml) => {
            log::info!("GPU 采集 · NVML 初始化成功（NVIDIA 用户态指标可用）");
            Some(nvml)
        }
        Err(e) => {
            // 常见场景：无 NVIDIA 驱动 / 核显-only 机器——INFO 级即可，不算错误
            log::info!("GPU 采集 · NVML 不可用（无 NVIDIA 驱动？）：{e}");
            None
        }
    })
    .as_ref()
}

/// 读取全部 NVML 设备实时指标（设备逐卡读，单字段失败不影响整卡）
fn sample_nvml_devices() -> Vec<NvmlDeviceSample> {
    let Some(nvml) = nvml_instance() else {
        return Vec::new();
    };
    let count = nvml.device_count().unwrap_or(0);
    let mut out = Vec::with_capacity(count as usize);
    for index in 0..count {
        let Ok(device) = nvml.device_by_index(index) else {
            continue;
        };
        // 单字段独立降级：某项读取失败仅该项为 None（NVML 偶发 NvmlError 不致命）
        let name = device.name().unwrap_or_default();
        let temperature_c = device
            .temperature(TemperatureSensor::Gpu)
            .ok()
            .map(|t| t as f32);
        let core_clock_mhz = device.clock_info(Clock::Graphics).ok().map(|c| c as f32);
        let power_w = device.power_usage().ok().map(|mw| mw as f32 / 1000.0);
        let load_percent = device.utilization_rates().ok().map(|u| u.gpu as f32);
        let memory = device.memory_info().ok();
        out.push(NvmlDeviceSample {
            name,
            temperature_c,
            core_clock_mhz,
            power_w,
            load_percent,
            memory_used_bytes: memory.as_ref().map(|m| m.used),
            memory_total_bytes: memory.map(|m| m.total),
        });
    }
    out
}

// ============================================================================
// DXGI（全厂商适配器枚举）
// ============================================================================

/// DXGI 适配器列表（静态缓存：适配器名称/专用显存为准静态数据；
/// eGPU 热插拔等罕见场景重启应用即可刷新，避免每拍 COM 调用开销）
fn dxgi_adapters() -> Option<&'static [DxgiAdapterInfo]> {
    static CACHE: OnceLock<Option<Vec<DxgiAdapterInfo>>> = OnceLock::new();
    CACHE
        .get_or_init(|| unsafe { enumerate_dxgi_adapters() })
        .as_deref()
}

/// 枚举 DXGI 适配器（IDXGIFactory1::EnumAdapters1，普通用户可读）
///
/// 追加净化规则（实机验证修正）：
/// - 过滤软件渲染器（"Microsoft Basic Render Driver"——WARP 虚拟适配器，非硬件）；
/// - 按归一名去重（WDDM/Hyper-V GPU-P 会为同一物理卡暴露多个适配器实例，
///   实测单卡 2080 Ti 枚举出 5 个同名条目；同名合并保留首个非零显存值）。
///
/// SAFETY：COM 调用全程遵循 windows crate 安全封装（RAII 句柄 + Result 语义）；
/// 仅调用枚举/描述读取类方法，不触达设备初始化。
unsafe fn enumerate_dxgi_adapters() -> Option<Vec<DxgiAdapterInfo>> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

    let factory: IDXGIFactory1 = match CreateDXGIFactory1() {
        Ok(f) => f,
        Err(e) => {
            log::info!("GPU 采集 · DXGI 工厂创建失败（适配器枚举不可用）：{e}");
            return None;
        }
    };

    let mut out: Vec<DxgiAdapterInfo> = Vec::new();
    let mut index = 0u32;
    loop {
        let Ok(adapter) = factory.EnumAdapters1(index) else {
            break; // DXGI_ERROR_NOT_FOUND = 枚举结束；其余错误同样终止
        };
        if let Ok(desc) = adapter.GetDesc1() {
            // Description 为 [u16; 128] NUL 结尾宽字符
            let name = String::from_utf16_lossy(&desc.Description)
                .trim_end_matches('\0')
                .trim()
                .to_string();
            // 软件渲染器过滤（非硬件，硬件监测域不展示）
            let is_software = name.eq_ignore_ascii_case("Microsoft Basic Render Driver");
            if !name.is_empty() && !is_software {
                // 同名去重（归一比较）：保留首个非零专用显存（副本实例显存可能为 0）
                let key = normalize_name(&name);
                let existing = out.iter().position(|a| normalize_name(&a.name) == key);
                match existing {
                    Some(i) if desc.DedicatedVideoMemory as u64 > out[i].dedicated_video_memory => {
                        out[i].dedicated_video_memory = desc.DedicatedVideoMemory as u64;
                    }
                    Some(_) => {}
                    None => out.push(DxgiAdapterInfo {
                        name,
                        dedicated_video_memory: desc.DedicatedVideoMemory as u64,
                    }),
                }
            }
        }
        index += 1;
        // 上限防御：适配器数量异常时终止（正常系统 ≤ 8）
        if index > 32 {
            log::warn!("GPU 采集 · DXGI 适配器枚举异常（超过 32 个），截断");
            break;
        }
    }
    if out.is_empty() {
        log::info!("GPU 采集 · DXGI 未枚举到适配器");
        return None;
    }
    Some(out)
}

// ============================================================================
// 合并逻辑
// ============================================================================

/// 名称归一（小写 + 去除非字母数字），用于 DXGI/NVML 名称匹配
fn normalize_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// DXGI 基准列表 + NVML 实时指标合并
fn merge_adapters_with_nvml(
    adapters: &[DxgiAdapterInfo],
    nvml_devices: Vec<NvmlDeviceSample>,
) -> Vec<GpuSample> {
    // consumed[i] = 该 NVML 设备已配对
    let mut consumed = vec![false; nvml_devices.len()];
    adapters
        .iter()
        .map(|adapter| {
            let mut sample = GpuSample {
                name: adapter.name.clone(),
                // 专用显存总量（DXGI，全厂商；NVMe 无关）
                memory_total_bytes: if adapter.dedicated_video_memory > 0 {
                    Some(adapter.dedicated_video_memory)
                } else {
                    None
                },
                ..GpuSample::default()
            };

            // 仅 NVIDIA 适配器关联 NVML 设备（NVML 本身只服务 NVIDIA）
            if adapter.name.to_ascii_lowercase().contains("nvidia") && !nvml_devices.is_empty() {
                // ① 名称归一精确匹配（多卡异型号场景精确到卡）
                let mut pick: Option<usize> = nvml_devices
                    .iter()
                    .enumerate()
                    .find(|(i, d)| {
                        !consumed[*i] && normalize_name(&d.name) == normalize_name(&adapter.name)
                    })
                    .map(|(i, _)| i);
                // ② 名称不匹配（驱动命名差异）→ 剩余未配对设备按序配对（同型号多卡兜底）
                if pick.is_none() {
                    pick = nvml_devices
                        .iter()
                        .enumerate()
                        .find(|(i, _)| !consumed[*i])
                        .map(|(i, _)| i);
                }
                if let Some(k) = pick {
                    consumed[k] = true;
                    let d = &nvml_devices[k];
                    sample.temperature_c = d.temperature_c;
                    sample.core_clock_mhz = d.core_clock_mhz;
                    sample.power_w = d.power_w;
                    sample.load_percent = d.load_percent;
                    sample.memory_used_bytes = d.memory_used_bytes;
                    // 显存总量以 NVML 为准（更精确；DXGI 值已在上方兜底）
                    if d.memory_total_bytes.unwrap_or(0) > 0 {
                        sample.memory_total_bytes = d.memory_total_bytes;
                    }
                }
            }
            sample
        })
        .collect()
}
