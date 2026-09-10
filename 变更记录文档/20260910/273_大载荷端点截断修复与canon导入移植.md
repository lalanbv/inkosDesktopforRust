# 273 号：大载荷端点 2MB 截断修复+canon 上传/导入两端点移植

- 日期：2026-09-10
- 分支：develop
- 关联：267 号（端点冒烟扫——本批为其未覆盖面的深挖）、256 号（authoring 工具面）、271 号（并行会话脚本审计——其 audit-rust.mjs 边界提醒已在其收尾范围）
- 编号衔接：查当日目录最大号 272，顺延 273
- 推送核验：origin/develop = d26658b1（272 号后无新远端提交）；本批待提交

## 一、缺陷面：axum 默认 2MB 请求体上限截断大载荷端点（9 处）

**根因**：axum 0.7 默认对 `Bytes`/`String`/`Json` 等**提取器**施加 2MB 请求体上限（axum-core 源码实证：提取器走 limited body），超限返回 413 纯文本；而手工 `to_bytes(req.into_body(), n)` 不受限（`put_project_artifact` 的 16MB 即此形态）。TS 端（Hono）对 body 无统一上限、由各端点业务上限兜底——双端契约在此断裂，**超过 2MB 的真实用户载荷在 Rust 默认引擎全部 413**。

| 端点 | 载荷 | TS 上限 | 用户可见故障 |
|---|---|---|---|
| `style/import`、`style/analyze` | 整本文本风格指纹/分析 | 无 | 整本书文风导入失败 |
| `import/chapters` | **整本小说全文** | 无 | 导入既有小说（fanfic 前置）失败 |
| `fanfic/init`、`fanfic/refresh`、`spinoff/init`、`imitation/init` | sourceText 整本原著 | 无 | 同人/番外/仿写在 >2MB 原著上失败 |
| `/agent`（聊天） | 8 附件 ×4MB，base64 后 ~43MB | 4MB/件（业务层） | >1.4MB 附件先被 axum 截断，业务层 413 永不可达 |
| `translations/upload` | 解码上限 80MB | 80MB | 模块文档宣称 80MB，实际 2MB 即截（上限校验成死逻辑） |
| `skills/import` | 文件总限 8MB，base64 后 ~10.7MB | 8MB | >1.5MB 技能包导入失败 |
| `count-length`、`cap-context` | 整稿统计/裁剪 | 无 | 全稿字数统计失败 |

**修复**：`server/mod.rs` 新增 `read_body_capped(req, max)`（手工 `to_bytes`+显式上限）与四档 `BODY_CAP_*` 常量（TS 业务上限 ×4/3 base64 膨胀取整；TS 无上限者 64MB 安全上界，loopback 守卫已限定本机）。11 处处理器从提取器改为 capped 手工读（count-length/cap-context 顺带消除了 axum `Json` 提取器的 415 content-type 强校验偏差，与 TS `c.req.json()` 语义更齐）。

## 二、缺失端点移植：canon 上传与 canon-file（UI ImportManager 实际调用面）

`packages/studio/src/pages/ImportManager.tsx` L100/L105 调用 `/api/v1/import/canon/upload` 与 `/books/:id/import/canon-file`——engine-rs **全仓零命中**，导入管理器的原著导入流在 Rust 默认引擎下 404。本批移植：

1. **`server/upload_common.rs`**（新）：TS `safeUploadFileName`（trim→路径字符→`\s+` 折叠→非法段单下划线→UTF-16 码元 120 截断→兜底 "upload"）/ `parseDataUrl`（mime+参数段禁逗号、缺省 octet-stream、宽松 base64 复用 skill_routes 解码器并委托收敛）/ `storeProjectUpload`（`.inkos/uploads/{scope}/{毫秒}-{name}`、错误形态逐字 `{error:{code,message}}`）。
2. **`POST /import/canon/upload`**（style_routes）：dataUrl 上传，18MB 解码上限（TS `MAX_CANON_UPLOAD_BYTES` 逐字），`{storedPath,size,mimeType}`。
3. **`POST /books/:id/import/canon-file`**（style_routes）：`load_book_config` 存在性校验 → `ingest_material`（file 路径、title 去扩展名、purpose=reference）→ 读归一 markdown → 复用 fanfic/refresh 同款 `import_from_text` + `fanfic_canon.md` 落盘链（TS `importFanficCanon(id, sourceText, material.title, "canon")`，mode 字面量 canon）→ SSE `import:start/complete/error`（canon-file 变体的 error 事件**含 type 字段**，与 chapters 变体的逐字差异保留）→ `{ok,material}` / 500 平铺 `{error}`。

## 三、验证

| 门禁 | 结果 |
|---|---|
| upload_common 单测（3：文件名 TS 用例/dataUrl 解析/上传落盘与三错误形态） | ✓ |
| e2e sub273（canon 上传往返+错误形态 / canon-file 全链含 fanfic_canon.md 落盘+缺书 500 / **>2MB 载荷三端点**） | ✓ 3/3 |
| 全量 cargo test（INKOS_DUEL=1 真跑）：lib 1338 + e2e 200 + duel 10 + golden_leaf 70 + 其余套件 | ✓ 全绿 0 失败 |
| clippy --all-targets | ✓ 0 警告 |
| 生产 bin 冒烟（debug 直启 + 真实 HTTP）：health ✓ / canon upload 200 落盘实证（temp root 内文件在盘）✓ / canon-file 缺书 500 平铺 ✓ / **4.8MB style/analyze 200**（旧代码 2MB 即 413）✓ | ✓ |

## 四、遗留

1. translation 上传的 dataUrl 解析错误文案（"Translation upload has an invalid data URL"）与 TS 逐字（INVALID_ATTACHMENT_DATA_URL）存在既存微差——本批未动其行为（新共享实现已就位，收敛候选）。
2. `put_prompt_pack`（Bytes 提取器，提示包内容现实尺度 <100KB）维持 2MB 默认，未纳入放宽面。
3. 两项默认值、ja A/B、历史瘦身——待用户决策（承接 272 号）。
