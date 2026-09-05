# ADR-0004：异步与数据轮询模型

- 状态：提议
- 日期：2026-09-05
- 关联：ADR-0001（BackgroundExecutor）、ADR-0002（分层）

## 背景

原实现并发纪律（S8 规范）：GUI 线程永不触碰采集逻辑；阻塞采集一律 spawn_blocking；网络工具用 tokio 流式 + Channel 推送事件；取消走 cancel.rs 注册表。
GPUI 0.2 提供 `cx.spawn`（前台异步）、`BackgroundExecutor`（后台线程）、subscription/`cx.subscribe`；主循环为单线程事件驱动。

## 决策

### D1：阻塞采集 → BackgroundExecutor
- secm-core 暴露同步 API；UI 层经 `BackgroundExecutor::spawn` 包裹，结果经 `cx.spawn` 回 UI 更新状态。
- 等价现 `spawn_blocking`；采集函数不得触碰 GPUI 上下文（分层规则保证）。

### D2：常驻轮询（1s 传感器、网络速率）
- 后台一个常驻线程循环采集 → 写 `Arc<Mutex<Snapshot>>`（或 mpsc 通道）→ 通过事件/订阅通知 UI 重绘。
- 对齐原：Dashboard 1s get_sensor_data；NetIfaceContext 1s get_net_stats（0.5–5s 可调）。
- 快照语义（对齐现 SensorData JSON）：cpu（freq/temp/power/source 标记）、gpu[]、mem、disks、motherboard、diag。

### D3：流式工具（ping/traceroute/nslookup/NAT/iperf3）
- 保留 surge-ping / trust-dns 等同步或阻塞实现，在后台线程执行；
- 输出经 `mpsc`/`crossbeam channel` 流式发往 UI（对齐原 Channel 事件：kind ∈ {info, summary, error, cancel}）；
- 取消：工具循环检查 `AtomicBool`（对齐 cancel.rs 语义），UI 发送取消命令置位。

### D4：生命周期与任务清理
- 页面切换时取消未完成任务（对齐原 cancel_net_cmd + genRef 竞态保护）；
- GPUI View drop 时清理订阅，防止泄漏。

## 后果
- 单事件循环模型下所有 UI 更新回主线程，天然无数据竞争面。
- 采集与 UI 解耦，secm-core 可脱离窗口单测。
