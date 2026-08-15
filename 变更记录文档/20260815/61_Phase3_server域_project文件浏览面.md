# 61 号变更记录：project 文件浏览面端点组（Phase3 · server 域）

- 日期：2026-08-14
- 范围：engine-rs（`src/server/project_files_routes.rs` 新建、`src/server/mod.rs` 路由挂载、`Cargo.toml` 依赖、E2E 测试）
- 契约源：`packages/studio/src/api/server.ts` L4270-L4320（三端点）+ L439-L524（`resolveProjectImageFile` / `normalizeProjectGeneratedPath` / `resolveProjectTextArtifactFile`）

## 背景

strangler 迁移第 61 号。Studio 的文件浏览面：生成图片预览（封面/短篇/互动影视）
与文本工件读写（剧本/分镜/短篇等生成物）。前端 `<img src>` 直接消费 files 端点；
artifacts 端点支撑剧本编辑器打开/保存生成工件。均为通配多段路径端点。

## 交付

### 端点（`src/server/project_files_routes.rs`，~300 行）

| 端点 | 契约 |
|---|---|
| GET /api/v1/project/files/*file | 图片二进制回流 + `Cache-Control: no-store`；读失败 404 |
| GET /api/v1/project/artifacts/*file | `{path, content, contentType, size}`；读失败 404 |
| PUT /api/v1/project/artifacts/*file | mkdir -p 父目录 + 写入 → `{ok, path, contentType, size}` |

路径校验链逐字对齐：

- **解码**：手动 `decodeURIComponent`（严格模式：每个 `%` 必须后随两位 hex，
  非 UTF-8 序列拒绝）→ 400 `INVALID_PROJECT_FILE_PATH/ARTIFACT_PATH`
  "Invalid project file/artifact path"
- **剥头部 `/`**（`replace(/^\/+/u, "")`）
- **安全段校验**：空 / `\0` / 绝对路径 / `[\\/]+` 分隔的 `..` 段 → 400
- **前缀白名单**：files 端点 `shorts/ | covers/ | interactive-films/`（否则 400
  "Only generated shorts/, covers/, interactive-films/ images can be previewed"）；
  artifacts 端点五前缀 `dramas/ | storyboards/ | interactive-films/ | shorts/ | covers/`
  （否则 400 "Only generated writing artifacts can be opened"）
- **ext → contentType**：files 端点 png/jpg/jpeg/webp（否则 415
  `UNSUPPORTED_PROJECT_FILE_TYPE`）；artifacts 端点 md/markdown/txt/json（否则 415
  `UNSUPPORTED_PROJECT_ARTIFACT_TYPE`）
- **逃逸兜底**：resolve + relative（词法 join 后 strip_prefix root）

- 新依赖：`percent-encoding = "2"`（axum 传递依赖，显式声明）

### 路由形态

Hono `:file{.+}`（多段通配）→ axum 0.7 `*file` 通配路由；handler 用 `Request`
提取器取 `uri().path()`（保留 percent 编码，与 Hono `c.req.param` 返回 raw 段一致），
剥固定前缀后手动解码——避免 axum `Path` 提取器的自动单次解码造成双重语义分歧。

## parity 要点

- 错误形状 `{"error":{"code","message"}}`；404 为 Hono `c.notFound()` 形态
  （text/plain "404 Not Found"）
- `size` 为 UTF-8 字节数（TS `Buffer.byteLength`）
- PUT body 校验：`json().catch(() => null)` + `payload && typeof === "object" &&
  "content" in`（数组/原始值/null 均 400 `INVALID_PROJECT_ARTIFACT_BODY`
  "content must be a string"）
- ext 提取 `split(".").pop()`（无点时整串小写参与匹配 → 415）

## 偏差备案

- **percent_decode 严格性**：`percent_decode_str` 对非法 `%xx` 宽容透传，而
  `decodeURIComponent` 严格抛 URIError——已补严格 `%` 扫描（每个 `%` 后随两位
  hex），E2E 用 `%zz` 固化该分支
- **空通配段**：Hono `{.+}` 要求 ≥1 字符（空 → 404 路由不匹配）；axum `*file`
  匹配空串 → 命中 handler → 走 400 校验分支。边缘 case（`/project/files/`），
  行为差异 404 vs 400，前端不触达
- **fs 写失败**：TS 无 try/catch（冒泡 Hono 500）；Rust 给结构化 500
  `INTERNAL_ERROR`（E2E 不覆盖权限错误分支）

## 暂缓件

- 无（本域三端点全量移植）

## 下一步（62 号候选）

- Node sidecar 下线核对（Rust 侧业务端点累计 82 个，切换核对未执行）
- resolveEffectiveLLMConfig 全链（env 层，54/56 号偏差备案遗留）
- reviseFoundation、resumeFrom 断点续导、同步钩子（59 号遗留清单）

## 影响面

- Rust 业务端点 79 → 82 个；e2e 60 → 65；lib/golden/export-bindings/TS 基线不变
- 全量基线：`cargo test --lib`（953）/ `golden_leaf`（76）/
  `e2e_write_next_contract`（65）/ `--features export-bindings --lib`（1112）/
  `clippy --lib --tests --bins`（零警告）/ TS vitest（185 文件 1798 测试）
