//! CPU 温度 ring0 采集 — PawnIO 模块直连（用户态 IOCTL，零 HTTP）
//!
//! v3.1.0 BUG 修复：v3.0.0 删除 sidecar 后 CPU 温度恒 unavailable——本 BUG 的
//! 根因是"本机已部署 PawnIO + 应用具备管理员令牌"时丢失了既有的 ring0 采集
//! 能力。本模块经 PawnIO 设备（\\.\PawnIO）加载签名模块直读 CPU 温度寄存器，
//! 恢复与 v2.x sidecar 等价的温度能力，且仍为进程内直调（无 HTTP/无子进程）。
//!
//! 数据源与权限语义（严格区分"非管理员可用"与"能读全部传感器"）：
//! - PawnIO 设备 DACL 仅 SYSTEM/Administrators 可访问 → 非管理员环境
//!   CreateFileW 被拒 → 恒不可用（如实 unavailable，不伪造、不提权）；
//! - 管理员 + 已部署 PawnIO（2.x）→ 读真实 CPU 温度；
//! - 未部署 PawnIO → 不可用，诊断文案给出部署指引。
//!
//! 厂商分流（HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0）：
//! - AuthenticAMD（Zen family 0x17/0x19/0x1A）：AMDFamily17 模块 `ioctl_read_smn`
//!   读 SMN THM_TCON_CUR_TMP(0x59800)，`temp = (raw >> 21) * 125 * 0.001`，
//!   RANGE_SEL/TJ_SEL 标志置位时 −49（Linux k10temp/LHM 等价公式）；
//! - GenuineIntel：IntelMSR 模块 `ioctl_read_msr` 读 IA32_TEMPERATURE_TARGET
//!   (0x1A2) 取 TjMax（bits 23:16），IA32_PACKAGE_THERM_STATUS (0x1B1) 取
//!   Package 距 TjMax 偏移（bits 22:16，readout 有效位 bit31），temp = TjMax − Δ
//!   （LHM IntelCpu 等价公式）；
//! - 其他厂商/代际 → 模块不加载，如实不可用。
//!
//! 模块 bin（AMDFamily17.bin / IntelMSR.bin）提取自 LibreHardwareMonitorLib
//! 嵌入资源（MPL-2.0），以独立文件存放于 third_party/PawnIO/modules/ 并随包
//! 分发（MPL-2.0 文件级隔离合规），经 include_bytes! 编入二进制——运行时无
//! 外部文件依赖。PawnIO 模块由驱动按作者密钥验签后执行，本模块不加载任何
//! 未签名代码。
//!
//! 生命周期：模块句柄进程级单例（PawnIO 约定每句柄仅可加载一个模块），
//! 惰性初始化一次；失败（设备缺失/权限不足/厂商不符）永久缓存错误原因，
//! 避免每秒采集重复发起 IOCTL。线程模型：同步微秒级 IOCTL，采集线程调用（S8）。

use std::sync::Mutex;

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

/// PawnIO 设备路径
const DEVICE_PATH: &str = "\\\\.\\PawnIO";
/// PawnIO 驱动设备类型（pawnio_um.h k_device_type）
const K_DEVICE_TYPE: u32 = 41394;
/// CTL_CODE(k_device_type, 0x821, METHOD_BUFFERED, FILE_ANY_ACCESS)
const IOCTL_PIO_LOAD_BINARY: u32 = (K_DEVICE_TYPE << 16) | (0x821 << 2);
/// CTL_CODE(k_device_type, 0x841, METHOD_BUFFERED, FILE_ANY_ACCESS)
const IOCTL_PIO_EXECUTE_FN: u32 = (K_DEVICE_TYPE << 16) | (0x841 << 2);
/// pawnio_execute 输入布局：32 字节 NUL 结尾函数名 + u64 参数数组
const FN_NAME_LENGTH: usize = 32;

/// AMD Zen SMN 寄存器：THM_TCON_CUR_TMP（LHM F17H_M01H_THM_TCON_CUR_TMP 等价）
const SMN_THM_TCON_CUR_TMP: u64 = 0x0005_9800;
/// SMN 温度 RANGE_SEL 位（bits 19）
const SMN_TEMP_RANGE_SEL_MASK: u64 = 0x8_0000;
/// SMN 温度 TJ_SEL 位域（bits 17:16，== 0b11 时含 −49 修正）
const SMN_TEMP_TJ_SEL_MASK: u64 = 0x3_0000;
/// Intel MSR：IA32_TEMPERATURE_TARGET（TjMax，bits 23:16）
const MSR_IA32_TEMPERATURE_TARGET: u64 = 0x01A2;
/// Intel MSR：IA32_PACKAGE_THERM_STATUS（Package digital readout，bits 22:16，有效位 bit31）
const MSR_IA32_PACKAGE_THERM_STATUS: u64 = 0x01B1;
/// 温度合理区间（℃）；越界视为读取异常（不伪造）
const TEMP_VALID_RANGE: std::ops::RangeInclusive<f32> = -10.0..=120.0;

// 模块 bin（提取自 LibreHardwareMonitorLib 嵌入资源，MPL-2.0；见模块头注释）
/// AMD Zen（family 0x17/0x19/0x1A）SMN/MSR 读取模块
const AMD_FAMILY17_BIN: &[u8] =
    include_bytes!("../../../third_party/PawnIO/modules/AMDFamily17.bin");
/// Intel MSR 读取模块
const INTEL_MSR_BIN: &[u8] = include_bytes!("../../../third_party/PawnIO/modules/IntelMSR.bin");

/// CPU 厂商（决定加载的 PawnIO 模块与解算公式）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CpuVendor {
    Amd,
    Intel,
}

/// 已加载的 PawnIO 模块句柄（进程生命周期持有）
struct PawnIoModule {
    handle: HANDLE,
}

// SAFETY: HANDLE 指向内核设备对象，DeviceIoControl 可跨线程并发调用；
// 句柄不绑定线程（无 APC 语义），Send/Sync 安全。
unsafe impl Send for PawnIoModule {}
unsafe impl Sync for PawnIoModule {}

impl Drop for PawnIoModule {
    fn drop(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE {
            // SAFETY: 有效句柄关闭
            unsafe { CloseHandle(self.handle) };
        }
    }
}

impl PawnIoModule {
    /// 打开 PawnIO 设备并加载模块 blob（每句柄仅一个模块，驱动验签）
    fn load(blob: &[u8]) -> Result<Self, String> {
        let path: Vec<u16> = DEVICE_PATH
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: 宽字符串 NUL 结尾；GENERIC_READ|WRITE 为 PawnIO 要求的访问级别
        //（非管理员被设备 DACL 拒绝 → 由调用方如实降级）
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            let err = std::io::Error::last_os_error();
            let code = err.raw_os_error().unwrap_or(0);
            // 权限拒绝（非管理员）与未部署分别给明确诊断
            if code == 5 {
                return Err(
                    "CPU 温度需 PawnIO 设备访问权限（仅 SYSTEM/Administrators），非管理员环境不可用"
                        .to_string(),
                );
            }
            return Err(format!(
                "CPU 温度需部署 PawnIO 2.x（设备 {DEVICE_PATH} 打开失败：{err}）；未部署时该指标不可用"
            ));
        }

        let module = PawnIoModule { handle };
        // SAFETY: 句柄有效；blob 只读缓冲
        unsafe {
            let mut returned: u32 = 0;
            let ok = DeviceIoControl(
                module.handle,
                IOCTL_PIO_LOAD_BINARY,
                blob.as_ptr() as *const core::ffi::c_void,
                blob.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            );
            if ok == 0 {
                let err = std::io::Error::last_os_error();
                CloseHandle(module.handle);
                return Err(format!(
                    "PawnIO 模块加载失败（驱动验签/模块厂商自检拒绝）：{err}"
                ));
            }
        }
        Ok(module)
    }

    /// 执行模块函数：输入 = 32 字节 NUL 结尾函数名 + u64 参数数组；输出 = u64 数组
    fn execute(&self, name: &str, input: &[u64], out_count: usize) -> Result<Vec<u64>, String> {
        let name_bytes = name.as_bytes();
        if name_bytes.len() >= FN_NAME_LENGTH {
            return Err("PawnIO 函数名过长".to_string());
        }
        let mut buf = vec![0u8; FN_NAME_LENGTH];
        buf[..name_bytes.len()].copy_from_slice(name_bytes);
        for v in input {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        let mut out = vec![0u64; out_count];
        let mut returned: u32 = 0;
        // SAFETY: 句柄有效；输入/输出缓冲布局符合 PawnIO execute 约定
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_PIO_EXECUTE_FN,
                buf.as_ptr() as *const core::ffi::c_void,
                buf.len() as u32,
                out.as_mut_ptr() as *mut core::ffi::c_void,
                (out.len() * std::mem::size_of::<u64>()) as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(format!(
                "PawnIO execute({name}) 失败：{}",
                std::io::Error::last_os_error()
            ));
        }
        out.truncate(returned as usize / std::mem::size_of::<u64>());
        Ok(out)
    }

    /// ioctl_read_smn：读 AMD SMN 寄存器（32 位值）
    fn read_smn(&self, address: u64) -> Result<u32, String> {
        let out = self.execute("ioctl_read_smn", &[address], 1)?;
        out.into_iter()
            .next()
            .map(|v| v as u32)
            .ok_or_else(|| "PawnIO ioctl_read_smn 空输出".to_string())
    }

    /// ioctl_read_msr：读 64 位 MSR
    fn read_msr(&self, index: u64) -> Result<u64, String> {
        let out = self.execute("ioctl_read_msr", &[index], 1)?;
        out.into_iter()
            .next()
            .ok_or_else(|| "PawnIO ioctl_read_msr 空输出".to_string())
    }
}

/// 进程级模块单例：None = 尚未初始化；Err = 永久失败原因（避免每秒重试 IOCTL/打开）
static MODULE: Mutex<Option<Result<PawnIoModule, String>>> = Mutex::new(None);

/// 从注册表读取 CPU 厂商与 family（HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0；
/// 静态信息，首次读取后缓存）
fn detect_cpu() -> Result<(CpuVendor, u8), String> {
    static CACHE: std::sync::OnceLock<Result<(CpuVendor, u8), String>> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            use winreg::enums::HKEY_LOCAL_MACHINE;
            use winreg::RegKey;
            let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
            let key = hklm
                .open_subkey(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0")
                .map_err(|e| format!("CPU 厂商探测失败（注册表）: {e}"))?;
            let vendor: String = key
                .get_value("VendorIdentifier")
                .map_err(|e| format!("CPU 厂商探测失败（VendorIdentifier）: {e}"))?;
            let identifier: String = key
                .get_value("Identifier")
                .map_err(|e| format!("CPU 厂商探测失败（Identifier）: {e}"))?;
            // Identifier 形如 "AMD64 Family 25 Model 97 Stepping 2" / "Intel64 Family 6 Model 190"
            let family = identifier
                .split_whitespace()
                .skip_while(|w| !w.eq_ignore_ascii_case("family"))
                .nth(1)
                .and_then(|w| w.parse::<u8>().ok())
                .unwrap_or(0);
            let v = if vendor.contains("AuthenticAMD") {
                CpuVendor::Amd
            } else if vendor.contains("GenuineIntel") {
                CpuVendor::Intel
            } else {
                return Err(format!("未知 CPU 厂商：{vendor}"));
            };
            Ok((v, family))
        })
        .clone()
}

/// 初始化模块（失败返回原因；由 with_module 永久缓存）
fn init_module() -> Result<PawnIoModule, String> {
    let (vendor, family) = detect_cpu()?;
    let blob = match (vendor, family) {
        (CpuVendor::Amd, f) if f == 0x17 || f == 0x19 || f == 0x1A => AMD_FAMILY17_BIN,
        (CpuVendor::Intel, _) => INTEL_MSR_BIN,
        (CpuVendor::Amd, f) => {
            return Err(format!(
                "CPU 温度暂不支持该 AMD 代际（family 0x{f:02X}，支持 Zen family 0x17/0x19/0x1A）"
            ));
        }
    };
    let module_name = match vendor {
        CpuVendor::Amd => "AMDFamily17",
        CpuVendor::Intel => "IntelMSR",
    };
    log::info!("CPU 温度 · 加载 PawnIO 模块 {module_name}（ring0 寄存器直读）");
    PawnIoModule::load(blob)
}

/// 在模块句柄上执行采集闭包（锁内执行；模块失败永久缓存原因，避免每秒重试 IOCTL）
fn with_module<T>(f: impl FnOnce(&PawnIoModule) -> Result<T, String>) -> Result<T, String> {
    // 毒锁恢复：采集闭包 panic 不应永久堵死温度通道（into_inner 取回状态）
    let mut guard = MODULE.lock().unwrap_or_else(|p| p.into_inner());
    if guard.is_none() {
        let result = init_module();
        if let Err(e) = &result {
            log::info!("CPU 温度 · ring0 通道不可用（永久降级）：{e}");
        }
        *guard = Some(result);
    }
    match guard.as_ref() {
        Some(Ok(m)) => f(m),
        Some(Err(e)) => Err(e.clone()),
        None => Err("PawnIO 模块状态异常".to_string()),
    }
}

/// Intel TjMax 缓存（静态值，首读后缓存）
static INTEL_TJ_MAX: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();

/// 读取 CPU 温度（℃；ring0 寄存器直读，每次调用为一次微秒级 IOCTL）
///
/// 失败返回可读诊断（供 Metric::unavailable 透出），不伪造数值。
pub fn read_cpu_temperature() -> Result<f32, String> {
    let (vendor, _) = detect_cpu()?;
    let temp = with_module(|module| match vendor {
        CpuVendor::Amd => {
            // THM_TCON_CUR_TMP：CUR_TEMP bits 31:21，0.125℃/lsb；
            // RANGE_SEL/TJ_SEL 标志 → −49 修正（k10temp/LHM 等价）
            let raw = module.read_smn(SMN_THM_TCON_CUR_TMP)?;
            let offset_flag = (raw as u64 & SMN_TEMP_RANGE_SEL_MASK) != 0
                || (raw as u64 & SMN_TEMP_TJ_SEL_MASK) == SMN_TEMP_TJ_SEL_MASK;
            Ok::<f32, String>(
                ((raw >> 21) * 125) as f32 * 0.001 - if offset_flag { 49.0 } else { 0.0 },
            )
        }
        CpuVendor::Intel => {
            // TjMax 缓存（IA32_TEMPERATURE_TARGET bits 23:16）
            let tj_max = *INTEL_TJ_MAX.get_or_init(|| {
                module
                    .read_msr(MSR_IA32_TEMPERATURE_TARGET)
                    .ok()
                    .map(|v| ((v >> 16) & 0xFF) as f32)
                    .filter(|t| (40.0..=130.0).contains(t))
            });
            let Some(tj_max) = tj_max else {
                return Err("CPU 温度读取失败（TjMax 无效）".to_string());
            };
            // Package digital readout：有效位 bit31，Δ = bits 22:16
            let pkg = module.read_msr(MSR_IA32_PACKAGE_THERM_STATUS)?;
            if (pkg >> 31) & 1 == 0 {
                return Err("CPU 温度读取失败（Package readout 无效位）".to_string());
            }
            let delta = ((pkg & 0x007F_0000) >> 16) as f32;
            Ok(tj_max - delta)
        }
    })?;
    if TEMP_VALID_RANGE.contains(&temp) {
        Ok(temp)
    } else {
        Err(format!("CPU 温度读数越界（{temp:.1}℃，疑似异常采样）"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amd_temp_decode() {
        // 探针实测样本：0x779B0000，offsetFlag=true → ≈70.5℃（2026-09-08 真机）
        let raw: u32 = 0x779B_0000;
        let offset_flag = (raw as u64 & SMN_TEMP_RANGE_SEL_MASK) != 0
            || (raw as u64 & SMN_TEMP_TJ_SEL_MASK) == SMN_TEMP_TJ_SEL_MASK;
        let t = ((raw >> 21) * 125) as f32 * 0.001 - if offset_flag { 49.0 } else { 0.0 };
        assert!(offset_flag);
        assert!((t - 70.5).abs() < 0.2, "实际 {t}");
        assert!(TEMP_VALID_RANGE.contains(&t));
    }

    #[test]
    fn test_amd_temp_decode_no_offset() {
        // 无修正位样本：0x70000000 → (raw>>21)=896 → 112℃（边界内）；offsetFlag=false
        let raw: u32 = 0x7000_0000;
        let offset_flag = (raw as u64 & SMN_TEMP_RANGE_SEL_MASK) != 0
            || (raw as u64 & SMN_TEMP_TJ_SEL_MASK) == SMN_TEMP_TJ_SEL_MASK;
        assert!(!offset_flag);
        let t = ((raw >> 21) * 125) as f32 * 0.001;
        assert!((t - 112.0).abs() < 0.2, "实际 {t}");
    }

    #[test]
    fn test_ioctl_codes() {
        // CTL_CODE(41394 = 0xA1B2, 0x821/0x841, METHOD_BUFFERED, FILE_ANY_ACCESS)
        assert_eq!(IOCTL_PIO_LOAD_BINARY, 0xA1B2_2084);
        assert_eq!(IOCTL_PIO_EXECUTE_FN, 0xA1B2_2104);
    }
}
