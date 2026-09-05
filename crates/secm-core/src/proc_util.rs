// secm-core::proc_util — 进程调用共享工具（PowerShell 执行 + 带超时进程执行；GUI 应用全程无窗口）
//
// 自旧 Tauri 端 src-tauri/src/proc_util.rs 迁入（run_ps_result + 解码辅助），
// 供需要调用系统命令的模块复用。net_config 的 DoH 配置走 PowerShell cmdlet，
// 即经本模块执行；后续迁入模块（如原 ip_info / net_stats 链路）亦可复用。

/// 进程执行结果（run_command_with_timeout 输出）
pub(crate) struct ProcOutput {
    /// 退出码是否成功（超时被杀恒为 false）
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// true = 超过 timeout 被强制杀树
    pub timed_out: bool,
}

/// 收集管道输出的后台线程（防管道缓冲写满导致子进程写阻塞死锁）
fn spawn_pipe_reader<R: std::io::Read + Send + 'static>(
    mut pipe: R,
) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new().name("proc-reader".into()).spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    rx
}

/// 构造 ExitStatus（跨平台小包装，本应用实际仅 Windows 编译运行）
pub(crate) fn exit_status(code: i32) -> std::process::ExitStatus {
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code as u32)
    }
    #[cfg(not(windows))]
    {
        std::process::ExitStatus::from_raw(code)
    }
}

/// 带超时的进程执行（P1-10：消除"子进程挂死 → 后台任务永久悬挂"）
///
/// - 超时后 `taskkill /PID <pid> /T /F` 杀整棵进程树（npm.cmd → node.exe 孙进程
///   一并终止；只 kill 直接子进程会留下持管道的孙进程，令读线程悬挂）
/// - stdout/stderr 由独立线程收集；进程树被杀/退出后管道关闭，收集自然结束，
///   调用侧最多再等 5s 收尾，不悬挂
/// - CREATE_NO_WINDOW（0x08000000）防止 GUI 应用弹出控制台黑窗
/// - 返回 None = 启动失败（CreateProcess 失败）
pub(crate) fn run_command_with_timeout(
    program: &str,
    args: &[&str],
    timeout: std::time::Duration,
) -> Option<ProcOutput> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()?;
    let stdout_pipe = child.stdout.take()?;
    let stderr_pipe = child.stderr.take()?;
    let out_rx = spawn_pipe_reader(stdout_pipe);
    let err_rx = spawn_pipe_reader(stderr_pipe);

    let deadline = std::time::Instant::now() + timeout;
    let mut timed_out = false;
    let mut success = false;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                success = st.success();
                break;
            }
            Ok(None) if std::time::Instant::now() >= deadline => {
                timed_out = true;
                // 杀整棵进程树（含 cmd→npm→node 孙进程），再回收子进程句柄
                let pid = child.id();
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .creation_flags(CREATE_NO_WINDOW)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                let _ = child.wait();
                break;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(25)),
            // try_wait 出错（极罕见）：按失败终止，避免死循环
            Err(_) => break,
        }
    }

    // 进程退出/被杀后管道关闭，read_to_end 自然返回；5s 兜底防极端悬挂
    let stdout = out_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or_default();
    let stderr = err_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or_default();

    Some(ProcOutput {
        success: success && !timed_out,
        stdout,
        stderr,
        timed_out,
    })
}

/// 执行 PowerShell 脚本，返回 stdout 文本；失败返回含 stderr 的中文可读错误
///
/// - `0x08000000` = CREATE_NO_WINDOW，防止 GUI 应用弹出控制台黑窗
/// - 强制 `[Console]::OutputEncoding = UTF-8`（无 BOM）：
///   中文 Windows 控制台默认 GBK，直接重定向会让中文输出
///   （如网卡别名"以太网"）在 `from_utf8_lossy` 下乱码
/// - 错误不再静默吞掉：启动失败、非零退出码、cmdlet 报错
///   （stderr/非空输出）均以 `Err(String)` 返回，消息含错误码与修复建议
///   （对齐源 proc_util run_ps_result 语义）
/// - 30s 超时（P1-10）：PS 冷启动 0.5–2s + 脚本执行，超时杀树不再悬挂
/// - 退出码 0 但 stderr 非空视为失败（P1-14）：cmdlet 非终止错误
///   （如 Set-DnsClientDohServerAddress "找不到实例"）以 0 退出码 + stderr 出现，
///   历史实现返回 Ok("") 静默吞错
pub(crate) fn run_ps_result(script: &str) -> Result<String, String> {
    let wrapped = format!("[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); {script}");
    let output = run_command_with_timeout(
        "powershell",
        &["-NoProfile", "-Command", &wrapped],
        std::time::Duration::from_secs(30),
    )
    .ok_or_else(|| {
        "PowerShell 启动失败（API: CreateProcess / powershell.exe）（建议：检查系统 PowerShell 是否可用或被组策略禁用）".to_string()
    })?;
    if output.timed_out {
        return Err(
            "PowerShell 执行超时（30s，API: powershell -NoProfile -Command）（建议：重试或检查系统负载/代理配置）"
                .to_string(),
        );
    }
    // cmdlet 错误流（stderr）可能不受 [Console]::OutputEncoding 控制而仍为 GBK
    // （中文系统），故统一走 UTF-8→GBK 兜底解码，避免错误消息乱码
    let stderr = decode_ps(&output.stderr).trim().to_string();
    let stdout = decode_ps(&output.stdout).trim().to_string();
    if output.success {
        if !stderr.is_empty() {
            return Err(format!(
                "PowerShell 执行失败（退出码 0 但有错误输出，API: powershell -NoProfile -Command）: {stderr}（建议：确认命令与参数合法、具备管理员权限后重试）"
            ));
        }
        return Ok(stdout);
    }
    let code = if output.success { 0 } else { 1 };
    // cmdlet 错误优先取 stderr；部分场景错误在 stdout（如命令语法回显）
    let msg = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "(无输出)".to_string()
    };
    Err(format!(
        "PowerShell 执行失败（退出码 {}，API: powershell -NoProfile -Command）: {}（建议：确认命令与参数合法、具备管理员权限后重试）",
        code, msg
    ))
}

/// 解码 PowerShell/系统命令输出：优先 UTF-8（stdout 已被强制 UTF-8），回退 GBK
/// （中文系统 stderr）。net_config 的 decode_ansi 已收敛到本函数。
pub(crate) fn decode_ps(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let (cow, _, _) = encoding_rs::GBK.decode(bytes);
    cow.into_owned()
}
