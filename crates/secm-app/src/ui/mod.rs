// secm-app::ui — 主题化基础控件
//
// page — 统一页面布局框架：全部左侧边栏页面主内容区的装配入口
//        （页头/卡片/数据表/状态横幅/按钮/键值行；颜色取自 pi_clone::theme::Palette，
//         明暗双主题随壳联动）
// text_input — 单行文本输入控件（移植 GPUI 官方 input example）
// toast — 全局泡泡提示系统（右上角；成功/警告/错误/提醒，自动消失+手动关闭）

pub mod page;
pub mod text_input;
pub mod toast;
