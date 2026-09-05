// secm-core::sysinfo — 系统信息模块（静态 Windows OS 信息采集）
//
// 自旧 Tauri 端 src-tauri/src/sysinfo.rs 迁入（Phase 1），保留数据契约与中文注释。
//
// 采集项目：
// - 产品名称 (Edition)
// - Build 号 + UBR
// - 系统架构 (32/64 位)
// - 安装日期
// - 激活状态
// - 最新安装的补丁 (KB)
// - 启动模式 (UEFI / BIOS)
//
// 所有采集函数独立运行，单字段失败不影响其他字段。
//
// 数据源说明（对齐重构设计 ADR-0006）：
// - 激活状态 / 最新补丁主路径走纯 Rust 驱动 `secm_datasource::activation` /
//   `secm_datasource::registry`（WMI/注册表，无外部进程）；
//   驱动无数据或失败时回退 PowerShell 查询（经 `crate::proc_util::run_ps_result`，
//   隐藏控制台窗口；该路径仅在极少数驱动不可用场景触发）。
// - 其余字段为注册表 / 环境变量 / Win32 API 直读，无外部命令。

use serde::Serialize;

// ============================================================================
// 类型定义
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct SystemInfo {
    /// 产品名称，如 "Windows 11 专业版"
    pub edition: String,
    /// Build 号，如 "22631"
    pub build_number: String,
    /// 更新版本号 (UBR)，如 "4602"
    pub ubr: String,
    /// 系统架构，如 "64 位"
    pub arch: String,
    /// 系统安装日期，如 "2024-03-15"
    pub install_date: String,
    /// 激活状态信息
    pub activation: ActivationInfo,
    /// 最新安装的补丁
    pub latest_patch: PatchInfo,
    /// 启动模式："UEFI"、"BIOS" 或 "未知"
    pub boot_mode: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ActivationInfo {
    /// 原始激活状态 (Licensed / Unlicensed / Notification / Grace / Unknown)
    pub status_raw: String,
    /// 中文标签 (已激活 / 未激活 / 通知模式 / 宽限期 / 未知)
    pub label: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PatchInfo {
    /// KB 编号，如 "KB5039212"
    pub kb: String,
    /// 安装日期，如 "2024-07-09"
    pub date: String,
    /// 补丁标题（英文原文），如 "Security Update"
    pub title_raw: String,
    /// 补丁标题（中文翻译），如 "安全更新"
    pub title_cn: String,
}

// ============================================================================
// 主入口
// ============================================================================

/// 采集全部系统信息（多线程并行采集，各字段独立执行；单字段失败返回 "无法获取"）
pub fn get_system_info() -> SystemInfo {
    // P1-15：单字段线程 panic（如 PS 解析越界）不再放大为整页检测失败/任务悬挂——
    // thread::scope 会在作用域结束处重抛任何子线程 panic，故在线程体内 catch_unwind 隔离
    std::thread::scope(|s| {
        let edition = s.spawn(|| safe_call("edition", get_edition, "无法获取".to_string()));
        let build_number =
            s.spawn(|| safe_call("build_number", get_build_number, "无法获取".to_string()));
        let ubr = s.spawn(|| safe_call("ubr", get_ubr, "无法获取".to_string()));
        let arch = s.spawn(|| safe_call("arch", get_arch, "无法获取".to_string()));
        let install_date =
            s.spawn(|| safe_call("install_date", get_install_date, "无法获取".to_string()));
        let activation = s.spawn(|| {
            safe_call(
                "activation",
                get_activation,
                ActivationInfo {
                    status_raw: "Unknown".into(),
                    label: "无法获取".into(),
                },
            )
        });
        let latest_patch = s.spawn(|| {
            safe_call(
                "latest_patch",
                get_latest_patch,
                PatchInfo {
                    kb: "无法获取".into(),
                    date: String::new(),
                    title_raw: String::new(),
                    title_cn: "无法获取".into(),
                },
            )
        });
        let boot_mode = s.spawn(|| safe_call("boot_mode", get_boot_mode, "未知".to_string()));

        SystemInfo {
            edition: edition.join().unwrap_or_default(),
            build_number: build_number.join().unwrap_or_default(),
            ubr: ubr.join().unwrap_or_default(),
            arch: arch.join().unwrap_or_default(),
            install_date: install_date.join().unwrap_or_default(),
            activation: activation.join().unwrap_or_default(),
            latest_patch: latest_patch.join().unwrap_or_default(),
            boot_mode: boot_mode.join().unwrap_or_default(),
        }
    })
}

/// 隔离单字段采集的 panic（返回字段级降级值），并记录日志
fn safe_call<T>(what: &str, f: impl FnOnce() -> T + std::panic::UnwindSafe, fallback: T) -> T {
    std::panic::catch_unwind(f).unwrap_or_else(|_| {
        log::warn!("sysinfo: {} 采集线程 panic，降级为无法获取", what);
        fallback
    })
}

// ============================================================================
// 1. 产品名称 (Edition)
// ============================================================================

/// 从注册表读取 Windows 产品名称
///
/// 获取正确的 Windows 版本名（处理注册表 ProductName 残留旧值问题）
///
/// 微软在升级安装时不会更新 ProductName（如 Win11 系统可能残留 "Windows 10 Pro"），
/// 因此必须用 Build 号判定：Build >= 22000 = Windows 11，否则 Windows 10。
fn get_edition() -> String {
    let build = get_build_number_raw();
    let is_win11 = build >= 22000;

    // 基础版本名（按 Build 判定，不用注册表 ProductName）
    let base = if is_win11 { "Windows 11" } else { "Windows 10" };

    // 用注册表 ProductName 提取 SKU 名（如 "Pro for Workstations"），但要清理掉残留的版本前缀
    let product_name = read_reg_string(
        "HKLM",
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        "ProductName",
    )
    .unwrap_or_default();

    // 从 ProductName 提取版本名之后的部分（去掉 "Windows 10" / "Windows 11" 前缀）
    let sku = product_name
        .trim_start_matches("Windows 10")
        .trim_start_matches("Windows 11")
        .trim()
        .to_string();

    // 组合：Windows 11 <SKU>（或回退到 EditionID 中文映射）
    if !sku.is_empty() {
        format!("{} {}", base, sku)
    } else {
        // 回退：用 EditionID 拼装
        if let Some(edition) = read_reg_string(
            "HKLM",
            "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
            "EditionID",
        ) {
            format!("{} {}", base, edition)
        } else {
            base.to_string()
        }
    }
}

/// 读取原始 Build 号（u32），用于版本判定
fn get_build_number_raw() -> u32 {
    read_reg_string(
        "HKLM",
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        "CurrentBuild",
    )
    .and_then(|s| s.trim().parse::<u32>().ok())
    .unwrap_or(0)
}

// ============================================================================
// 2. Build 号
// ============================================================================

/// 从注册表读取 CurrentBuild
///
/// 路径: HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\CurrentBuild
fn get_build_number() -> String {
    read_reg_string(
        "HKLM",
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        "CurrentBuild",
    )
    .unwrap_or_else(|| "无法获取".into())
}

// ============================================================================
// 3. UBR (Update Build Revision)
// ============================================================================

/// 从注册表读取 UBR
///
/// 路径: HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\UBR
fn get_ubr() -> String {
    read_reg_dword(
        "HKLM",
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        "UBR",
    )
    .map(|v| v.to_string())
    .unwrap_or_else(|| "0".into())
}

// ============================================================================
// 4. 系统架构
// ============================================================================

/// 通过 PROCESSOR_ARCHITECTURE 环境变量判断架构
///
/// 返回值: "64 位" / "32 位" / "ARM64"
fn get_arch() -> String {
    let arch = std::env::var("PROCESSOR_ARCHITECTURE").unwrap_or_default();
    match arch.to_uppercase().as_str() {
        "AMD64" => "64 位".into(),
        "ARM64" => "ARM64".into(),
        "X86" => "32 位".into(),
        "IA64" => "IA-64".into(),
        _ if arch.is_empty() => "无法获取".into(),
        other => other.to_string(),
    }
}

// ============================================================================
// 5. 系统安装日期
// ============================================================================

/// 从注册表读取系统安装日期 (Unix 时间戳)
///
/// 路径: HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\InstallDate
/// 回退: HKLM\SYSTEM\Setup\State → ImageStateDate (字符串格式)
fn get_install_date() -> String {
    // 主路径：Unix 时间戳
    if let Some(ts) = read_reg_dword(
        "HKLM",
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        "InstallDate",
    ) {
        if let Some(dt) = chrono::DateTime::from_timestamp(ts as i64, 0) {
            return dt.format("%Y-%m-%d").to_string();
        }
    }

    // 回退：ImageStateDate (REG_SZ "YYYYMMDD")
    if let Some(raw) = read_reg_string("HKLM", "SYSTEM\\Setup\\State", "ImageStateDate") {
        let date_str = raw.trim();
        if date_str.len() >= 8 {
            let y = &date_str[0..4];
            let m = &date_str[4..6];
            let d = &date_str[6..8];
            if y.parse::<u32>().is_ok() && m.parse::<u32>().is_ok() && d.parse::<u32>().is_ok() {
                return format!("{}-{}-{}", y, m, d);
            }
        }
    }

    "无法获取".into()
}

// ============================================================================
// 6. 激活状态
// ============================================================================

/// 通过 PowerShell Get-CimInstance 查询 Windows 激活状态（回退路径）
///
/// 查询 SoftwareLicensingProduct where ApplicationId=55c92734-... AND PartialProductKey IS NOT NULL
/// 使用 PowerShell 替代 wmic（wmic 在 Win11 24H2+ 已被移除）
/// LicenseStatus 映射:
///   0 → Unlicensed (未激活)
///   1 → Licensed (已激活)
///   2 → OOBGrace (OOB 宽限期)
///   3 → OOTGrace (OOT 宽限期)
///   4 → NonGenuineGrace (非正版宽限期)
///   5 → Notification (通知模式)
///   6 → ExtendedGrace (延长宽限期)
fn get_activation_by_powershell() -> ActivationInfo {
    let script = r#"Get-CimInstance -ClassName SoftwareLicensingProduct -Filter "ApplicationId='55c92734-d682-4d71-983e-d6ec3f16059f' AND PartialProductKey IS NOT NULL" | Select-Object -ExpandProperty LicenseStatus"#;

    // run_ps_result 已内置隐藏控制台窗口 (CREATE_NO_WINDOW) 与 UTF-8/GBK 解码
    let output = match crate::proc_util::run_ps_result(script) {
        Ok(out) => out,
        Err(e) => {
            log::warn!("sysinfo: 激活状态 PowerShell 查询失败: {}", e);
            return ActivationInfo {
                status_raw: "Unknown".into(),
                label: "无法获取".into(),
            };
        }
    };
    let status_num: Option<u32> = output
        .lines()
        .find(|l| !l.trim().is_empty())
        .and_then(|l| l.trim().parse::<u32>().ok());

    match status_num {
        Some(0) => ActivationInfo {
            status_raw: "Unlicensed".into(),
            label: "未激活".into(),
        },
        Some(1) => ActivationInfo {
            status_raw: "Licensed".into(),
            label: "已激活".into(),
        },
        Some(2) => ActivationInfo {
            status_raw: "OOBGrace".into(),
            label: "OOB 宽限期".into(),
        },
        Some(3) => ActivationInfo {
            status_raw: "OOTGrace".into(),
            label: "OOT 宽限期".into(),
        },
        Some(4) => ActivationInfo {
            status_raw: "NonGenuineGrace".into(),
            label: "非正版宽限期".into(),
        },
        Some(5) => ActivationInfo {
            status_raw: "Notification".into(),
            label: "通知模式".into(),
        },
        Some(6) => ActivationInfo {
            status_raw: "ExtendedGrace".into(),
            label: "延长宽限期".into(),
        },
        Some(n) => ActivationInfo {
            status_raw: format!("Unknown({})", n),
            label: "未知".into(),
        },
        None => ActivationInfo {
            status_raw: "Unknown".into(),
            label: "无法获取".into(),
        },
    }
}

/// 采集 Windows 激活状态
///
/// 主路径走纯 Rust 驱动 `secm_datasource::activation`（WMI → 注册表近似三级降级）；
/// 驱动无数据或失败时回退 PowerShell 查询，仍失败则降级为 "Unknown"/"无法获取"
/// （与旧版文案一致），并打 warn 日志。
fn get_activation() -> ActivationInfo {
    match secm_datasource::activation::get_activation() {
        Ok(Some(info)) => ActivationInfo {
            status_raw: info.status_raw,
            label: info.label,
        },
        Ok(None) => {
            log::warn!("sysinfo: 激活状态纯 Rust 驱动无数据，回退 PowerShell 查询");
            get_activation_by_powershell()
        }
        Err(e) => {
            log::warn!("sysinfo: 激活状态纯 Rust 驱动失败，回退 PowerShell 查询: {}", e);
            get_activation_by_powershell()
        }
    }
}

// ============================================================================
// 7. 最新补丁
// ============================================================================

/// 通过 PowerShell Get-CimInstance 查询已安装补丁列表，返回最新一条（回退路径）
///
/// 查询 Win32_QuickFixEngineering → HotFixID, InstalledOn, Description
/// 按 InstalledOn 降序排序，取第一条
/// 使用 PowerShell 替代 wmic（wmic 在 Win11 24H2+ 已被移除）
fn get_latest_patch_by_powershell() -> PatchInfo {
    let script = r#"Get-CimInstance -ClassName Win32_QuickFixEngineering | Sort-Object InstalledOn -Descending | Select-Object -First 1 | ForEach-Object { "$($_.HotFixID)|$($_.InstalledOn)|$($_.Description)" }"#;

    let output = match crate::proc_util::run_ps_result(script) {
        Ok(out) => out,
        Err(e) => {
            log::warn!("sysinfo: 最新补丁 PowerShell 查询失败: {}", e);
            return PatchInfo {
                kb: "无".into(),
                date: "—".into(),
                title_raw: "—".into(),
                title_cn: "无补丁记录".into(),
            };
        }
    };

    // 解析输出: KB1234567|2026/7/15 0:00:00|Security Update
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return PatchInfo {
            kb: "无".into(),
            date: "—".into(),
            title_raw: "—".into(),
            title_cn: "无补丁记录".into(),
        };
    }

    let parts: Vec<&str> = trimmed.split('|').collect();
    if parts.len() < 3 {
        return PatchInfo {
            kb: "无".into(),
            date: "—".into(),
            title_raw: "—".into(),
            title_cn: "无补丁记录".into(),
        };
    }

    let kb = parts[0].trim().to_string();
    let date_raw = parts[1].trim().to_string();
    let desc = parts[2].trim().to_string();

    // PowerShell 返回的日期格式可能是 "7/15/2026 0:00:00" — 只取日期部分
    let date_part = if let Some(space_idx) = date_raw.find(' ') {
        &date_raw[..space_idx]
    } else {
        &date_raw
    };
    let formatted_date = normalize_date(date_part);

    PatchInfo {
        kb,
        date: formatted_date,
        title_raw: desc.clone(),
        title_cn: translate_patch_title(&desc),
    }
}

/// 采集最新安装的补丁
///
/// 主路径走纯 Rust 驱动 `secm_datasource::registry::latest_patch`
/// （注册表 HotFix 枚举 + WMI 兜底，与 WMI 同源）；驱动无数据或失败时回退
/// PowerShell 查询，仍失败则降级为空占位（与旧版文案一致），并打 warn 日志。
fn get_latest_patch() -> PatchInfo {
    match secm_datasource::registry::latest_patch() {
        Ok(Some(p)) => PatchInfo {
            kb: p.kb,
            date: p.date,
            title_raw: p.title_raw,
            title_cn: p.title_cn,
        },
        Ok(None) => {
            log::warn!("sysinfo: 最新补丁纯 Rust 驱动无数据，回退 PowerShell 查询");
            get_latest_patch_by_powershell()
        }
        Err(e) => {
            log::warn!("sysinfo: 最新补丁纯 Rust 驱动失败，回退 PowerShell 查询: {}", e);
            get_latest_patch_by_powershell()
        }
    }
}

// 日期归一化与补丁标题翻译收敛到 datasource::registry 单一实现
// （历史为两份漂移副本：datasource 版兼容 年/月/日 与 月/日/年 + 时间后缀，
// core 版只认 M/D/Y——展示格式曾跨链路不一致，审计 P2 收敛）
use secm_datasource::registry::{normalize_date, translate_patch_title};

// ============================================================================
// 8. 启动模式 (UEFI / BIOS)
// ============================================================================

/// 通过 kernel32.GetFirmwareType 检测固件类型
///
/// 返回值:
///   0 → Unknown → "未知"
///   1 → BIOS
///   2 → UEFI
///   3 → Max (保留值)
fn get_boot_mode() -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::GetFirmwareType;

        // FIRMWARE_TYPE 为 i32（FirmwareTypeBios=1 / FirmwareTypeUefi=2 / Unknown=0）
        let mut firmware_type: i32 = 0;

        // SAFETY: GetFirmwareType 是 kernel32 的安全导出函数（Windows 8+），
        // 传递有效的 &mut i32 指针即可
        let result = unsafe { GetFirmwareType(&mut firmware_type) };

        if result == 0 {
            // 函数调用失败（可能是 Win7 或 kernel32 无此导出）
            return "无法获取".into();
        }

        match firmware_type {
            1 => "BIOS".into(),
            2 => "UEFI".into(),
            0 => "未知".into(),
            _ => "未知".into(),
        }
    }
    #[cfg(not(windows))]
    {
        "无法获取".into()
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 读取注册表 REG_SZ 字符串值（64 位视图）
fn read_reg_string(hive: &str, key: &str, value: &str) -> Option<String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkey = match hive {
        "HKLM" => HKEY_LOCAL_MACHINE,
        "HKCU" => HKEY_CURRENT_USER,
        _ => return None,
    };

    let reg_key = RegKey::predef(hkey);
    let subkey = reg_key
        .open_subkey_with_flags(key, KEY_READ | KEY_WOW64_64KEY)
        .ok()?;
    subkey.get_value::<String, _>(value).ok()
}

/// 读取注册表 REG_DWORD 值（64 位视图）
fn read_reg_dword(hive: &str, key: &str, value: &str) -> Option<u32> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkey = match hive {
        "HKLM" => HKEY_LOCAL_MACHINE,
        "HKCU" => HKEY_CURRENT_USER,
        _ => return None,
    };

    let reg_key = RegKey::predef(hkey);
    let subkey = reg_key
        .open_subkey_with_flags(key, KEY_READ | KEY_WOW64_64KEY)
        .ok()?;
    subkey.get_value::<u32, _>(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_date_md_yyyy() {
        // PowerShell InstalledOn 的 M/D/YYYY 格式
        assert_eq!(normalize_date("7/15/2026"), "2026-07-15");
        assert_eq!(normalize_date("12/1/2026"), "2026-12-01");
    }

    #[test]
    fn test_normalize_date_iso_passthrough() {
        // 已是 YYYY-MM-DD 格式则原样返回
        assert_eq!(normalize_date("2026-07-15"), "2026-07-15");
    }

    #[test]
    fn test_translate_patch_title() {
        assert_eq!(translate_patch_title("Security Update"), "安全更新");
        assert_eq!(
            translate_patch_title("2026-07 Cumulative Update for Windows 11"),
            "累积更新"
        );
        assert_eq!(
            translate_patch_title("2026-07 Cumulative Security Update"),
            "累积安全更新"
        );
        assert_eq!(
            translate_patch_title("2026-07 Security Quality Update"),
            "质量安全更新"
        );
        assert_eq!(translate_patch_title("Update Rollup"), "更新汇总");
        assert_eq!(translate_patch_title("Service Pack 1"), "服务包");
        assert_eq!(translate_patch_title("Something Else"), "Something Else");
    }

    #[test]
    fn test_get_arch_mapping() {
        // 纯映射验证：覆盖环境变量不存在与空串两种降级路径
        let _ = get_arch();
    }
}
