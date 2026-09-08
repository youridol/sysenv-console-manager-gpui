// secm-core::settings — Windows 系统设置（注册表开关 + 电源计划）
// 对齐上游 settings.rs 语义：HAGS/游戏模式/窗口化优化/VRR/鼠标精度 +
// 高精度计时器（hpt）+ NVIDIA 电源模式（nvidia_drs）+
// 异类线程调度策略/卓越性能 + 电源计划（含 overlay 过滤/中文名映射/删除）+ 服务管理。

use serde::Serialize;

// ============================================================================
// 模块 re-export（设置页全量能力面：hpt / nvidia_drs / 服务契约）
// ============================================================================

pub use crate::hpt::{get_hpt_state, set_hpt_state};
pub use crate::nvidia_drs::{
    get_power_mode as get_nvidia_power_mode, set_power_mode as set_nvidia_power_mode,
    NvidiaPowerMode,
};

// ============================================================================
// 数据类型（对齐源）
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct SettingState {
    pub name: String,
    pub enabled: bool,
    pub admin_required: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PowerPlan {
    pub guid: String,
    pub name: String,
    pub is_active: bool,
}

// ============================================================================
// 注册表辅助（winreg，HKCU/HKLM + WOW64 重定向保护）
// ============================================================================

/// 读取 DWORD（含 KEY_WOW64_64KEY 以对齐源语义）
pub fn read_registry_dword(hive: &str, key: &str, value: &str) -> Result<u32, String> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkey = match hive.to_uppercase().as_str() {
        "HKCU" => HKEY_CURRENT_USER,
        "HKLM" => HKEY_LOCAL_MACHINE,
        _ => return Err(format!("[REG_ERR] 不支持的根键: {}", hive)),
    };
    let subkey = RegKey::predef(hkey)
        .open_subkey_with_flags(key, KEY_READ | KEY_WOW64_64KEY)
        .map_err(|e| format!("[REG_OPEN_ERR] 无法打开: {}\\{} | {}", hive, key, e))?;
    subkey.get_value(value).map_err(|e| {
        format!(
            "[REG_VALUE_ERR] 无法读取 '{}': {}\\{} | {}",
            value, hive, key, e
        )
    })
}

/// 写入 DWORD（HKLM 需管理员；key 不存在自动创建）
pub fn write_registry_dword(hive: &str, key: &str, value: &str, data: u32) -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkey = match hive.to_uppercase().as_str() {
        "HKCU" => HKEY_CURRENT_USER,
        "HKLM" => HKEY_LOCAL_MACHINE,
        _ => return Err("不支持的根键".to_string()),
    };
    if hive.to_uppercase() == "HKLM" && !is_admin() {
        return Err("需要管理员权限才能写入 HKLM。请以管理员身份运行 SECM。".to_string());
    }
    let subkey = RegKey::predef(hkey)
        .create_subkey(key)
        .map_err(|e| format!("创建/打开注册表键失败 {}: {}", key, e))?;
    subkey
        .0
        .set_value(value, &data)
        .map_err(|e| format!("写入注册表值失败 {}: {}", value, e))
}

/// 读取 REG_SZ（字符串）值（含 KEY_WOW64_64KEY 以对齐源语义）
pub fn read_registry_string(hive: &str, key: &str, value: &str) -> Result<String, String> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkey = match hive.to_uppercase().as_str() {
        "HKCU" => HKEY_CURRENT_USER,
        "HKLM" => HKEY_LOCAL_MACHINE,
        _ => return Err(format!("不支持的根键: {}", hive)),
    };
    let subkey = RegKey::predef(hkey)
        .open_subkey_with_flags(key, KEY_READ | KEY_WOW64_64KEY)
        .map_err(|e| format!("[REG_OPEN_ERR] 无法打开: {}\\{} | {}", hive, key, e))?;
    subkey.get_value(value).map_err(|e| {
        format!(
            "[REG_VALUE_ERR] REG_SZ '{}': {}\\{} | {}",
            value, hive, key, e
        )
    })
}

/// 写入 REG_SZ（字符串）值（HKLM 需管理员；key 不存在自动创建）
pub fn write_registry_string(hive: &str, key: &str, value: &str, data: &str) -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkey = match hive.to_uppercase().as_str() {
        "HKCU" => HKEY_CURRENT_USER,
        "HKLM" => HKEY_LOCAL_MACHINE,
        _ => return Err("不支持的根键".to_string()),
    };
    if hive.to_uppercase() == "HKLM" && !is_admin() {
        return Err("需要管理员权限才能写入 HKLM。请以管理员身份运行 SECM。".to_string());
    }
    let subkey = RegKey::predef(hkey)
        .create_subkey(key)
        .map_err(|e| format!("创建/打开注册表键失败 {}: {}", key, e))?;
    subkey
        .0
        .set_value(value, &data)
        .map_err(|e| format!("写入注册表值失败 {}: {}", value, e))
}

// ============================================================================
// 权限检查
// ============================================================================

/// 当前进程是否以管理员权限运行（TokenElevation）
pub fn is_admin() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY};
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        unsafe {
            let process = GetCurrentProcess();
            let mut token = std::ptr::null_mut();
            if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 || token.is_null() {
                return false;
            }
            let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut len: u32 = 0;
            let ok = GetTokenInformation(
                token,
                20, // TokenElevation
                &mut elevation as *mut _ as *mut std::ffi::c_void,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            );
            CloseHandle(token);
            ok != 0 && elevation.TokenIsElevated != 0
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

// ============================================================================
// HAGS（GPU 硬件加速调度）
// ============================================================================

const GRAPHICS_DRIVERS_KEY: &str = r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers";

pub fn get_hags_state() -> SettingState {
    match read_registry_dword("HKLM", GRAPHICS_DRIVERS_KEY, "HwSchMode") {
        Ok(val) => SettingState {
            name: "GPU 硬件加速调度 (HAGS)".to_string(),
            enabled: val == 2,
            admin_required: true,
            message: if val == 2 {
                format!("HwSchMode={} -> 已启用", val)
            } else {
                format!("HwSchMode={} -> 已禁用", val)
            },
        },
        Err(e) => SettingState {
            name: "GPU 硬件加速调度 (HAGS)".to_string(),
            enabled: false,
            admin_required: true,
            // 对齐上游：读取失败时错误消息并入 GPU 检测信息（帮助定位无显卡/驱动异常环境）
            message: format!("读取失败: {} | {}", e, detect_gpu_message()),
        },
    }
}

pub fn set_hags_state(enabled: bool) -> Result<SettingState, String> {
    let current = read_registry_dword("HKLM", GRAPHICS_DRIVERS_KEY, "HwSchMode").ok();
    let value = if enabled {
        2u32
    } else if current == Some(2) {
        1u32
    } else {
        0u32
    };
    write_registry_dword("HKLM", GRAPHICS_DRIVERS_KEY, "HwSchMode", value)?;
    Ok(SettingState {
        name: "GPU 硬件加速调度 (HAGS)".to_string(),
        enabled,
        admin_required: true,
        message: format!(
            "已{}（HwSchMode={}，重启后生效）",
            if enabled { "启用" } else { "禁用" },
            value
        ),
    })
}

// ============================================================================
// 游戏模式（设置 → 游戏 → 游戏模式）
// ============================================================================

const GAMEBAR_KEY: &str = r"Software\Microsoft\GameBar";
const GAME_MODE_VAL: &str = "AutoGameModeEnabled";

pub fn get_game_mode_state() -> SettingState {
    match read_registry_dword("HKCU", GAMEBAR_KEY, GAME_MODE_VAL) {
        Ok(val) => SettingState {
            name: "游戏模式".to_string(),
            enabled: val == 1,
            admin_required: false,
            message: if val == 1 {
                "AutoGameModeEnabled=1 -> 已启用".to_string()
            } else {
                "AutoGameModeEnabled=0 -> 已禁用".to_string()
            },
        },
        Err(_) => SettingState {
            // 缺失 = 系统默认（启用）
            name: "游戏模式".to_string(),
            enabled: true,
            admin_required: false,
            message: "未显式配置（系统默认启用）".to_string(),
        },
    }
}

pub fn set_game_mode_state(enabled: bool) -> Result<SettingState, String> {
    write_registry_dword("HKCU", GAMEBAR_KEY, GAME_MODE_VAL, enabled as u32)?;
    Ok(SettingState {
        name: "游戏模式".to_string(),
        enabled,
        admin_required: false,
        message: format!("已{}", if enabled { "启用" } else { "禁用" }),
    })
}

// ============================================================================
// DirectX 窗口化游戏优化 + VRR（同一 REG_SZ 键，分号分隔 "Key=Val;" 格式）
// ============================================================================

/// DirectX GPU 偏好注册表键（HKCU）
const DX_GPU_PREFS_KEY: &str = "Software\\Microsoft\\DirectX\\UserGpuPreferences";
/// 全局 DirectX 用户设置值名（REG_SZ，分号分隔 key=value 串）
const DX_GLOBAL_SETTINGS_VAL: &str = "DirectXUserGlobalSettings";

/// 解析分号分隔的 key=value 字符串（DirectXUserGlobalSettings 格式）。
/// 返回目标键对应值是否非 "0"；键不存在返回 None。
fn parse_dx_setting(settings: &str, target_key: &str) -> Option<bool> {
    for part in settings.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            if k.trim() == target_key {
                return Some(v.trim() != "0");
            }
        }
    }
    None
}

/// 在 DirectXUserGlobalSettings 分号分隔字符串中更新目标键（已存在则替换，不存在则追加）
fn update_dx_setting(settings: &str, target_key: &str, enabled: bool) -> String {
    let val = if enabled { "1" } else { "0" };
    let mut found = false;
    let mut result = String::new();
    for part in settings.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((k, _v)) = part.split_once('=') {
            if k.trim() == target_key {
                result.push_str(&format!("{}={};", target_key, val));
                found = true;
                continue;
            }
        }
        result.push_str(part);
        result.push(';');
    }
    if !found {
        result.push_str(&format!("{}={};", target_key, val));
    }
    result
}

/// 检测 GPU 厂商并返回用户可读描述（纯 Rust 驱动：注册表 Class 键枚举，替代 wmic）。
/// 失败/无显卡记录时返回 "未检测到 GPU"（与源 detect_gpu_via_wmi 语义一致）。
fn detect_gpu_message() -> String {
    match secm_datasource::registry::detect_gpu() {
        Ok(Some(g)) => g.message,
        _ => "未检测到 GPU".to_string(),
    }
}

/// 窗口化游戏优化（SwapEffectUpgradeEnable）
/// 注册表：HKCU\Software\Microsoft\DirectX\UserGpuPreferences\DirectXUserGlobalSettings
///   1=启用 | 0=禁用 | 缺失 = 系统默认（启用）
pub fn get_game_optimization_state() -> SettingState {
    match read_registry_string("HKCU", DX_GPU_PREFS_KEY, DX_GLOBAL_SETTINGS_VAL) {
        Ok(settings) => match parse_dx_setting(&settings, "SwapEffectUpgradeEnable") {
            Some(true) => SettingState {
                name: "窗口化游戏优化".to_string(),
                enabled: true,
                admin_required: false,
                message: "[GAMEOPT_OK] SwapEffectUpgradeEnable=1".to_string(),
            },
            Some(false) => SettingState {
                name: "窗口化游戏优化".to_string(),
                enabled: false,
                admin_required: false,
                message: "[GAMEOPT_OK] SwapEffectUpgradeEnable=0".to_string(),
            },
            None => SettingState {
                name: "窗口化游戏优化".to_string(),
                enabled: true,
                admin_required: false,
                message: "[GAMEOPT_DEF] SwapEffectUpgradeEnable 不存在 -> 默认启用".to_string(),
            },
        },
        Err(_) => {
            // 旧版 Windows 兜底：GameDVR_FSEBehaviorMode=2 表示全屏优化开启
            if let Ok(val) =
                read_registry_dword("HKCU", "System\\GameConfigStore", "GameDVR_FSEBehaviorMode")
            {
                return SettingState {
                    name: "窗口化游戏优化".to_string(),
                    enabled: val == 2,
                    admin_required: false,
                    message: format!("[GAMEOPT_FB] GameDVR_FSEBehaviorMode={} (旧版Win)", val),
                };
            }
            SettingState {
                name: "窗口化游戏优化".to_string(),
                enabled: false,
                admin_required: false,
                message: format!(
                    "[GAMEOPT_NF] {} 不存在 | {}",
                    DX_GLOBAL_SETTINGS_VAL,
                    detect_gpu_message()
                ),
            }
        }
    }
}

pub fn set_game_optimization(enabled: bool) -> Result<SettingState, String> {
    let cur =
        read_registry_string("HKCU", DX_GPU_PREFS_KEY, DX_GLOBAL_SETTINGS_VAL).unwrap_or_default();
    let upd = update_dx_setting(&cur, "SwapEffectUpgradeEnable", enabled);
    write_registry_string("HKCU", DX_GPU_PREFS_KEY, DX_GLOBAL_SETTINGS_VAL, &upd)?;
    Ok(SettingState {
        name: "窗口化游戏优化".to_string(),
        enabled,
        admin_required: false,
        message: format!(
            "窗口化游戏优化已{} (SwapEffectUpgradeEnable={})",
            if enabled { "启用" } else { "禁用" },
            if enabled { "1" } else { "0" }
        ),
    })
}

/// 可变刷新率 (VRR/G-Sync/FreeSync)
/// 注册表：HKCU\...\DirectXUserGlobalSettings -> VRROptimizeEnable
///   1=启用 | 0=禁用 | 缺失 = 系统默认（关闭）
pub fn get_vrr_state() -> SettingState {
    match read_registry_string("HKCU", DX_GPU_PREFS_KEY, DX_GLOBAL_SETTINGS_VAL) {
        Ok(settings) => match parse_dx_setting(&settings, "VRROptimizeEnable") {
            Some(true) => SettingState {
                name: "可变刷新率 (VRR/G-Sync/FreeSync)".to_string(),
                enabled: true,
                admin_required: false,
                message: "[VRR_OK] VRROptimizeEnable=1".to_string(),
            },
            Some(false) => SettingState {
                name: "可变刷新率 (VRR/G-Sync/FreeSync)".to_string(),
                enabled: false,
                admin_required: false,
                message: "[VRR_OK] VRROptimizeEnable=0".to_string(),
            },
            None => SettingState {
                name: "可变刷新率 (VRR/G-Sync/FreeSync)".to_string(),
                enabled: false,
                admin_required: false,
                message: "[VRR_DEF] VRROptimizeEnable 不存在 -> 默认关闭".to_string(),
            },
        },
        Err(_) => SettingState {
            name: "可变刷新率 (VRR/G-Sync/FreeSync)".to_string(),
            enabled: false,
            admin_required: false,
            message: format!(
                "[VRR_NF] {} 不存在 | {}",
                DX_GLOBAL_SETTINGS_VAL,
                detect_gpu_message()
            ),
        },
    }
}

pub fn set_vrr_state(enabled: bool) -> Result<SettingState, String> {
    let cur =
        read_registry_string("HKCU", DX_GPU_PREFS_KEY, DX_GLOBAL_SETTINGS_VAL).unwrap_or_default();
    let upd = update_dx_setting(&cur, "VRROptimizeEnable", enabled);
    write_registry_string("HKCU", DX_GPU_PREFS_KEY, DX_GLOBAL_SETTINGS_VAL, &upd)?;
    Ok(SettingState {
        name: "可变刷新率 (VRR/G-Sync/FreeSync)".to_string(),
        enabled,
        admin_required: false,
        message: format!(
            "VRR 已{} (VRROptimizeEnable={})",
            if enabled { "启用" } else { "禁用" },
            if enabled { "1" } else { "0" }
        ),
    })
}

// ============================================================================
// 鼠标精准度（提高指针精确度）
// 依据 Windows 规范：必须用 SystemParametersInfoW(SPI_GET/SETMOUSE)，
// 纯注册表写入无效（SPI_SETMOUSE 会回写 HKCU\Control Panel\Mouse）。
// ============================================================================

/// 读取鼠标精准度状态：SPI_GETMOUSE 返回 [Threshold1, Threshold2, 加速开关]，
/// 第三项非 0 表示"提高指针精确度"开启；API 失败时回退注册表 MouseSpeed。
pub fn get_mouse_precision_state() -> SettingState {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETMOUSE};
    let mut params: [i32; 3] = [0; 3];
    // SAFETY: SPI_GETMOUSE 向 pvParam 写入 3 个 i32（与数组长度匹配），无其他资源要求
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETMOUSE,
            0,
            params.as_mut_ptr() as *mut std::ffi::c_void,
            0,
        )
    };
    if ok == 0 {
        // API 失败回退：注册表 MouseSpeed=1 表示开启（与系统设置面板同源）
        let reg_enabled = read_registry_dword("HKCU", "Control Panel\\Mouse", "MouseSpeed")
            .map(|v| v == 1)
            .unwrap_or(false);
        SettingState {
            name: "鼠标精准度 (提高指针精确度)".to_string(),
            enabled: reg_enabled,
            admin_required: false,
            message: if reg_enabled {
                "提高指针精确度已启用 (注册表回退)".to_string()
            } else {
                "无法通过系统API读取，注册表显示已禁用".to_string()
            },
        }
    } else {
        let enabled = params[2] != 0;
        SettingState {
            name: "鼠标精准度 (提高指针精确度)".to_string(),
            enabled,
            admin_required: false,
            message: if enabled {
                format!(
                    "已启用 (Threshold1={}, Threshold2={}, Speed={})",
                    params[0], params[1], params[2]
                )
            } else {
                format!(
                    "已禁用 (Thresholds: {}/{} Speed={})",
                    params[0], params[1], params[2]
                )
            },
        }
    }
}

/// 设置鼠标精准度：SPI_SETMOUSE + SPIF_UPDATEINIFILE | SPIF_SENDCHANGE
/// 启用 = [6, 10, 1]，禁用 = [0, 0, 0]（对齐源 v1.19.0 实现）。
pub fn set_mouse_precision(enabled: bool) -> Result<SettingState, String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETMOUSE,
    };
    let mut params: [i32; 3] = if enabled { [6, 10, 1] } else { [0, 0, 0] };
    // SAFETY: SPI_SETMOUSE 读取 pvParam 指向的 3 个 i32；flags 同时持久化并广播变更
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_SETMOUSE,
            0,
            params.as_mut_ptr() as *mut std::ffi::c_void,
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
    };
    if ok == 0 {
        return Err("SystemParametersInfoW(SPI_SETMOUSE) 调用失败".to_string());
    }
    Ok(SettingState {
        name: "鼠标精准度".to_string(),
        enabled,
        admin_required: false,
        message: format!(
            "鼠标精准度已{} (SPI_SETMOUSE)",
            if enabled { "启用" } else { "禁用" }
        ),
    })
}

// ============================================================================
// 电源计划（枚举/切换；活动态经 datasource::power 权威 API）
// ============================================================================

use secm_datasource::power;

/// Win11 overlay GUID —— 非真实电源计划（节能模式覆盖层等），枚举时必须剔除（对齐上游）
const OVERLAY_GUIDS: &[&str] = &[
    "961cc777-2547-4f9d-8174-7d86181b8a7a",
    "ded574b5-45a0-4f42-8737-46345c09c238",
];

/// 已知电源计划 GUID → 中文名映射 + FriendlyName 间接串解析（对齐上游 power_plan_name）
///
/// FriendlyName 可能为 `@C:\Windows\...dll,-15,Balanced` 形式的间接字符串，取最后一个逗号后的
/// 可读名；英文名回退中文；空名回退 GUID 前 8 位。
fn power_plan_name(guid: &str, raw_name: &str) -> String {
    match guid.to_lowercase().as_str() {
        "381b4222-f694-41f0-9685-ff5bb260df2e" => "平衡".to_string(),
        "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c" => "高性能".to_string(),
        "a1841308-3541-4fab-bc81-f71556f20b4a" => "节能".to_string(),
        "e9a42b02-d5df-448d-aa00-03f14749eb61" => "卓越性能".to_string(),
        _ => {
            // 解析 FriendlyName —— 可能为 "@C:\\Windows\\...dll,-15,Balanced" 间接串
            let name = if let Some(idx) = raw_name.rfind(',') {
                raw_name[idx + 1..].trim().to_string()
            } else {
                raw_name.to_string()
            };
            // 英文 → 中文回退
            match name.to_lowercase().as_str() {
                "balanced" => "平衡".into(),
                "high performance" => "高性能".into(),
                "power saver" => "节能".into(),
                "ultimate performance" => "卓越性能".into(),
                _ if name.is_empty() => guid[..8].to_string(),
                _ => name,
            }
        }
    }
}

/// 枚举电源计划（注册表 PowerSchemes 子键 + 活动 GUID 标注；剔除 overlay 非真实计划）
pub fn get_power_plans() -> Result<Vec<PowerPlan>, String> {
    use winreg::enums::*;
    use winreg::RegKey;
    let active = power::get_active_scheme()
        .ok()
        .flatten()
        .unwrap_or_default()
        .to_lowercase();

    let schemes_key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            r"SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes",
            KEY_READ | KEY_WOW64_64KEY,
        )
        .map_err(|e| format!("打开电源计划注册表失败: {}", e))?;

    let mut plans = Vec::new();
    for name in schemes_key.enum_keys().flatten() {
        // Win11 overlay GUID 剔除（非真实电源计划，UI 不应展示/激活/删除）
        if OVERLAY_GUIDS.contains(&name.to_lowercase().as_str()) {
            continue;
        }
        // 名称：已知 GUID 中文映射 → FriendlyName 间接串解析 → GUID 前 8 位
        let friendly = read_power_scheme_name(&name);
        let display = power_plan_name(&name, &friendly);
        // FriendlyName 含 "Overlay" 的条目同样不是真实电源计划（对齐上游双重过滤）
        if display.to_lowercase().contains("overlay") {
            continue;
        }
        plans.push(PowerPlan {
            // GUID 统一小写（与激活比对/删除/导入查重一致）
            guid: name.to_lowercase(),
            name: display,
            is_active: name.to_lowercase() == active,
        });
    }
    Ok(plans)
}

/// 读取电源计划显示名（HKLM FriendlyName；失败回退 GUID）
fn read_power_scheme_name(guid: &str) -> String {
    use winreg::enums::*;
    use winreg::RegKey;
    let path = format!(
        r"SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes\{}",
        guid
    );
    if let Ok(key) =
        RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(&path, KEY_READ | KEY_WOW64_64KEY)
    {
        if let Ok(name) = key.get_value::<String, _>("FriendlyName") {
            return name;
        }
    }
    guid.to_string()
}

/// 切换激活电源计划
pub fn set_power_plan(guid: &str) -> Result<(), String> {
    power::set_active_scheme(guid).map_err(|e| format!("切换电源计划失败: {}", e))
}

/// 删除电源计划（活动计划禁止删除由系统拒绝）
pub fn delete_power_plan(guid: &str) -> Result<(), String> {
    power::delete_scheme(guid).map_err(|e| format!("删除电源计划失败: {}", e))
}

// ============================================================================
// 异类线程调度策略（混合架构 CPU 大小核调度：Intel 12代+ / AMD 大小核）
// ============================================================================

/// 处理器电源管理子组 GUID
const SUBGROUP_PROCESSOR: &str = "54533251-82be-4824-96c1-47b60b740d00";
/// 异类线程调度策略设置 GUID
const HETERO_THREAD_POLICY_GUID: &str = "93b8b6dc-0698-4d1c-9ee4-0644e900c85d";
/// 异类短运行线程调度策略设置 GUID
const HETERO_SHORT_THREAD_POLICY_GUID: &str = "465e1f50-b610-473a-ab58-00d1077dc418";

/// 异类调度策略 AC/DC 当前值（None = 使用系统默认，未显式设置）
///
/// `*_present`：当前电源计划下是否存在该策略设置项（`PowerSchemes\<方案>\{处理器子组}\{策略GUID}` 键）。
/// 缺失时 UI 提示"首次配置将自动注入"，`set_hetero_policy_scoped` 写入前自动注入，保证配置成功。
#[derive(Debug, Clone, Serialize)]
pub struct HeteroPolicies {
    pub supported: bool,
    pub thread_present: bool,
    pub short_present: bool,
    pub thread_ac: Option<u32>,
    pub thread_dc: Option<u32>,
    pub short_ac: Option<u32>,
    pub short_dc: Option<u32>,
}

/// 读取活动电源方案下异类调度策略的 AC/DC 值（注册表读取，零进程 spawn）
///
/// 取值枚举（与电源选项面板一致）：
///   0=所有处理器 1=高性能处理器 2=首选高性能处理器 3=高效处理器 4=首选高效处理器 5=自动
pub fn get_hetero_policies() -> Result<HeteroPolicies, String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let schemes = hklm
        .open_subkey_with_flags(
            r"SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes",
            KEY_READ | KEY_WOW64_64KEY,
        )
        .map_err(|e| format!("[REG] 无法打开 PowerSchemes: {}", e))?;
    let power_settings = hklm
        .open_subkey_with_flags(
            r"SYSTEM\CurrentControlSet\Control\Power\PowerSettings",
            KEY_READ | KEY_WOW64_64KEY,
        )
        .ok();
    let power_settings = power_settings.as_ref();

    let active_guid: String = schemes.get_value("ActivePowerScheme").unwrap_or_default();

    /// 读取方案下某设置的 AC/DC 索引（未显式设置时回退到系统默认值）
    fn read_index(
        schemes: &winreg::RegKey,
        power_settings: &winreg::RegKey,
        active: &str,
        setting: &str,
        ac: bool,
    ) -> Option<u32> {
        let value_name = if ac {
            "ACSettingIndex"
        } else {
            "DCSettingIndex"
        };
        let fallback = if ac {
            "DefaultACSettingIndex"
        } else {
            "DefaultDCSettingIndex"
        };
        if active.is_empty() {
            return None;
        }
        // 1) 活动方案下的显式值
        let key = format!("{}\\{}\\{}", active, SUBGROUP_PROCESSOR, setting);
        if let Ok(sub) = schemes.open_subkey_with_flags(&key, KEY_READ) {
            if let Ok(v) = sub.get_value::<u32, _>(value_name) {
                return Some(v);
            }
        }
        // 2) 回退：系统默认值（PowerSettings 全局定义）
        let default_key = format!("{}\\{}", SUBGROUP_PROCESSOR, setting);
        if let Ok(sub) = power_settings.open_subkey_with_flags(&default_key, KEY_READ) {
            if let Ok(v) = sub.get_value::<u32, _>(fallback) {
                return Some(v);
            }
        }
        None
    }

    let read = |setting: &str, ac: bool| -> Option<u32> {
        match power_settings {
            Some(ps) => read_index(&schemes, ps, &active_guid, setting, ac),
            None => read_index(&schemes, &schemes, &active_guid, setting, ac),
        }
    };

    // 非混合架构 CPU 上这两个设置键不存在（或读取失败）→ 标记不支持
    let thread_ac = read(HETERO_THREAD_POLICY_GUID, true);
    let thread_dc = read(HETERO_THREAD_POLICY_GUID, false);
    let short_ac = read(HETERO_SHORT_THREAD_POLICY_GUID, true);
    let short_dc = read(HETERO_SHORT_THREAD_POLICY_GUID, false);

    let supported = thread_ac.is_some() || thread_dc.is_some();

    // 方案级设置项存在性检测（缺失时写入前自动注入，见 ensure_hetero_setting_key）
    let thread_present = hetero_setting_key_exists(&active_guid, HETERO_THREAD_POLICY_GUID);
    let short_present = hetero_setting_key_exists(&active_guid, HETERO_SHORT_THREAD_POLICY_GUID);

    Ok(HeteroPolicies {
        supported,
        thread_present,
        short_present,
        thread_ac,
        thread_dc,
        short_ac,
        short_dc,
    })
}

/// 检测当前（或指定）电源方案下是否已存在异类策略设置项键
///
/// 存在性判定 = `PowerSchemes\<方案>\{处理器子组}\{策略GUID}` 键存在（值可由
/// ACSettingIndex/DCSettingIndex 承载；键存在但值缺失时读取回退系统默认）。
///
/// 缺失时的注入机制（真机验证）：`PowerWrite*ValueIndex` 对不存在的设置项键
/// 自动创建并落值（电源服务链路），因此"注入"内建于写入路径，无需注册表直写
/// ——PowerSchemes 子键受电源服务 ACL 保护，直写注册表即使管理员也被拒绝。
fn hetero_setting_key_exists(active_scheme: &str, setting: &str) -> bool {
    use winreg::enums::*;
    use winreg::RegKey;
    if active_scheme.is_empty() {
        return false;
    }
    let path = format!(
        r"SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes\{}\{}\{}",
        active_scheme, SUBGROUP_PROCESSOR, setting
    );
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(&path, KEY_READ | KEY_WOW64_64KEY)
        .is_ok()
}

/// 设置异类调度策略（AC/DC 双写便捷封装：两路同时写入 + 重新激活当前方案生效）
///
/// `kind`: "thread" = 异类线程调度策略, "short" = 异类短运行线程调度策略
pub fn set_hetero_policy(kind: &str, value: u32) -> Result<(), String> {
    set_hetero_policy_scoped(kind, value, true, true)
}

/// 设置异类调度策略（作用域版：按需写 AC / DC 任意组合 + 重新激活当前方案使更改生效）
///
/// 纯 Rust 驱动（secm-datasource::power：PowerWriteACValueIndex /
/// PowerWriteDCValueIndex / PowerSetActiveScheme，与 powercfg /setacvalueindex 等等价）。
///
/// 缺失自动注入（真机机制验证）：方案下不存在策略设置项键时，PowerWrite*ValueIndex
/// 会自动创建该键并落值（与 powercfg /setacvalueindex 同链路，电源服务落盘）——
/// 无需（也不可）直写注册表：PowerSchemes 子键受电源服务 ACL 保护，管理员直写亦被拒绝。
///
/// `kind`: "thread" = 异类线程调度策略, "short" = 异类短运行线程调度策略
/// `apply_ac` / `apply_dc`: 是否写入交流/电池两路值（UI 允许分别调整六档策略）
pub fn set_hetero_policy_scoped(
    kind: &str,
    value: u32,
    apply_ac: bool,
    apply_dc: bool,
) -> Result<(), String> {
    let setting = match kind {
        "short" => HETERO_SHORT_THREAD_POLICY_GUID,
        _ => HETERO_THREAD_POLICY_GUID,
    };

    // P1-17：先读 AC 原值（回滚用），写失败时回滚已写侧，避免留下半套状态。
    // 读失败（部分机器该 GUID 无值）视为"无原值"，回滚时跳过。
    let ac_backup = power::read_ac_value(None, SUBGROUP_PROCESSOR, setting).ok();

    // 写 AC/DC 值索引（None = 当前激活方案，对应 SCHEME_CURRENT 语义）
    if apply_ac {
        if let Err(e) = power::write_ac_value(None, SUBGROUP_PROCESSOR, setting, value) {
            return Err(format!("写入异类策略 AC 值失败: {}", e));
        }
    }
    if apply_dc {
        if let Err(e) = power::write_dc_value(None, SUBGROUP_PROCESSOR, setting, value) {
            // DC 写失败：回滚 AC 至原值（有原值才回滚），保证 AC/DC 状态一致
            if apply_ac {
                if let Some(orig) = ac_backup {
                    let _ = power::write_ac_value(None, SUBGROUP_PROCESSOR, setting, orig);
                }
            }
            return Err(format!("写入异类策略 DC 值失败（已回滚 AC）: {}", e));
        }
    }

    // 重新激活当前方案（对应 powercfg /s SCHEME_CURRENT，使更改立即生效）
    match power::get_active_scheme() {
        Ok(Some(active)) => {
            if let Err(e) = power::set_active_scheme(&active) {
                // 值已写入但未生效：如实提示，不谎报成功（重开应用/重启后生效）
                return Err(format!("异类策略已写入但激活刷新失败（重启后生效）: {}", e));
            }
        }
        Ok(None) => {
            log::warn!("settings: 异类策略写入成功但无激活方案可刷新");
        }
        Err(e) => {
            return Err(format!("获取当前电源计划失败: {}", e));
        }
    }
    Ok(())
}

/// 导入卓越性能电源计划（e9a42b02-...）
///
/// 纯 Rust 驱动（secm-datasource::power::duplicate_scheme，即 PowerDuplicateScheme，
/// 与 powercfg /duplicatescheme 等价），新实现直接取 API 返回的新 GUID。
pub fn enable_ultimate_performance() -> Result<String, String> {
    // 先查是否已存在（注册表读路径）
    let existing = get_power_plans().unwrap_or_default();
    for plan in &existing {
        if plan.guid.contains("e9a42b02-d5df-448d-aa00-03f14749eb61") {
            return Ok(format!("卓越性能电源计划已存在: {}", plan.guid));
        }
    }

    power::duplicate_scheme("e9a42b02-d5df-448d-aa00-03f14749eb61")
        .map(|guid| format!("卓越性能已导入: {}", guid))
        .map_err(|e| format!("导入卓越性能电源计划失败: {}", e))
}

// ============================================================================
// Windows 服务管理（枚举/启停/启动类型；对齐源 settings.rs 服务面）
// ============================================================================

pub use secm_datasource::service::ServiceInfo;

/// 枚举全部服务（含启动类型并行补全，对齐上游 list_all_services 语义）
///
/// datasource::enum_services 一次返回名称/显示名/状态（启动类型占位"未知"），
/// 启动类型由本函数 8 线程并行补全（内存内 QueryServiceConfigW 调用，
/// 比 spawn `sc qc` 快 2-3 个数量级）。单个服务查询失败降级"未知"，不阻塞整体。
pub fn list_all_services() -> Result<Vec<ServiceInfo>, String> {
    let mut entries =
        secm_datasource::service::enum_services().map_err(|e| format!("枚举服务失败: {}", e))?;

    // 8 线程分片并行补全启动类型（QueryServiceConfigW 为阻塞 API，并行摊薄总时延）
    const START_TYPE_THREADS: usize = 8;
    let n = entries.len();
    if n > 0 {
        let threads = START_TYPE_THREADS.min(n);
        let chunk = n.div_ceil(threads);
        std::thread::scope(|scope| {
            let handles: Vec<_> = entries
                .chunks_mut(chunk)
                .map(|slice| {
                    scope.spawn(move || {
                        for svc in slice.iter_mut() {
                            svc.start_type = secm_datasource::service::start_type_of(&svc.name);
                        }
                    })
                })
                .collect();
            for h in handles {
                let _ = h.join();
            }
        });
    }
    Ok(entries)
}

/// 启动服务（需管理员；幂等）
pub fn start_service(name: &str) -> Result<String, String> {
    if !is_admin() {
        return Err("启动服务需要管理员权限。请以管理员身份运行 SECM。".to_string());
    }
    secm_datasource::service::start_service(name)
        .map_err(|e| format!("启动服务 '{}' 失败: {}", name, e))?;
    Ok(format!("服务 '{}' 已启动", name))
}

/// 停止服务（需管理员；幂等）
pub fn stop_service(name: &str) -> Result<String, String> {
    if !is_admin() {
        return Err("停止服务需要管理员权限。请以管理员身份运行 SECM。".to_string());
    }
    secm_datasource::service::stop_service(name)
        .map_err(|e| format!("停止服务 '{}' 失败: {}", name, e))?;
    Ok(format!("服务 '{}' 已停止", name))
}

/// 设置服务启动类型（自动/手动/禁用；需管理员）
pub fn set_service_start_type(name: &str, start_type: &str) -> Result<String, String> {
    if !is_admin() {
        return Err("修改服务启动类型需要管理员权限。请以管理员身份运行 SECM。".to_string());
    }
    let normalized = match start_type {
        "自动" | "auto" | "Auto" => "auto",
        "手动" | "manual" | "Manual" | "demand" => "manual",
        "禁用" | "disabled" | "Disabled" => "disabled",
        other => return Err(format!("不支持的启动类型: {}", other)),
    };
    secm_datasource::service::set_service_start_type(name, normalized)
        .map_err(|e| format!("设置服务 '{}' 启动类型失败: {}", name, e))?;
    Ok(format!("服务 '{}' 启动类型已设为 {}", name, normalized))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dx_setting() {
        let s = "SwapEffectUpgradeEnable=1;VRROptimizeEnable=0;";
        assert_eq!(parse_dx_setting(s, "SwapEffectUpgradeEnable"), Some(true));
        assert_eq!(parse_dx_setting(s, "VRROptimizeEnable"), Some(false));
        assert_eq!(parse_dx_setting(s, "NotFound"), None);
    }

    #[test]
    fn test_update_dx_setting() {
        let s = "SwapEffectUpgradeEnable=1;VRROptimizeEnable=0;";
        let s2 = update_dx_setting(s, "VRROptimizeEnable", true);
        assert!(s2.contains("VRROptimizeEnable=1"));
        assert!(s2.contains("SwapEffectUpgradeEnable=1")); // preserved

        // 不存在时追加
        let s3 = update_dx_setting("VRROptimizeEnable=0;", "SwapEffectUpgradeEnable", true);
        assert!(s3.contains("SwapEffectUpgradeEnable=1"));
        assert!(s3.contains("VRROptimizeEnable=0"));

        // 空串追加
        let s4 = update_dx_setting("", "SwapEffectUpgradeEnable", false);
        assert_eq!(s4, "SwapEffectUpgradeEnable=0;");
    }

    #[test]
    fn test_read_registry_string_error_unknown_hive() {
        // 未知根键应返回 Err（不触碰真实注册表）
        assert!(read_registry_string("HKCZ", "Software\\X", "V").is_err());
        assert!(read_registry_dword("HKCZ", "Software\\X", "V").is_err());
        assert!(write_registry_string("HKCZ", "Software\\X", "V", "data").is_err());
    }
}
