/// observer 模块：监听 inkos sidecar 的 SSE 事件流并转发到 Tauri 前端。
///
/// 本模块当前包含 `sse`（纯函数帧解析）与 `router`（事件分派）。后续 task 将加入：
/// - `notifier`：系统通知
/// - `tray_badge`：托盘角标
pub mod router;
pub mod sse;
