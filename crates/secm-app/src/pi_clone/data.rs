// pi_clone::data — 右侧文件工作台 mock 数据（文件树）
//
// 产品调整（用户指令）：已移除全部会话相关功能（项目/会话树、会话行、搜索会话、
// unread/running 会话状态均不再需要），本模块仅保留右面板文件树 mock。

/// 文件树条目
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: &'static str,
    pub is_dir: bool,
    pub children: Vec<FileEntry>,
}

pub fn file_tree() -> Vec<FileEntry> {
    vec![
        FileEntry {
            name: "crates",
            is_dir: true,
            children: vec![
                FileEntry {
                    name: "secm-app",
                    is_dir: true,
                    children: vec![
                        FileEntry {
                            name: "src",
                            is_dir: true,
                            children: vec![
                                FileEntry { name: "app.rs", is_dir: false, children: vec![] },
                                FileEntry { name: "main.rs", is_dir: false, children: vec![] },
                                FileEntry { name: "theme.rs", is_dir: false, children: vec![] },
                                FileEntry { name: "pi_clone", is_dir: false, children: vec![] },
                            ],
                        },
                        FileEntry {
                            name: "Cargo.toml",
                            is_dir: false,
                            children: vec![],
                        },
                    ],
                },
                FileEntry {
                    name: "secm-core",
                    is_dir: true,
                    children: vec![
                        FileEntry {
                            name: "src",
                            is_dir: true,
                            children: vec![
                                FileEntry { name: "lib.rs", is_dir: false, children: vec![] },
                                FileEntry { name: "settings.rs", is_dir: false, children: vec![] },
                            ],
                        },
                    ],
                },
            ],
        },
        FileEntry {
            name: "docs",
            is_dir: true,
            children: vec![
                FileEntry {
                    name: "adr",
                    is_dir: true,
                    children: vec![
                        FileEntry { name: "ADR-0003.md", is_dir: false, children: vec![] },
                    ],
                },
            ],
        },
        FileEntry {
            name: "Cargo.toml",
            is_dir: false,
            children: vec![],
        },
        FileEntry {
            name: "README.md",
            is_dir: false,
            children: vec![],
        },
    ]
}

/// 打开的文件标签（TabBar / FileViewer mock 共用；id 为文件路径）
#[derive(Debug, Clone)]
#[allow(dead_code)] // file_path 供后续 FileViewer 使用
pub struct FileTab {
    pub id: String,
    pub label: String,
    pub file_path: String,
}

