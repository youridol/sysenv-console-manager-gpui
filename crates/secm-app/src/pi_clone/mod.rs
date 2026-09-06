// pi_clone — SECM 三栏桌面壳（原 pi-agent-desktop 克隆壳改造）
//
// 产品调整（用户指令，2026-09-06）：
//   - 旧导航（AppRoot/Page/11 页外层）已迁移到本壳并被删除；
//   - 严格三栏 Flex Layout：Sidebar(工具页导航) | Main(工具页内容) | RightPanel(日志流面板)；
//   - 链路移除所有会话相关功能；右栏文件工作台已整改为日志流输出面板
//     （调试日志从左栏页面迁出，全链路行为/返回信息流式展示）。
//
// 视觉沿用原复刻基准（pi-agent-desktop 原生壳规格，docs/ui-clone/reference-spec.md）。

pub mod icons;
pub mod layout;
pub mod nav;
pub mod panel;
pub mod right_panel;
pub mod shell;
pub mod sidebar;
pub mod theme;

pub use shell::PiShell;
