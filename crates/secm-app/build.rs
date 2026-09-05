// secm-app 构建脚本 — Windows 资源嵌入
//
// 把 crates/icons/icon.ico 嵌入 secm-app.exe 资源段（资源 ID 1），
// 作为 Explorer 文件图标 / 任务栏 / Alt-Tab 图标来源；
// 运行时窗口图标经 icons::apply_window_icon 从同一资源加载（LoadImageW）。
//
// 全链路图标统一（v2.1.0）：exe / 窗口 / 托盘 均出自 crates/icons 资源。

fn main() {
    // 资源文件变更时触发重编（路径相对 crate 根）
    println!("cargo:rerun-if-changed=../icons/icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    winresource::WindowsResource::new()
        .set_icon("../icons/icon.ico")
        .compile()
        .expect("嵌入 Windows 资源（icons/icon.ico）失败");
}
