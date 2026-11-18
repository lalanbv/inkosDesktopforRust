# 1576 逐功能测试 + check-routes 门禁正则修复轮（2026-11-19）

## 定性

按 /goal「逐功能测试」：1575 修复三站逐站核验行为级覆盖——embed_batch/detection 已有活体（主绿 A/B），**DOCTOR `/models` 腿只有编译保护**（判定面靠 loopback_proxy 17 案表测，接入点本身未测）为真缺口，本轮补齐；随后实跑 gate:ts 浮出 check-routes 腿真红并根因修复。

## 修复（2 处）

1. **`engine-rs/src/server/ops_routes.rs`**：DOCTOR `/models` 快探腿从 handler 内联抽出为可测单元 `pub async fn doctor_models_probe(base, api_key) -> bool`（谓词钉实际请求目标语义不变，外层 9s 预算仍主导）。
2. **`scripts/check-routes.mjs` 根因修复（本轮实跑浮出的真红）**：handler 形态判定正则 `/pub async fn ([a-z_0-9]+)\s*\(.*?\)\s*->\s*impl IntoResponse/gs` 的裸 `.*?` 跨行吞尾——从「签名不含 impl IntoResponse 的普通 pub async fn」括号一路吞到**下一个真 handler** 尾部闭合，一个误匹配还吃掉该真 handler 自己的命中位。新抽的 `doctor_models_probe`（`-> bool`）触发误报暴露此疾。收紧为 `\([^;{}]*?\)`（真 handler 签名段不含 `;{}`）+ 白名单勘误：**413 号三项白名单（effective_router/save_radar_scan/peek_create_status）全部是同一根因的误匹配产物**（新正则 5↔5 对称实证：save_radar_scan→post_radar_scan、doctor_models_probe→get_doctor、effective_router+draft→plan+revise、peek_create_status→create_book），五个真 handler（含 get_doctor/post_radar_scan/create_book 等）的路由注册核验在旧正则下**从未生效**（只因都注册了才没爆）；正则收紧后白名单无命中对象，删除。**影响面：1569→1576 间 check-routes 腿的 handler 面核验是残缺的**（registered 207 面不受影响）。

## 活体（红转绿实证）

- 新活体 `engine-rs/tests/doctor_loopback_no_proxy_live.rs`（独立进程防 env 外泄）：**标记代理法两臂闭环**——代理回 599 并对每笔连接计数：回环臂「通且计数=0」证明直连（未过代理）+ Bearer api_key 校验真传递；远端臂（TEST-NET-3 192.198.152.10）「败且计数+1」证明真走代理。撤修转红四拍：撤谓词成默认 builder（1575 前漏网形态）→ 回环臂经标记代理被 599 红（88 行）→ 复原 → 绿。
- check-routes 红转绿：旧正则红（gate:ts ✗ + 单跑孤儿=1）→ 收紧后绿（handler=110 不变、registered=207 不变、孤儿=0、get_doctor 恢复在检）。

## 验证

- cargo:testgate 双 crate 全绿 **2723 passed**（engine-rs 56 目标 2124 + src-tauri 599，+1 新活体）；clippy 0；fmt 0；gate:ts 十腿全绿（check-routes 0.1s 绿）；U+FFFD 净。
- 1575 既有活体（embed/detection）复跑仍绿；既有 get_doctor 单测（127.0.0.1:9 谓词旁路语义）兼容不变。

## 后续

- 下号自 1577 起；用户侧待办不变（Fork 推送 955-1576 + 正式打包三脚本重烤）。
