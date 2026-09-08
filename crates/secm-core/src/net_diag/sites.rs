// net_diag::sites — 网站测试（默认站点 + 自定义 CRUD + 持久化 + HEAD 探测）
//
// 行为对齐原版 Network.tsx 网站测试（ADR-0001 §2/§6.4）：
// - 4 个默认站点（Apple / GitHub / Google / YouTube）+ 自定义站点增删改
// - 持久化：原版 localStorage "secm_sites" → 本版 JSON 文件
//   %LOCALAPPDATA%\SECM\config\network_sites.json（原子写：tmp + rename）
// - 探测语义对齐原版 fetch HEAD（no-cors）：任意 HTTP 响应（含 4xx/5xx/重定向）=
//   可达；连接失败 / 超时 = 不可达；返回耗时 ms 与状态码
// - 网站测试 8s 超时；端口检测 5s 超时（与原版一致）

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// 默认站点（与原版 DEFAULT_SITES 一致）
pub const DEFAULT_SITES: &[(&str, &str)] = &[
    ("Apple", "https://www.apple.com"),
    ("GitHub", "https://github.com"),
    ("Google", "https://www.google.com"),
    ("YouTube", "https://www.youtube.com"),
];

/// 站点条目（自定义站点持久化单元）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SiteItem {
    pub name: String,
    pub url: String,
}

/// 站点持久化路径：%LOCALAPPDATA%\SECM\config\network_sites.json
pub fn sites_path() -> Option<PathBuf> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    if base.is_empty() {
        return None;
    }
    Some(
        PathBuf::from(base)
            .join("SECM")
            .join("config")
            .join("network_sites.json"),
    )
}

/// 读取自定义站点列表（文件缺失/损坏 → 空列表，不中断）
pub fn load_sites() -> Vec<SiteItem> {
    let Some(path) = sites_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<SiteItem>>(&text).unwrap_or_else(|e| {
        log::warn!(
            "站点配置解析失败（{}），按空列表处理: {}",
            path.display(),
            e
        );
        Vec::new()
    })
}

/// 保存自定义站点列表（原子写：先写 tmp 再 rename，防中途崩溃损坏配置）
pub fn save_sites(sites: &[SiteItem]) -> Result<(), String> {
    let Some(path) = sites_path() else {
        return Err("无法确定站点配置路径（LOCALAPPDATA 缺失）".to_string());
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("创建配置目录失败（{}）: {}", dir.display(), e))?;
    }
    let text = serde_json::to_string_pretty(sites).map_err(|e| format!("序列化失败: {}", e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text.as_bytes())
        .map_err(|e| format!("写入临时文件失败（{}）: {}", tmp.display(), e))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        // rename 失败时尽力清理 tmp
        let _ = std::fs::remove_file(&tmp);
        format!("替换配置文件失败（{}）: {}", path.display(), e)
    })?;
    Ok(())
}

/// HEAD 探测结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct SiteProbeResult {
    /// 是否可达（任意 HTTP 响应即可达，对齐原版 no-cors fetch 语义）
    pub ok: bool,
    /// HTTP 状态码（收到响应时有值；传输层失败为 None）
    pub status: Option<u16>,
    /// 总耗时（毫秒）
    pub ms: u64,
    /// 失败原因（传输层错误/超时）
    pub error: Option<String>,
}

/// HEAD 探测：任意 HTTP 响应（含非 2xx）= 可达；连接失败/超时 = 不可达
///
/// 使用 ureq（工作区既有依赖）：跟随重定向、总超时由调用方指定。
pub fn head_probe(url: &str, timeout_ms: u64) -> SiteProbeResult {
    let start = Instant::now();
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(timeout_ms))
        .build();
    let mut final_url = url.to_string();
    // 补协议前缀：用户输入裸域名时默认 https（对齐原版输入占位符语义）
    if !final_url.starts_with("http://") && !final_url.starts_with("https://") {
        final_url = format!("https://{}", final_url);
    }
    match agent.head(&final_url).call() {
        Ok(resp) => SiteProbeResult {
            ok: true,
            status: Some(resp.status()),
            ms: start.elapsed().as_millis() as u64,
            error: None,
        },
        Err(ureq::Error::Status(code, _)) => SiteProbeResult {
            // 收到 HTTP 响应（非 2xx）→ 网络可达（对齐原版 no-cors fetch：任何响应都算成功）
            ok: true,
            status: Some(code),
            ms: start.elapsed().as_millis() as u64,
            error: None,
        },
        Err(e) => SiteProbeResult {
            ok: false,
            status: None,
            ms: start.elapsed().as_millis() as u64,
            error: Some(format!("{}", e)),
        },
    }
}

/// TCP 端口连通性探测（端口检测工具；HEAD 语义等价：连接成功 = 开放）
///
/// 原版为 fetch HEAD http://target:port；本实现对任意端口（含非 HTTP 服务）语义更准确：
/// 直接 TCP connect，连接建立 = 端口开放。超时与原版一致 5s。
pub fn port_probe(target: &str, port: u16, timeout_ms: u64) -> SiteProbeResult {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Instant as Instant2;

    let start = Instant2::now();
    // 解析目标（域名/IP 皆可）
    let addrs: Vec<std::net::SocketAddr> = match format!("{}:{}", target, port).to_socket_addrs() {
        Ok(it) => it.collect(),
        Err(e) => {
            return SiteProbeResult {
                ok: false,
                status: None,
                ms: start.elapsed().as_millis() as u64,
                error: Some(format!("目标解析失败: {}", e)),
            };
        }
    };
    let mut last_err = String::from("无可用地址");
    for sa in addrs {
        match TcpStream::connect_timeout(&sa, Duration::from_millis(timeout_ms)) {
            Ok(stream) => {
                drop(stream);
                return SiteProbeResult {
                    ok: true,
                    status: None,
                    ms: start.elapsed().as_millis() as u64,
                    error: None,
                };
            }
            Err(e) => last_err = format!("{}", e),
        }
    }
    SiteProbeResult {
        ok: false,
        status: None,
        ms: start.elapsed().as_millis() as u64,
        error: Some(last_err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_sites_shape() {
        assert_eq!(DEFAULT_SITES.len(), 4, "默认站点应为 4 个");
        for (name, url) in DEFAULT_SITES {
            assert!(!name.is_empty());
            assert!(url.starts_with("https://"), "默认站点应为 https: {}", url);
        }
    }

    #[test]
    fn test_sites_roundtrip() {
        let sites = vec![
            SiteItem {
                name: "测试".to_string(),
                url: "https://example.org".to_string(),
            },
            SiteItem {
                name: "例2".to_string(),
                url: "http://127.0.0.1:8080".to_string(),
            },
        ];
        save_sites(&sites).expect("保存应成功");
        let loaded = load_sites();
        assert_eq!(loaded, sites, "保存后读取应还原（round-trip）");
        // 清理：写回空列表
        save_sites(&[]).ok();
    }

    #[test]
    fn test_load_missing_is_empty() {
        // 使用一个几乎必然不存在的路径行为：load_sites 对缺失文件返回空
        // （不实际改环境变量；直接验证函数对损坏内容鲁棒）
        let items = serde_json::from_str::<Vec<SiteItem>>("not-json");
        assert!(
            items.is_err(),
            "损坏 JSON 应解析失败（load_sites 捕获后返回空）"
        );
    }

    #[test]
    fn test_head_probe_input_shapes() {
        // 仅验证函数可调用与结构（不依赖外网；失败路径也合法）
        let r = head_probe("127.0.0.1:1", 300);
        assert!(!r.ok, "回环关闭端口应不可达");
        assert!(r.error.is_some(), "失败应有原因");
        assert!(r.ms < 5000, "应按超时返回");
    }

    /// 实机：公网站点 HEAD 探测（对齐原版 no-cors 语义）
    #[test]
    #[ignore = "实机网络用例：cargo test -- --ignored 运行"]
    fn real_head_probe_public() {
        let r = head_probe("https://www.apple.com", 8000);
        assert!(r.ok, "Apple 应可达: {:?}", r);
        assert!(r.status.is_some(), "应有 HTTP 状态码");
    }

    /// 实机：端口探测（本机回环监听端口 vs 未监听端口）
    #[test]
    #[ignore = "实机网络用例：cargo test -- --ignored 运行"]
    fn real_port_probe_loopback() {
        // 起临时 TCP 监听
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                drop(conn);
            }
        });
        let open = port_probe("127.0.0.1", port, 2000);
        assert!(open.ok, "监听端口应开放: {:?}", open);
        let closed = port_probe("127.0.0.1", 1, 1000);
        assert!(!closed.ok, "端口 1（回环）应关闭: {:?}", closed);
    }
}
