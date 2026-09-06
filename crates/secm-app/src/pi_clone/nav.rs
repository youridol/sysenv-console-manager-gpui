// pi_clone::nav — SECM 工具页导航元数据（替代原会话树）
//
// 产品调整（用户指令）：移除全部会话相关功能；克隆壳迁移旧 11 页导航，
// 侧栏为工具页导航列表，Main 区渲染对应工具页内容。
//
// 分组与顺序沿用原 AppRoot::Page 顺序（不自行重排），分组仅为侧栏视觉层级。

use super::icons::Icon;

/// SECM 工具页枚举（对齐原 app.rs Page，顺序不变；调试日志已迁右栏日志流面板，不再占导航）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecmPage {
    Dashboard,
    Cleanup,
    Network,
    NetConfig,
    Settings,
    Services,
    Environment,
    AiEnvironment,
    Hardware,
    About,
}

impl SecmPage {
    pub const ALL: &'static [SecmPage] = &[
        SecmPage::Dashboard,
        SecmPage::Cleanup,
        SecmPage::Network,
        SecmPage::NetConfig,
        SecmPage::Settings,
        SecmPage::Services,
        SecmPage::Environment,
        SecmPage::AiEnvironment,
        SecmPage::Hardware,
        SecmPage::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dashboard => "硬件信息",
            Self::Cleanup => "清理优化",
            Self::Network => "网络诊断",
            Self::NetConfig => "网络配置",
            Self::Settings => "系统设置",
            Self::Services => "服务管理",
            Self::Environment => "环境检测",
            Self::AiEnvironment => "AI 环境",
            Self::Hardware => "硬件检测",
            Self::About => "关于",
        }
    }

    /// 导航行图标
    pub fn icon(self) -> Icon {
        match self {
            Self::Dashboard => Icon::Gauge,
            Self::Cleanup => Icon::Trash,
            Self::Network => Icon::Wifi,
            Self::NetConfig => Icon::Sliders,
            Self::Settings => Icon::Settings,
            Self::Services => Icon::Server,
            Self::Environment => Icon::Box,
            Self::AiEnvironment => Icon::Cpu,
            Self::Hardware => Icon::HardDrive,
            Self::About => Icon::Info,
        }
    }
}

/// 侧栏分组标题
#[derive(Debug, Clone, Copy)]
pub enum NavGroup {
    Overview,
    Tools,
    System,
}

impl NavGroup {
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "概览",
            Self::Tools => "工具",
            Self::System => "系统",
        }
    }

    pub fn contains(self, page: SecmPage) -> bool {
        match self {
            Self::Overview => page == SecmPage::Dashboard,
            Self::Tools => matches!(
                page,
                SecmPage::Cleanup
                    | SecmPage::Network
                    | SecmPage::NetConfig
                    | SecmPage::Settings
                    | SecmPage::Services
                    | SecmPage::Environment
                    | SecmPage::AiEnvironment
                    | SecmPage::Hardware
            ),
            Self::System => matches!(page, SecmPage::About),
        }
    }
}

pub const NAV_GROUPS: &'static [NavGroup] = &[NavGroup::Overview, NavGroup::Tools, NavGroup::System];
