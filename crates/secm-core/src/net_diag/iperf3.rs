// net_diag::iperf3 — iperf3 带宽测试（外部进程，stdout 逐行流式，取消即 kill）
//
// 行为对齐原版 iperf3_stream（ADR-0001 §3）：
// - 命令行：iperf3 -c <host> -p <port> -t <duration> -f m
// - CREATE_NO_WINDOW（0x08000000）防止黑窗常驻
// - stdout 逐行 kind="info" 事件流
// - 成功（退出码 0）→ Ok(stderr 去首尾)；失败 → Err(stderr 去首尾)
// 增强点（ADR-0001 §5.3）：取消即 kill 子进程（原版仅在收到新 stdout 行时才检查取消，
// 长时间无输出时取消延迟）；未安装给出明确中文反馈。
use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::cancel::is_cancelled;
use super::StreamEvent;

/// 子进程轮询间隔（取消响应粒度）
const POLL_MS: u64 = 100;

/// CREATE_NO_WINDOW（DOS 过程创建标志，防止控制台窗口弹出）
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 运行 iperf3（阻塞；在后台线程执行；stdout 读线程为 'static，emit 以 Arc 共享）
pub fn run_streaming(
    host: &str,
    port: u16,
    duration: u32,
    cmd_id: &str,
    emit: std::sync::Arc<dyn Fn(StreamEvent) + Send + Sync>,
) -> Result<String, String> {
    // creation_flags：Windows 专属安全扩展方法（CommandExt），防控制台窗口弹出
    let child = Command::new("iperf3")
        .args([
            "-c",
            host,
            "-p",
            &port.to_string(),
            "-t",
            &duration.to_string(),
            "-f",
            "m",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        // 未安装/无法启动：明确反馈（对齐原版文案 + 修复建议）
        Err(e) => {
            let msg = format!(
                "iperf3 未安装或无法启动: {}（建议：安装 iperf3 并加入 PATH 后重试）",
                e
            );
            emit(StreamEvent::text("error", msg.clone()));
            return Err(msg);
        }
    };

    // stdout 读线程：逐行 emit；取消 → kill 后随管道 EOF 退出
    let stdout = child.stdout.take().expect("stdout 已 piped");
    let cancel_id = cmd_id.to_string();
    let emit_line = emit.clone();
    let stdout_thread = std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if is_cancelled(&cancel_id) {
                break;
            }
            match line {
                Ok(l) => emit_line(StreamEvent::text("info", l)),
                Err(_) => break,
            }
        }
    });

    // stderr 收集线程：并发排空管道（防缓冲写满阻塞子进程），退出后作为结果返回
    // （对齐原版契约：成功 → Ok(stderr)；失败 → Err(stderr)，错误原因在 stderr 中）
    let stderr = child.stderr.take().expect("stderr 已 piped");
    let stderr_thread = std::thread::spawn(move || {
        let mut collected = String::new();
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    collected.push_str(&l);
                    collected.push('\n');
                }
                Err(_) => break,
            }
        }
        collected
    });

    // 主循环：轮询子进程退出 + 取消即 kill
    let started = Instant::now();
    let mut killed = false;
    loop {
        if is_cancelled(cmd_id) {
            emit(StreamEvent::text("info", "用户取消，正在终止 iperf3…"));
            let _ = child.kill();
            killed = true;
            break;
        }
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if started.elapsed() > Duration::from_secs(u64::from(duration) + 30) {
                    // 兜底看门狗：超出预期时长 30s 强制结束（防挂死）
                    let _ = child.kill();
                    break;
                }
                std::thread::sleep(Duration::from_millis(POLL_MS));
            }
            Err(e) => {
                let msg = format!("iperf3 进程等待失败: {e}");
                emit(StreamEvent::text("error", msg.clone()));
                return Err(msg);
            }
        }
    }

    // 等待读线程随管道 EOF 退出（kill 后立即 EOF，不会悬挂）
    let _ = stdout_thread.join();
    let stderr_text = stderr_thread.join().unwrap_or_default();

    let output = child.wait().map_err(|e| format!("{}", e))?;
    if killed || is_cancelled(cmd_id) {
        return Err("用户取消".to_string());
    }
    let stderr_trim = stderr_text.trim().to_string();
    if output.success() {
        Ok(stderr_trim)
    } else {
        Err(if stderr_trim.is_empty() {
            format!("iperf3 退出码 {}", output.code().unwrap_or(-1))
        } else {
            stderr_trim
        })
    }
}
