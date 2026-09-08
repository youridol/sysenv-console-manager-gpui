//! NVIDIA 显卡电源管理模式（NVAPI DRS）模块
//!
//! 移植自上游 `src-tauri/src/nvidia_drs.rs`（语义 1:1 对齐），替换 `debug_warn!` 为 `log::warn!`。
//!
//! 读取/设置 NVIDIA 控制面板「管理 3D 设置 → 电源管理模式」（Power management mode）：
//! - 存储位置：DRS（Display Driver Settings）全局 profile，NVIDIA 驱动私有数据库
//!   （`%ProgramData%\NVIDIA Corporation\Drs\nvdrsdb0.bin`，DVS 加密格式，无注册表路径）
//! - 访问方式：加载 `nvapi64.dll` → `nvapi_QueryInterface(ordinal)` 获取 DRS 接口
//!   （序数来自 NVIDIA SDK 提取的 nvapi-sys 序数表；设置名 `Power management mode`
//!   按名动态解析 settingId，上游 RTX 2080 Ti + 驱动 595.79 实测读写闭环验证通过）
//! - 值域（DWORD，NVIDIA App featureEnum 权威确认）：0 = Adaptive（自适应）、
//!   1 = Prefer Maximum Performance（最高性能优先）、5 = Optimal Power（最佳功率）；
//!   其余值（如旧版误写的 2）无效，控制面板会回退显示默认。
//!
//! 线程模型：每次调用独立 CreateSession/LoadSettings/DestroySession，线程安全，
//! 调用方须在后台线程执行（上层 UI 已按后台任务编排）。
//! 错误处理：全部失败路径返回 `Err(String)`，含 API 名 + 错误码（日志完整）。

use std::ffi::c_void;
use std::mem::size_of;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

// ── NVAPI 接口序数（nvapi-sys 序数表，与 nvapi64.dll 实测一致）──
/// NvAPI_Initialize（入口初始化，NVAPI 契约要求最先调用）
const ORD_INITIALIZE: u32 = 0x0150E828;
const ORD_DRS_CREATE_SESSION: u32 = 0x0694d52e;
const ORD_DRS_DESTROY_SESSION: u32 = 0x0dad9cff8;
const ORD_DRS_LOAD_SETTINGS: u32 = 0x375dbd6b;
const ORD_DRS_SAVE_SETTINGS: u32 = 0x0fcbc7e14;
const ORD_DRS_GET_CURRENT_GLOBAL_PROFILE: u32 = 0x617bff9f;
const ORD_DRS_GET_SETTING: u32 = 0x73bf8338;
const ORD_DRS_SET_SETTING: u32 = 0x577dd202;
const ORD_DRS_GET_SETTING_ID_FROM_NAME: u32 = 0x0cb7309cd;

/// 电源管理模式设置名（DRS 全局 profile 内，按名解析 settingId，避免硬编码 id）
const SETTING_POWER_MANAGEMENT_MODE: &str = "Power management mode";

const NVAPI_OK: i32 = 0;
/// NVAPI_APPLICATION_PROFILE_NOT_FOUND（0xFFFFFF60 = -160）：
/// NvAPI_DRS_GetSetting 对「全局 profile 中不存在的设置项」返回该错误——
/// 新装 NVIDIA 驱动 / 干净配置的机器（未在控制面板/NVIDIA App 修改过电源模式）
/// 全局 profile 无该设置项，属预期状态而非故障。
const NVAPI_APPLICATION_PROFILE_NOT_FOUND: i32 = -160;
const NVAPI_BINARY_DATA_MAX: usize = 4096;
const NVAPI_UNICODE_STRING_MAX: usize = 2048;

/// NVIDIA 显卡电源管理模式（值域与 NVIDIA 控制面板/NVIDIA App 一致，
/// 权威来源：NVIDIA App `featureEnum`：
/// `NVCPLAPI_VALUE_POWER_MANAGEMENT_MODE_ADAPTIVE=0`、
/// `NVCPLAPI_VALUE_POWER_MANAGEMENT_MODE_MAX=1`、
/// `NVCPLAPI_VALUE_POWER_MANAGEMENT_MODE_OPTIMAL_POWER=5`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NvidiaPowerMode {
    /// 自适应（Adaptive）—— DRS 值 0
    Adaptive = 0,
    /// 最高性能优先（Prefer Maximum Performance）—— DRS 值 1
    MaxPerformance = 1,
    /// 最佳功率（Optimal Power）—— DRS 值 5（非 2；2 为无效值，旧版映射错误）
    Optimal = 5,
}

impl NvidiaPowerMode {
    /// 从 DRS 原始 DWORD 值解析；非法值（如旧版误写的 2）返回 None
    pub fn from_raw(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Adaptive),
            1 => Some(Self::MaxPerformance),
            5 => Some(Self::Optimal),
            _ => None,
        }
    }

    /// 中文显示名（UI 三档选项）
    pub fn label_cn(&self) -> &'static str {
        match self {
            Self::Optimal => "最佳功率",
            Self::MaxPerformance => "最高性能优先",
            Self::Adaptive => "自适应",
        }
    }
}

// ── DRS 数据结构（布局与 nvapi.h 一致，size=12320 校验通过）──

/// 值 union（`{ NvU32 u32Value; NVDRS_BINARY_SETTING binaryValue; NvU64 u64Value; }`）
///
/// ⚠ 重叠语义（真机诊断定论）：u32 视图与 binary 视图共享**前 4 字节**——
/// DWORD 类型设置的真实值就在 union 起始 4 字节（`u32_value`）；binary 视图的
/// `valueLength` 与之重叠（仅 BINARY 类型设置有意义）。旧实现把 `value_length`
/// 恒写 4 再把值写入 `value_data[0..4]`（union 偏移 4..8），实际把 u32 值位写成
/// 常量 4 → 驱动收到无效值静默回退默认（"写入不生效"根因）。
#[repr(C)]
#[derive(Clone, Copy)]
struct NvDrsUnion {
    /// u32 视图：DWORD 类型设置的真实读写位（union 偏移 0..4）
    u32_value: u32,
    /// binary 视图数据区（BINARY 类型设置时 data[0..4] 为长度、其后为数据；
    /// 仅 4092 字节有效重叠区，其余为保留填充，维持 union 总大小 4100）
    data: [u8; NVAPI_BINARY_DATA_MAX],
}

/// `NVDRS_BINARY_SETTING` 兼容视图（C 端：`{ NvU32 valueLength; NvU8 data[4096]; }`）；
/// 读写 DWORD 值一律走 `NvDrsUnion::u32_value`，禁止经此视图解释 DWORD。
type NvDrsBinarySetting = NvDrsUnion;

/// `NVDRS_SETTING_V1`（pack(4)）。`settingName` 为 UTF-16LE 数组。
#[repr(C, packed(4))]
#[derive(Clone, Copy)]
struct NvDrsSetting {
    version: u32,
    setting_name: [u16; NVAPI_UNICODE_STRING_MAX],
    setting_id: u32,
    setting_type: u32, // 0=DWORD 1=BINARY 2=STRING 3=WSTRING 4=QWORD
    setting_location: u32,
    is_current_predefined: u32,
    is_predefined_valid: u32,
    // union{ u32 | binary(4100) | u64 } —— binary 最大，DWORD 值位于其起始 4 字节
    binary_predefined_value: NvDrsBinarySetting,
    binary_current_value: NvDrsBinarySetting,
}

// ── NVAPI DRS 接口（函数指针集合）──

/// DRS 会话相关函数指针（`NvAPI_QueryInterface(ordinal)` 获取）
struct NvDrsApi {
    create_session: unsafe extern "C" fn(*mut u64) -> i32,
    destroy_session: unsafe extern "C" fn(u64) -> i32,
    load_settings: unsafe extern "C" fn(u64) -> i32,
    save_settings: unsafe extern "C" fn(u64) -> i32,
    get_current_global_profile: unsafe extern "C" fn(u64, *mut u64) -> i32,
    get_setting: unsafe extern "C" fn(u64, u64, u32, *mut NvDrsSetting) -> i32,
    set_setting: unsafe extern "C" fn(u64, u64, *const NvDrsSetting) -> i32,
    get_setting_id_from_name: unsafe extern "C" fn(*const u16, *mut u32) -> i32,
}

// ── 加载与单例 ──

/// 已加载的 DRS 接口（进程级单例；失败缓存错误信息避免重复加载）
static NVDRS_API: OnceLock<Result<NvDrsApi, String>> = OnceLock::new();

/// 加载 nvapi64.dll 并解析全部 DRS 函数指针（只执行一次）
fn load_nvdrs_api() -> Result<&'static NvDrsApi, String> {
    NVDRS_API
        .get_or_init(|| {
            // 声明 kernel32 导出（项目既有风格：extern "system" 直调，不引入新依赖）
            extern "system" {
                fn LoadLibraryW(lp_file_name: *const u16) -> *mut c_void;
                fn GetProcAddress(h_module: *mut c_void, lp_proc_name: *const u8) -> *mut c_void;
            }
            let mut dll_path: Vec<u16> = "nvapi64.dll".encode_utf16().collect();
            dll_path.push(0);
            // SAFETY: dll_path 为以 NUL 结尾的 UTF-16 字符串，LoadLibraryW 只读该缓冲
            let dll = unsafe { LoadLibraryW(dll_path.as_ptr()) };
            if dll.is_null() {
                return Err(format!(
                    "LoadLibraryW(nvapi64.dll) 失败: 未检测到 NVIDIA 显卡或驱动未安装 (err={})",
                    std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
                ));
            }
            // SAFETY: c"nvapi_QueryInterface" 为静态 NUL 结尾 ASCII C 字符串
            let qi_raw = unsafe { GetProcAddress(dll, c"nvapi_QueryInterface".as_ptr() as *const u8) };
            if qi_raw.is_null() {
                return Err("GetProcAddress(nvapi_QueryInterface) 失败: nvapi64.dll 缺少入口".into());
            }
            // SAFETY: nvapi_QueryInterface 签名固定为 (u32) -> void*（NVAPI 公开契约）
            let qi: unsafe extern "C" fn(u32) -> *mut c_void =
                unsafe { std::mem::transmute(qi_raw) };

            // NvAPI_Initialize（强制要求）：NVAPI 官方契约要求调用任何其他 NvAPI 函数前
            // 必须先初始化。跳过初始化时部分函数表面返回成功但内部状态不完整
            // （实测：DRS SetSetting/SaveSettings 不落盘——写入后新会话重读仍为旧值）。
            // SAFETY: NvAPI_Initialize 签名固定为 () -> i32（NVAPI 公开契约，序数 0x0150E828）
            let init: unsafe extern "C" fn() -> i32 =
                unsafe { std::mem::transmute(qi(ORD_INITIALIZE)) };
            let init_rc = unsafe { init() };
            if init_rc != NVAPI_OK {
                return Err(format!("NvAPI_Initialize 失败: rc=0x{:08X}", init_rc));
            }

            // 通过 QueryInterface 按序数解析各 DRS 函数（显式类型标注 = 签名与 nvapi.h 一致）
            // SAFETY: 每个函数指针均来自 nvapi_QueryInterface(ordinal)，序数与
            // 官方 SDK 提取的 nvapi-sys 序数表一致，签名与 nvapi.h 声明一致；
            // 上游实机（RTX 2080 Ti, 驱动 595.79）全部解析成功
            let create_session = unsafe {
                std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut u64) -> i32>(qi(
                    ORD_DRS_CREATE_SESSION,
                ))
            };
            let destroy_session = unsafe {
                std::mem::transmute::<*mut c_void, unsafe extern "C" fn(u64) -> i32>(qi(
                    ORD_DRS_DESTROY_SESSION,
                ))
            };
            let load_settings = unsafe {
                std::mem::transmute::<*mut c_void, unsafe extern "C" fn(u64) -> i32>(qi(
                    ORD_DRS_LOAD_SETTINGS,
                ))
            };
            let save_settings = unsafe {
                std::mem::transmute::<*mut c_void, unsafe extern "C" fn(u64) -> i32>(qi(
                    ORD_DRS_SAVE_SETTINGS,
                ))
            };
            let get_current_global_profile = unsafe {
                std::mem::transmute::<*mut c_void, unsafe extern "C" fn(u64, *mut u64) -> i32>(qi(
                    ORD_DRS_GET_CURRENT_GLOBAL_PROFILE,
                ))
            };
            let get_setting = unsafe {
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "C" fn(u64, u64, u32, *mut NvDrsSetting) -> i32,
                >(qi(ORD_DRS_GET_SETTING))
            };
            let set_setting = unsafe {
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "C" fn(u64, u64, *const NvDrsSetting) -> i32,
                >(qi(ORD_DRS_SET_SETTING))
            };
            let get_setting_id_from_name = unsafe {
                std::mem::transmute::<
                    *mut c_void,
                    unsafe extern "C" fn(*const u16, *mut u32) -> i32,
                >(qi(ORD_DRS_GET_SETTING_ID_FROM_NAME))
            };

            Ok(NvDrsApi {
                create_session,
                destroy_session,
                load_settings,
                save_settings,
                get_current_global_profile,
                get_setting,
                set_setting,
                get_setting_id_from_name,
            })
        })
        .as_ref()
        .map_err(|e| e.clone())
}

/// `MAKE_NVAPI_VERSION(type, 1)`：sizeof(struct) | (1 << 16)
fn nvapi_version() -> u32 {
    (size_of::<NvDrsSetting>() as u32) | (1 << 16)
}

/// 在 DRS 会话内执行闭包：创建会话 → 加载设置 → 取全局 profile → 闭包 → 销毁会话
/// 闭包参数：(&NvDrsApi, session, hprofile)
fn with_session<T>(f: impl FnOnce(&NvDrsApi, u64, u64) -> Result<T, String>) -> Result<T, String> {
    let api = load_nvdrs_api()?;
    let mut session: u64 = 0;
    // SAFETY: session 为输出参数，指向有效 u64 栈变量
    let rc = unsafe { (api.create_session)(&mut session) };
    if rc != NVAPI_OK || session == 0 {
        return Err(format!(
            "NvAPI_DRS_CreateSession 失败: rc=0x{:08X} (err={})",
            rc,
            std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
        ));
    }
    // SAFETY: session 句柄由 create_session 返回且非 0
    let rc = unsafe { (api.load_settings)(session) };
    if rc != NVAPI_OK {
        // SAFETY: 销毁已创建的有效会话句柄
        unsafe { (api.destroy_session)(session) };
        return Err(format!("NvAPI_DRS_LoadSettings 失败: rc=0x{:08X}", rc));
    }

    let mut hprofile: u64 = 0;
    // SAFETY: hprofile 为输出参数，指向有效 u64 栈变量
    let rc = unsafe { (api.get_current_global_profile)(session, &mut hprofile) };
    if rc != NVAPI_OK || hprofile == 0 {
        // SAFETY: 销毁已创建的有效会话句柄
        unsafe { (api.destroy_session)(session) };
        return Err(format!(
            "NvAPI_DRS_GetCurrentGlobalProfile 失败: rc=0x{:08X}",
            rc
        ));
    }

    let result = f(api, session, hprofile);

    // SAFETY: 会话句柄有效；销毁失败不影响主结果（日志由调用方记录）
    unsafe { (api.destroy_session)(session) };
    result
}

/// 解析「电源管理模式」的 settingId（按设置名动态解析，避免硬编码 id）
fn power_mode_setting_id(api: &NvDrsApi) -> Result<u32, String> {
    let mut name: Vec<u16> = SETTING_POWER_MANAGEMENT_MODE.encode_utf16().collect();
    name.push(0);
    let mut id: u32 = 0;
    // SAFETY: name 为 NUL 结尾 UTF-16 缓冲；id 为输出参数
    let rc = unsafe { (api.get_setting_id_from_name)(name.as_ptr(), &mut id) };
    if rc != NVAPI_OK {
        return Err(format!(
            "NvAPI_DRS_GetSettingIdFromName({}) 失败: rc=0x{:08X}（驱动不支持或 DRS 不可用）",
            SETTING_POWER_MANAGEMENT_MODE, rc
        ));
    }

    Ok(id)
}

/// 读取 DWORD 类型设置的当前值：union 起始 4 字节（u32 视图，小端）。
///
/// 语义修复（真机诊断定论）：旧实现从 `value_data[0..4]`（union 偏移 4..8）读值——
/// 该处并非 u32 值位；真实值在 union 起始 4 字节（旧字段名 value_length 处）。
fn dword_current_value(s: &NvDrsSetting) -> Option<u32> {
    Some(s.binary_current_value.u32_value)
}

/// 写入 DWORD 类型设置的当前值：union 起始 4 字节（u32 视图）。
///
/// 语义修复：旧实现把 `value_length` 恒写 4（恰好等于坏写遗留的旧值）再把值写入
/// value_data 偏移 4..8 —— u32 值位被写成常量 4，驱动按无效值静默回退默认。
fn set_dword_current_value(s: &mut NvDrsSetting, value: u32) {
    s.binary_current_value.u32_value = value;
}

/// 是否为「设置项不存在」错误（0xFFFFFF60）：目标机器 DRS 全局 profile 无该设置，
/// 属预期状态（驱动默认），读取应返回默认值、设置应新建设置项而非报错。
fn is_profile_not_found(rc: i32) -> bool {
    rc == NVAPI_APPLICATION_PROFILE_NOT_FOUND
}

// ── 公开 API ──

/// 读取 NVIDIA 显卡电源管理模式
///
/// 目标机器降级：全局 DRS profile 无该设置项时（新装驱动/干净配置，
/// NvAPI_DRS_GetSetting 返回 NVAPI_APPLICATION_PROFILE_NOT_FOUND=0xFFFFFF60），
/// 返回驱动默认值 Adaptive（NVIDIA 出厂默认），不再报错——用户可在 UI 正常切换。
pub fn get_power_mode() -> Result<NvidiaPowerMode, String> {
    with_session(|api, session, hprofile| {
        let id = power_mode_setting_id(api)?;
        let mut setting: NvDrsSetting = unsafe { std::mem::zeroed() };
        setting.version = nvapi_version();
        // SAFETY: setting 为有效栈缓冲，version 已设置（上游探针实测通过）；session/hprofile 句柄有效
        let rc = unsafe { (api.get_setting)(session, hprofile, id, &mut setting) };
        if rc != NVAPI_OK {
            if is_profile_not_found(rc) {
                // 目标机器 profile 无此设置项：驱动默认 = 自适应，降级返回
                log::warn!(
                    "[nvidia_drs] get_power_mode: 全局 profile 无电源管理模式设置项（rc=0x{:08X}），返回驱动默认自适应",
                    rc
                );
                return Ok(NvidiaPowerMode::Adaptive);
            }
            return Err(format!("NvAPI_DRS_GetSetting 失败: rc=0x{:08X}", rc));
        }
        // 读取 u32 值位；无效值（如旧版坏写遗留的 4）按驱动语义降级：
        // 驱动对无效值一律回退默认自适应，故读取侧同样报告自适应并记录日志（UI 可正常修复）
        match dword_current_value(&setting) {
            Some(v) => match NvidiaPowerMode::from_raw(v) {
                Some(m) => Ok(m),
                None => {
                    log::warn!(
                        "[nvidia_drs] get_power_mode: 存储值 {} 无效（疑似坏写遗留），按驱动默认自适应报告",
                        v
                    );
                    Ok(NvidiaPowerMode::Adaptive)
                }
            },
            None => Err("NVIDIA 电源管理模式读取失败（union 值位为空）".to_string()),
        }
    })
}

/// 设置 NVIDIA 显卡电源管理模式（写入 DRS 并保存）
///
/// 标准流程（与 NVIDIA DRS 示例一致）：先 GetSetting 取完整结构（含 settingName），
/// 再改值并标记 isCurrentPredefined=0（用户自定义值），最后 SetSetting + SaveSettings。
///
/// u32 值位修复（真机定论）：DWORD 类型设置的真实值在值 union 起始 4 字节
/// （u32CurrentValue）；旧实现误写 `value_length=4` + value_data 偏移 4..8，
/// u32 值位被写成常量 4（无效值）→ 驱动静默回退默认 → "写入不生效"。
///
/// 验证会话分离：写入（Get→Set→Save）与验证（新会话 LoadSettings→Get）使用两个
/// 独立会话，验证会话从盘上 DRS 数据库读取持久化真值（防任何形式的脏缓存）。
///
/// 目标机器降级：GetSetting 返回 NVAPI_APPLICATION_PROFILE_NOT_FOUND
/// （0xFFFFFF60，全局 profile 无该设置项）时，构造完整设置项（settingName + settingId +
/// DWORD 类型 + 值）直接 SetSetting——NVIDIA DRS 允许为 profile 新增设置项。
pub fn set_power_mode(mode: NvidiaPowerMode) -> Result<(), String> {
    // ── 会话 1：读取完整结构 → 改值 → Set → Save ──
    with_session(|api, session, hprofile| {
        let id = power_mode_setting_id(api)?;
        // 先 Get：取得完整 setting（settingName/location 等字段由驱动填充，避免空结构写入）
        let mut setting: NvDrsSetting = unsafe { std::mem::zeroed() };
        setting.version = nvapi_version();
        // SAFETY: setting 为有效栈缓冲，version 已设置（上游探针实测通过）；session/hprofile 句柄有效
        let rc = unsafe { (api.get_setting)(session, hprofile, id, &mut setting) };
        if rc != NVAPI_OK && !is_profile_not_found(rc) {
            return Err(format!("NvAPI_DRS_GetSetting 失败: rc=0x{:08X}", rc));
        }
        if is_profile_not_found(rc) {
            // 目标机器全局 profile 无该设置项：构造完整设置项（新建设置）
            log::warn!(
                "[nvidia_drs] set_power_mode: 全局 profile 无电源管理模式设置项（rc=0x{:08X}），构造新建设置项写入",
                rc
            );
            setting = unsafe { std::mem::zeroed() };
            setting.version = nvapi_version();
            // settingName：按名写入（与 GetSettingIdFromName 解析同一名称）
            let name_utf16: Vec<u16> = SETTING_POWER_MANAGEMENT_MODE.encode_utf16().collect();
            for (i, ch) in name_utf16.iter().copied().enumerate() {
                if i >= NVAPI_UNICODE_STRING_MAX - 1 {
                    break;
                }
                setting.setting_name[i] = ch;
            }
            setting.setting_id = id;
            setting.setting_type = 0; // NVDRS_DWORD_TYPE
            setting.is_current_predefined = 0; // 用户自定义值
        }
        // 修改值并标记为用户自定义值（isCurrentPredefined=0，驱动按用户值应用）
        set_dword_current_value(&mut setting, mode as u32);
        setting.is_current_predefined = 0;
        // SAFETY: setting 由 GetSetting 完整填充（或按新建设置项构造）后仅改值字段
        let rc = unsafe { (api.set_setting)(session, hprofile, &setting) };
        if rc != NVAPI_OK {
            return Err(format!(
                "NvAPI_DRS_SetSetting({}) 失败: rc=0x{:08X}",
                mode.label_cn(),
                rc
            ));
        }
        // SAFETY: 会话句柄有效
        let rc = unsafe { (api.save_settings)(session) };
        if rc != NVAPI_OK {
            return Err(format!("NvAPI_DRS_SaveSettings 失败: rc=0x{:08X}", rc));
        }
        Ok(())
    })?;

    // ── 会话 2（新会话重读盘上 DRS 数据库）：验证持久化真值，防驱动静默拒绝 ──
    // 注意：不复用会话 1 —— SaveSettings 后同会话 GetSetting 可能返回旧缓存值（非真值）。
    let actual = with_session(|api, session, hprofile| {
        let id = power_mode_setting_id(api)?;
        let mut verify: NvDrsSetting = unsafe { std::mem::zeroed() };
        verify.version = nvapi_version();
        // SAFETY: verify 为有效栈缓冲；session/hprofile 句柄有效
        let rc = unsafe { (api.get_setting)(session, hprofile, id, &mut verify) };
        if rc != NVAPI_OK {
            if is_profile_not_found(rc) {
                return Ok(None); // 设置项不存在 → 未持久化
            }
            return Err(format!(
                "写入后验证读取失败: NvAPI_DRS_GetSetting rc=0x{:08X}",
                rc
            ));
        }
        Ok(dword_current_value(&verify))
    })?;
    if actual != Some(mode as u32) {
        return Err(format!(
            "写入后验证不一致（新会话读盘）: 期望 {}（{}），实际 {:?}（{}）",
            mode as u32,
            mode.label_cn(),
            actual,
            actual
                .and_then(NvidiaPowerMode::from_raw)
                .map(|m| m.label_cn())
                .unwrap_or("未知")
        ));
    }
    Ok(())
}

/// DRS 写入全链路诊断（支撑工具：输出各阶段原始 rc 与 NVDRS_SETTING 关键字段）
///
/// 用于真机排查"写入不持久化/验证不一致"类问题。执行顺序：Get → 改值 → Set →
/// Save → 同会话 Get → 新会话 Get，全部原始字段随文本返回。不改回系统状态
/// （调用方自行决定恢复值）。
#[doc(hidden)]
pub fn diagnostic_roundtrip(target_value: u32) -> String {
    let mut report = String::new();
    // ── 阶段 1：会话 A 读取 ──
    let stage_a = with_session(|api, session, hprofile| {
        let id = power_mode_setting_id(api)?;
        let mut s: NvDrsSetting = unsafe { std::mem::zeroed() };
        s.version = nvapi_version();
        // SAFETY: s 为有效栈缓冲；session/hprofile 句柄有效
        let rc = unsafe { (api.get_setting)(session, hprofile, id, &mut s) };
        Ok((rc, id, setting_dump(rc, &s, "Get(会话A 初读)")))
    });
    match stage_a {
        Err(e) => return format!("会话A 失败: {}", e),
        Ok((_rc_a, id, dump_a)) => {
            report.push_str(&dump_a);
            report.push('\n');
            // ── 阶段 2：会话 A 内 Get→改值→Set→Save→同会话 Get ──
            let stage_b = with_session(|api, session, hprofile| {
                let mut s: NvDrsSetting = unsafe { std::mem::zeroed() };
                s.version = nvapi_version();
                // SAFETY: s 为有效栈缓冲；session/hprofile 句柄有效
                let rc_get = unsafe { (api.get_setting)(session, hprofile, id, &mut s) };
                let constructed = is_profile_not_found(rc_get);
                if constructed {
                    s = unsafe { std::mem::zeroed() };
                    s.version = nvapi_version();
                    let name_utf16: Vec<u16> =
                        SETTING_POWER_MANAGEMENT_MODE.encode_utf16().collect();
                    for (i, ch) in name_utf16.iter().copied().enumerate() {
                        if i >= NVAPI_UNICODE_STRING_MAX - 1 {
                            break;
                        }
                        s.setting_name[i] = ch;
                    }
                    s.setting_id = id;
                    s.setting_type = 0;
                }
                s.setting_location = 0; // NVDRS_CURRENT_PROFILE_LOCATION（写入当前 profile）
                set_dword_current_value(&mut s, target_value);
                s.is_current_predefined = 0;
                // SAFETY: s 已按 Get 结果或新建设置项构造
                let rc_set = unsafe { (api.set_setting)(session, hprofile, &s) };
                // SAFETY: 会话句柄有效
                let rc_save = unsafe { (api.save_settings)(session) };
                let mut v: NvDrsSetting = unsafe { std::mem::zeroed() };
                v.version = nvapi_version();
                // SAFETY: v 为有效栈缓冲；session/hprofile 句柄有效
                let rc_reget = unsafe { (api.get_setting)(session, hprofile, id, &mut v) };
                Ok(format!(
                    "rc_get=0x{:08X}(constructed={}) rc_set=0x{:08X} rc_save=0x{:08X}\n{}\n{}",
                    rc_get,
                    constructed,
                    rc_set,
                    rc_save,
                    setting_dump(rc_set, &s, "Set 提交结构"),
                    setting_dump(rc_reget, &v, "Get(会话A 同会话回读)")
                ))
            });
            match stage_b {
                Err(e) => report.push_str(&format!("会话B 失败: {}", e)),
                Ok(text) => report.push_str(&text),
            }
            // ── 阶段 3：新会话 B 读盘验证 ──
            let stage_c = with_session(|api, session, hprofile| {
                let mut s: NvDrsSetting = unsafe { std::mem::zeroed() };
                s.version = nvapi_version();
                // SAFETY: s 为有效栈缓冲；session/hprofile 句柄有效
                let rc = unsafe { (api.get_setting)(session, hprofile, id, &mut s) };
                Ok(setting_dump(rc, &s, "Get(新会话B 读盘)"))
            });
            match stage_c {
                Err(e) => report.push_str(&format!("\n新会话失败: {}", e)),
                Ok(text) => report.push_str(&format!("\n{}", text)),
            }
        }
    }
    report
}

/// 单条 NVDRS_SETTING 的关键字段转储（诊断用）
fn setting_dump(rc: i32, s: &NvDrsSetting, tag: &str) -> String {
    let name_utf16: String = s
        .setting_name
        .split(|&c| c == 0)
        .next()
        .unwrap_or(&[])
        .iter()
        .filter_map(|&c| char::from_u32(c as u32))
        .collect();
    format!(
        "[{}] rc=0x{:08X}({}) id=0x{:08X} type={} location={} curPredef={} predefValid={} predefU32={} predefVal={:?} curU32={} curVal={:?} name={:?}",
        tag,
        rc,
        if rc == NVAPI_OK {
            "OK"
        } else if is_profile_not_found(rc) {
            "NOT_FOUND"
        } else {
            "ERR"
        },
        s.setting_id,
        s.setting_type,
        s.setting_location,
        s.is_current_predefined,
        s.is_predefined_valid,
        s.binary_predefined_value.u32_value,
        dword_current_value_predef(s),
        s.binary_current_value.u32_value,
        dword_current_value(s),
        name_utf16,
    )
}

/// 读取预定义值 union 前 4 字节（诊断用，与 dword_current_value 同语义）
fn dword_current_value_predef(s: &NvDrsSetting) -> Option<u32> {
    Some(s.binary_predefined_value.u32_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvapi_version_结构大小与官方一致() {
        // 上游探针实测 NVDRS_SETTING_V1 = 12320 字节（与 nvapi.h 布局一致）
        assert_eq!(size_of::<NvDrsSetting>(), 12320);
        let v = nvapi_version();
        assert_eq!(v, 12320 | (1 << 16));
        assert_eq!(v >> 16, 1);
    }

    #[test]
    fn 电源管理模式值域映射() {
        // 值域以 NVIDIA App featureEnum 为准：0=Adaptive 1=Max 5=Optimal
        assert_eq!(
            NvidiaPowerMode::from_raw(0),
            Some(NvidiaPowerMode::Adaptive)
        );
        assert_eq!(
            NvidiaPowerMode::from_raw(1),
            Some(NvidiaPowerMode::MaxPerformance)
        );
        assert_eq!(NvidiaPowerMode::from_raw(5), Some(NvidiaPowerMode::Optimal));
        // 旧版误写的 2 为无效值
        assert_eq!(NvidiaPowerMode::from_raw(2), None);
        assert_eq!(NvidiaPowerMode::from_raw(3), None);
        assert_eq!(NvidiaPowerMode::MaxPerformance.label_cn(), "最高性能优先");
        assert_eq!(NvidiaPowerMode::Optimal.label_cn(), "最佳功率");
        assert_eq!(NvidiaPowerMode::Adaptive.label_cn(), "自适应");
    }

    #[test]
    fn dword_value_roundtrip() {
        // u32 值位修复验证：DWORD 值读写均走 union 起始 4 字节（u32 视图）。
        // 旧实现把 value_length 恒写 4（= u32 值位常量 4）→ 驱动视为无效值回退默认。
        let mut s: NvDrsSetting = unsafe { std::mem::zeroed() };
        // 写入 MaxPerformance=1 → u32 值位为 1
        set_dword_current_value(&mut s, 1);
        assert_eq!(
            s.binary_current_value.u32_value, 1,
            "u32 值位应直接承载 DWORD 值"
        );
        assert_eq!(dword_current_value(&s), Some(1), "值应从 u32 值位读取");
        // 写入 Optimal=5
        set_dword_current_value(&mut s, 5);
        assert_eq!(s.binary_current_value.u32_value, 5);
        assert_eq!(dword_current_value(&s), Some(5));
        // 写入 0
        set_dword_current_value(&mut s, 0);
        assert_eq!(dword_current_value(&s), Some(0));
        // 值为 4（无效值）时读取仍返回 4，由 get_power_mode 降级为自适应
        set_dword_current_value(&mut s, 4);
        assert_eq!(dword_current_value(&s), Some(4));
    }

    #[test]
    fn profile_not_found_judgement() {
        // 0xFFFFFF60 = -160（NVAPI_APPLICATION_PROFILE_NOT_FOUND）
        assert!(is_profile_not_found(-160));
        assert!(is_profile_not_found(0xFFFFFF60u32 as i32));
        assert!(!is_profile_not_found(0));
        assert!(!is_profile_not_found(-1));
        assert!(!is_profile_not_found(0xFFFFFF08u32 as i32)); // INCOMPATIBLE_STRUCT_VERSION
    }
}
