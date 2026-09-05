// secm-core 统一错误模型（对齐源 CollectError 语义：中文消息 + API/操作/详情分级）

use serde::Serialize;

/// 核心层错误（采集/系统操作统一错误类型）
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// Win32 API 调用失败（含 WMI/COM 封装失败）
    #[error("[{api}] {op} 失败: {detail}")]
    WinApi { api: &'static str, op: String, detail: String },

    /// 注册表读取失败
    #[error("注册表读取失败: {path} | {detail}")]
    Registry { path: String, detail: String },

    /// HTTP 请求失败
    #[error("HTTP 请求失败: {url} | {detail}")]
    Http { url: String, detail: String },

    /// 数据解析失败
    #[error("数据解析失败: {what} | {detail}")]
    Parse { what: String, detail: String },

    /// 需要管理员权限
    #[error("需要管理员权限: {op}")]
    NeedsAdmin { op: String },

    /// 数据不存在
    #[error("数据不存在: {what}")]
    NotFound { what: String },

    /// 外部命令执行失败（保留原样给用户）
    #[error("{0}")]
    Command(String),

    /// 其他内部错误
    #[error("内部错误: {0}")]
    Internal(String),
}

impl CoreError {
    pub fn winapi(api: &'static str, op: impl Into<String>) -> Self {
        Self::WinApi {
            api,
            op: op.into(),
            detail: format!("错误码 {}", last_error_code()),
        }
    }

    pub fn winapi_detailed(
        api: &'static str,
        op: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self::WinApi { api, op: op.into(), detail: detail.into() }
    }

    pub fn registry(path: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Registry { path: path.into(), detail: detail.into() }
    }

    pub fn http(url: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Http { url: url.into(), detail: detail.into() }
    }

    pub fn parse(what: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Parse { what: what.into(), detail: detail.into() }
    }

    pub fn needs_admin(op: impl Into<String>) -> Self {
        Self::NeedsAdmin { op: op.into() }
    }

    pub fn command(msg: impl Into<String>) -> Self {
        Self::Command(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// 用户可见中文消息（兼容旧 String 错误面）
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}

/// 读取 GetLastError（Windows）
#[cfg(windows)]
fn last_error_code() -> u32 {
    // SAFETY: GetLastError 无参标准导出，线程局部存储
    unsafe { windows_sys::Win32::Foundation::GetLastError() }
}

#[cfg(not(windows))]
fn last_error_code() -> u32 {
    0
}

/// 前台展示用的可序列化错误（对齐旧 HardwareError 形态）
#[derive(Debug, Clone, Serialize)]
pub struct UserError {
    pub code: String,
    pub message: String,
}
