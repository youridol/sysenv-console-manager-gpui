# ADR-0002：项目结构（Cargo workspace 与目录布局）

- 状态：提议
- 日期：2026-09-05
- 关联：ADR-0001、ADR-0003

## 背景

重构交付主目录：`Y:\SysEnv-Console-Manager\SysEnv-Console-Manager`（独立新 git 仓库，不携带旧历史）。
原仓库 `Y:\SysEnv-Console-Manager` 保留为迁移参照（迁移完成后按 ADR-0010 归档/冻结）。

## 决策

### D1：单 Cargo workspace，三个成员

```
SysEnv-Console-Manager/
├── Cargo.toml                 # [workspace] + 共享依赖版本
├── crates/
│   ├── secm-datasource/       # 迁入现 datasource/（纯 Rust 采集层，零改动）
│   ├── secm-core/             # 迁入并重构 src-tauri 业务模块（无 UI 依赖）
│   └── secm-app/              # 新：GPUI 二进制（UI 视图 + 应用装配 + main）
├── assets/                    # 图标/品牌资源（从源 public/、icons/ 迁移）
├── docs/
│   ├── adr/                   # 本决策记录
│   └── spec/                  # 功能基准规格（重构验收依据）
├── third_party/               # 保留：WinRing0/PawnIO 驱动依赖 + 许可（若有）
├── LICENSE                    # MIT（沿用 SECM Team）
├── README.md
└── CHANGELOG.md
```

### D2：分层依赖规则（严格单向）

```
secm-app (GPUI UI)
   └── secm-core (业务逻辑/采集编排，无 UI 类型)
          └── secm-datasource (Win32/注册表/网络原始采集，叶子)
```

- `secm-core` 不依赖 gpui：其 API 用普通同步函数 + `std` 回调/Channel 表达，UI 层负责线程调度。
- `secm-datasource` 保持叶子 crate（同现状规则，禁止反向依赖）。

### D3：线程模型归属
- UI 线程：GPUI 主线程（渲染 + 交互）。
- 阻塞采集：`BackgroundExecutor`（等价现 spawn_blocking），经 GPUI `cx.spawn` + subscription 回 UI。
- 常驻轮询（1s 传感器等）：后台循环 + 快照缓存，UI 订阅快照（对齐现 NetIfaceContext/1s 轮询语义）。

## 后果
- 清晰分层让 UI 与业务可独立测试。
- secm-core 可用 `cargo test` 直接单测（无窗口），datasource 单测延续。
- 每个 crate 独立编译单元，增量编译友好。
