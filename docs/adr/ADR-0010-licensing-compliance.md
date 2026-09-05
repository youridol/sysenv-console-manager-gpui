# ADR-0010：许可与第三方合规（开源续期）

- 状态：提议
- 日期：2026-09-05

## 背景

SECM 已在 GitHub 公开（MIT，SECM Team，v1.19.0 Tauri 版）。重构为纯 Rust + GPUI 后需保持开源合规并同步更新仓库。

## 决策

### D1：主许可证不变
- 主项目 MIT（Copyright (c) 2026 SECM Team），LICENSE 文件沿用。

### D2：GPUI 及其依赖许可
- gpui 0.2.2 为 Apache-2.0 → README/关于页第三方许可清单新增（Apache-2.0 兼容 MIT 分发，需保留声明）。
- 其余新增 crate（tray-icon/rfd/open 等）逐一核对其许可（多为 MIT/Apache-2.0/BSD），统一在"第三方许可"清单登记。
- 沿用 scripts/audit/licenses-cargo.mjs 逻辑审计 workspace Cargo.lock。

### D3：保留组件的既有合规义务
- LHM sidecar（MPL-2.0）+ PawnIO（GPL-2.0+例外）+ WinRing0（BSD-2-Clause）：源码/许可/来源追溯文件随仓库与发布包保留（沿用 third_party/ 与 sidecar-lhm/licenses/ 结构，见 ADR-0002）。
- datasource crate（MIT）：保留其 license 字段与作者信息。

### D4：仓库与发布
- 采用新仓库独立历史（ADR-0011 已定案）：在目标目录新建独立 git 仓库，从 v2.0.0 起全新历史；旧 Tauri 仓库保留供参照。
- 新版本 v2.0.0（MAJOR：技术栈整体替换），版本单点维护于根 `Cargo.toml`（workspace.package.version）。

## 后果
- 开源合规面与现状一致并小幅新增（gpui Apache-2.0 声明）。
- 审计脚本（licenses-cargo）在新 workspace 继续可用。
