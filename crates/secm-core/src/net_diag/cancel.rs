// net_diag::cancel — 通用取消注册表（移植自原 src-tauri/src/cancel.rs，语义一致）
//
// 工作原理：
// - 每个命令通过 `cancel_flag(id)` 获取一个 `Arc<AtomicBool>`
// - 命令在执行循环中通过 `is_cancelled(id)` 检查是否应终止
// - UI 停止按钮调用 `cancel_command(id)` 置位取消标志
// - 命令完成后调用 `clear_cancel(id)` 清理（防孤儿注册项）

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

static CANCEL_MAP: Mutex<Option<HashMap<String, Arc<AtomicBool>>>> = Mutex::new(None);

fn with_map<R>(f: impl FnOnce(&mut HashMap<String, Arc<AtomicBool>>) -> R) -> R {
    let mut guard = CANCEL_MAP.lock();
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

/// 获取或创建指定命令 ID 的取消标志
pub fn cancel_flag(cmd_id: &str) -> Arc<AtomicBool> {
    with_map(|map| {
        map.entry(cmd_id.to_string())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone()
    })
}

/// 标记命令为已取消（未注册的 cmdId 为无操作，安全）
pub fn cancel_command(cmd_id: &str) {
    with_map(|map| {
        if let Some(flag) = map.get(cmd_id) {
            flag.store(true, Ordering::SeqCst);
        }
    });
}

/// 命令完成后清理取消标志
pub fn clear_cancel(cmd_id: &str) {
    with_map(|map| {
        map.remove(cmd_id);
    });
}

/// 检查命令是否已被取消（未注册视为未取消）
pub fn is_cancelled(cmd_id: &str) -> bool {
    with_map(|map| {
        map.get(cmd_id)
            .map(|f| f.load(Ordering::SeqCst))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_cancel_lifecycle() {
        let id = "test-cancel-lifecycle";
        assert!(!is_cancelled(id), "未注册的 cmdId 应视为未取消");
        cancel_command(id);
        assert!(!is_cancelled(id), "取消未注册的 cmdId 应为无操作");
        let _flag = cancel_flag(id);
        assert!(!is_cancelled(id), "注册后默认未取消");
        cancel_command(id);
        assert!(is_cancelled(id), "cancel_command 后应命中取消");
        clear_cancel(id);
        assert!(!is_cancelled(id), "clear 后应恢复未取消（注册项移除）");
    }

    #[test]
    fn test_flag_shared_across_calls() {
        let id = "test-cancel-shared";
        let flag = cancel_flag(id);
        cancel_command(id);
        assert!(
            flag.load(Ordering::SeqCst),
            "cancel_flag 返回的句柄与注册表内为同一 Arc"
        );
        clear_cancel(id);
    }

    #[test]
    fn test_clear_unknown_is_noop() {
        clear_cancel("test-never-registered");
    }

    #[test]
    fn test_many_ids_isolated() {
        let a = "test-iso-a";
        let b = "test-iso-b";
        let _fa = cancel_flag(a);
        let _fb = cancel_flag(b);
        cancel_command(a);
        assert!(is_cancelled(a));
        assert!(!is_cancelled(b), "取消 a 不应影响 b");
        clear_cancel(a);
        clear_cancel(b);
        let _ = Duration::from_millis(0); // 保持 import 稳定
    }
}
