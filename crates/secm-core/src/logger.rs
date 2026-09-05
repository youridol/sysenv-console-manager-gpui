// secm-core::logger — 用户日志环形缓冲 + 事件通知（Logs 页与全局诊断共用）

use parking_lot::Mutex;
use serde::Serialize;
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
