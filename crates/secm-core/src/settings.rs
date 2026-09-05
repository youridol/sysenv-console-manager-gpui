// secm-core::settings — Windows 系统设置（注册表开关 + 电源计划）
// 对齐源 v1.19.0 settings.rs 语义：HAGS/游戏模式/窗口化优化/VRR/鼠标精度 + 电源计划。
// Phase 2 首批：注册表 helper、HAGS、游戏模式、电源计划枚举/切换；其余后续补齐。

use serde::Serialize;

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
    subkey
        .get_value(value)
        .map_err(|e| format!("[REG_VALUE_ERR] 无法读取 '{}': {}\\{} | {}", value, hive, key, e))
}

/// 写入 DWORD（HKLM 需管理员；key 不存在自动创建）
pub fn write_registry_dword(
    hive: &str,
    key: &str,
    value: &str,
    data: u32,
) -> Result<(), String> {
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
            message: format!("读取失败: {}", e),
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
// 电源计划（枚举/切换；活动态经 datasource::power 权威 API）
// ============================================================================

use secm_datasource::power;

/// 枚举电源计划（注册表 PowerSchemes 子键 + 活动 GUID 标注）
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
        // 名称来自子键 FriendlyName (可本地化) → 优先读英文名称
        let friendly = read_power_scheme_name(&name);
        plans.push(PowerPlan {
            guid: name.clone(),
            name: friendly,
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
    if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(
        &path,
        KEY_READ | KEY_WOW64_64KEY,
    ) {
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
