# ADR-0011：仓库与版本策略

- 状态：**已定案（用户确认：新仓库独立历史）**
- 日期：2026-09-05

## 决策

- 采用**方案 B：新仓库（纯 GPUI 版）**——重构代码在目标目录
  `Y:\SysEnv-Console-Manager\SysEnv-Console-Manager` 建立**独立 git 仓库**，从 **v2.0.0** 起全新历史。
- 旧 Tauri 仓库 `youridol/sysenv-console-manager` 保持不动；新仓库 README 声明
  "源于 Tauri 版 SECM，历史见原仓库链接"。
- 版本：v2.0.0（MAJOR：技术栈整体替换），单点维护于根 `Cargo.toml`
  （workspace.package.version），同步 README 徽章 + CHANGELOG。

## 背景（备选方案与理由）

### 方案 B（选定）：新仓库（纯 GPUI 版）
- 在子目录新建独立 git 仓库，从 v2.0.0 起全新历史；旧 Tauri 仓库保留供参照。
- 优点：仓库体积小、无旧技术栈历史、满足"干净、纯 Rust+GPUI、无迁移遗留"要求。
- 缺点：与旧仓库 issue/star 不共享（README 互链缓解）。

### 方案 A（未选）：同一仓库演进
- 在现 public 仓库继续开发，历史保留。因与"纯 GPUI 无迁移遗留"目标相悖而弃选。
