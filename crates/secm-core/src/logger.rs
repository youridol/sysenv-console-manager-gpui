// secm-core::logger — 用户日志环形缓冲 + log crate 桥接后端 + 按天落盘
//
// 历史缺陷（审计 P1-2）：全库 47+ 处 log::warn!/debug! 没有任何后端
// （env_logger 从未初始化），日志全部静默丢弃，调试日志页只有 1 条自写记录。
// 现提供 `init()`：注册 log::Log 桥接后端，把全库日志转发到全局 LogBuffer
// （Logs 页消费）并按天落盘 %LOCALAPPDATA%\SECM\logs\app-YYYYMMDD.log。

use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 单条日志
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub level: String,
    pub module: String,
    pub message: String,
    pub timestamp: String,
}

/// 环形日志缓冲（容量 200，对齐源 log.rs）
pub struct LogBuffer {
    inner: Mutex<LogInner>,
}

struct LogInner {
    entries: Vec<LogEntry>,
    capacity: usize,
    /// 新条目回调（UI 订阅实时推送）
    listeners: Vec<Arc<dyn Fn(&LogEntry) + Send + Sync>>,
}

impl LogBuffer {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(LogInner {
                entries: Vec::with_capacity(200),
                capacity: 200,
                listeners: Vec::new(),
            }),
        })
    }

    /// 进程级全局日志缓冲（Logs 页与各处日志共享）
    pub fn global() -> &'static Arc<Self> {
        static G: std::sync::OnceLock<Arc<LogBuffer>> = std::sync::OnceLock::new();
        G.get_or_init(Self::new)
    }

    pub fn append(&self, level: &str, module: &str, message: &str) {
        let ts = now_ts();
        let entry = LogEntry {
            level: level.to_string(),
            module: module.to_string(),
            message: message.to_string(),
            timestamp: ts,
        };
        let mut g = self.inner.lock();
        if g.entries.len() >= g.capacity {
            g.entries.remove(0);
        }
        g.entries.push(entry.clone());
        let listeners = g.listeners.clone();
        drop(g);
        for l in listeners {
            l(&entry);
        }
    }

    pub fn get_all(&self) -> Vec<LogEntry> {
        self.inner.lock().entries.clone()
    }

    pub fn clear(&self) {
        self.inner.lock().entries.clear();
    }

    pub fn subscribe(&self, cb: Arc<dyn Fn(&LogEntry) + Send + Sync>) {
        self.inner.lock().listeners.push(cb);
    }
}

/// 当前时间戳（HH:MM:SS.mmm，对齐前端 logger 格式）
fn now_ts() -> String {
    let now = chrono::Local::now();
    format!("{}", now.format("%Y-%m-%d %H:%M:%S%.3f"))
}

// ============================================================================
// log crate 桥接后端 + 按天落盘（P1-2）
// ============================================================================

/// log crate → LogBuffer/文件 的桥接后端
struct BridgeLogger;

static BRIDGE_INSTALLED: AtomicBool = AtomicBool::new(false);

/// 落盘文件句柄（按天滚动：日期变化即重开新文件）
static LOG_FILE: Mutex<Option<(String, std::fs::File)>> = Mutex::new(None);

impl log::Log for BridgeLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = match record.level() {
            log::Level::Error => "Error",
            log::Level::Warn => "Warn",
            log::Level::Info => "Info",
            log::Level::Debug => "Debug",
            log::Level::Trace => "Trace",
        };
        // 模块名：剥掉 secm_ crate 前缀（"secm_core::cleanup" → "core::cleanup"）
        let module = record.target().trim_start_matches("secm_").to_string();
        let message = format!("{}", record.args());
        write_log_file(level, &module, &message);
        LogBuffer::global().append(level, &module, &message);
    }

    fn flush(&self) {}
}

/// 按天落盘（失败静默：日志不得影响业务；LogBuffer 侧始终可用）
fn write_log_file(level: &str, module: &str, message: &str) {
    let today = chrono::Local::now().format("%Y%m%d").to_string();
    let mut guard = LOG_FILE.lock();
    // 日期变化或未打开 → 重开当天文件
    if guard.as_ref().map_or(true, |(d, _)| *d != today) {
        let dir = match std::env::var("LOCALAPPDATA") {
            Ok(base) if !base.is_empty() => std::path::PathBuf::from(base).join("SECM").join("logs"),
            _ => return,
        };
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join(format!("app-{}.log", today));
        match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            Ok(f) => *guard = Some((today, f)),
            Err(_) => {
                // 打开失败置空，避免每条日志重试 IO
                *guard = None;
                return;
            }
        }
    }
    if let Some((_, file)) = guard.as_mut() {
        use std::io::Write;
        let line = format!("{} [{:5}] [{}] {}\r\n", now_ts(), level, module, message);
        let _ = file.write_all(line.as_bytes());
    }
}

/// 启动时清理 7 天前的旧日志文件（按天滚动下文件数量有限，防无限累积）
fn cleanup_old_logs() {
    let Ok(base) = std::env::var("LOCALAPPDATA") else {
        return;
    };
    if base.is_empty() {
        return;
    }
    let dir = std::path::PathBuf::from(base).join("SECM").join("logs");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let cutoff = chrono::Local::now() - chrono::Duration::days(7);
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // 文件名形如 app-20260905.log
        let Some(date_part) = name
            .strip_prefix("app-")
            .and_then(|s| s.strip_suffix(".log"))
        else {
            continue;
        };
        if let Ok(d) = chrono::NaiveDate::parse_from_str(date_part, "%Y%m%d") {
            if d < cutoff.date_naive() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// 初始化日志后端（应用启动时调用一次；幂等）。
///
/// 把全库 `log::info!/warn!/debug!` 转发到全局 LogBuffer（Logs 页展示）
/// 并按天落盘；重复调用只生效一次。
pub fn init() {
    if BRIDGE_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    cleanup_old_logs();
    // set_boxed_logger 失败 = 已有后端（重复 init），忽略
    let _ = log::set_boxed_logger(Box::new(BridgeLogger));
    log::set_max_level(log::LevelFilter::Debug);
}
