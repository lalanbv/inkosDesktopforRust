# HTTP 端点登记与迁移排期

> 从 `packages/studio/src/api/server.ts`（Hono，134 路由）提取，按 core 域归类 + 映射迁移 Phase。
> 用途：strangler 迁移的端点切换排期依据（见迁移规划 v1 §4）。
> 生成日期：2026-08-11（Phase 0.4）

## 概览

| 维度 | 数量 |
|---|---|
| 总路由数 | **134** |
| GET | 58 |
| POST | 46 |
| PUT | 23 |
| DELETE | 7 |
| SSE（特殊） | 1（`/api/v1/events`） |
| 静态资源 | 1（`/assets/*`） |

**strangler 切换原则**：按域的 Phase 顺序，域内所有端点一次性切到 Rust axum，Node 侧该域端点下线。前端零改动（`/api/v1/*` 契约不变）。

---

## 按域归类 + 迁移 Phase

### Phase 1 — 叶子域（最先迁，无内部依赖）

#### `models` / `utils`（数据与工具，渗透在所有端点）
不单独占端点，但每个端点的 DTO 都依赖。**Phase 1 完成类型真源后，所有端点的契约测试即可建立。**

#### `translation` 域（6 路由）
| 方法 | 路径 |
|---|---|
| GET | `/api/v1/translations` |
| POST | `/api/v1/translations/create` |
| POST | `/api/v1/translations/upload` |
| GET | `/api/v1/translations/:id` |
| POST | `/api/v1/translations/:id/run` |
| GET | `/api/v1/translations/:id/export` |

#### `genres` / `materials` 域（5 路由）
| 方法 | 路径 |
|---|---|
| GET | `/api/v1/genres` |
| POST | `/api/v1/genres/create` |
| GET/PUT/DELETE | `/api/v1/genres/:id` |
| POST | `/api/v1/genres/:id/copy` |

---

### Phase 2 — 中层域

#### `notify` / `project` 配置类（14 路由）
| 方法 | 路径 |
|---|---|
| GET/PUT | `/api/v1/project` |
| GET/PUT | `/api/v1/project/language` |
| GET/PUT | `/api/v1/project/notify` |
| GET/PUT | `/api/v1/project/default-model` |
| GET/PUT | `/api/v1/project/model-overrides` |
| GET/PUT | `/api/v1/project/input-governance-mode` |
| GET/PUT | `/api/v1/project/chapter-review-mode` |
| GET/PUT | `/api/v1/project/detection` |
| GET/POST | `/api/v1/project/research-search` |
| GET | `/api/v1/project/files/:file{.+}` |
| GET | `/api/v1/project/artifacts/:file{.+}` |

#### `prompts` 域（4 路由）
| 方法 | 路径 |
|---|---|
| GET | `/api/v1/prompt-packs` |
| GET/PUT | `/api/v1/prompt-packs/:promptId` |
| POST | `/api/v1/skills/import` |

#### `skills` 域（3 路由）
| 方法 | 路径 |
|---|---|
| GET | `/api/v1/skills` |
| GET/PUT | `/api/v1/skills/:skillId` |

#### `state` 域（sessions，9 路由）
| 方法 | 路径 |
|---|---|
| GET/POST | `/api/v1/sessions` |
| GET/DELETE | `/api/v1/sessions/:sessionId` |
| POST | `/api/v1/sessions/:sessionId/abort` |
| GET/PUT | `/api/v1/sessions/:sessionId/play-mode` |

---

### Phase 3 — 高层域 + 引擎核心

#### `agent` / `agents` 域（books 业务主体，~50 路由，最大块）
| 方法 | 路径 | 子功能 |
|---|---|---|
| GET | `/api/v1/books` / `/api/v1/books/:id` | 列表/详情 |
| POST | `/api/v1/books/create` | 创建（异步状态） |
| GET | `/api/v1/books/:id/create-status` | 创建进度 |
| GET/PUT/DELETE | `/api/v1/books/:id/chapters/:num` | 章节 CRUD |
| POST | `/api/v1/books/:id/chapters/:num/approve\|reject` | 审核 |
| GET/PUT | `/api/v1/books/:id/chapters/:num/workspace` | 工作区 |
| PUT | `/api/v1/books/:id/chapters/:num/workspace/brief` | 大纲 |
| POST | `/api/v1/books/:id/chapters/:num/workspace/inspiration` | 灵感 |
| GET | `/api/v1/books/:id/chapters/:num/versions/:versionId` | 版本 |
| POST | `/api/v1/books/:id/chapters/:num/versions/:versionId/restore` | 回滚 |
| POST | `/api/v1/books/:id/write-next` | 续写（核心） |
| POST | `/api/v1/books/:id/draft` | 草稿 |
| POST | `/api/v1/books/:id/plan` / `compose` / `consolidate` | 规划/作曲/合并 |
| GET | `/api/v1/books/:id/eval` / `analytics` | 评估 |
| POST | `/api/v1/books/:id/audit/:chapter` | 审核 |
| POST | `/api/v1/books/:id/repair-state/:chapter` | 状态修复 |
| POST | `/api/v1/books/:id/foundation/revise` | 基础修订 |
| GET/POST | `/api/v1/books/:id/truth` / `truth/:file{.+}` | 真相文件 |
| POST | `/api/v1/books/:id/import/canon` / `import/chapters` | 导入 |
| POST | `/api/v1/books/:id/style/import` | 风格 |
| POST | `/api/v1/books/:id/export` / `export-save` | 导出 |
| POST | `/api/v1/books/:id/detect/:chapter` / `detect-all` / `detect/stats` | 检测 |
| POST | `/api/v1/books/:id/revise/:chapter` / `rewrite/:chapter` / `resync/:chapter` | 修订/重写/重同步 |
| POST | `/api/v1/books/:id/fanfic` / `fanfic/refresh` | 同人 |
| GET/PUT | `/api/v1/books/:id/chapter-review-mode` | 审核模式 |
| POST | `/api/v1/fanfic/init` / `imitation/init` / `spinoff/init` | 衍生初始化 |
| POST | `/api/v1/style/analyze` | 风格分析 |
| POST | `/api/v1/agent` | Agent 直接调用 |

#### `llm` 域（services，16 路由）
| 方法 | 路径 |
|---|---|
| GET | `/api/v1/services` |
| GET/PUT/DELETE | `/api/v1/services/:service` |
| GET | `/api/v1/services/:service/models` |
| GET/PUT/DELETE | `/api/v1/services/:service/secret` |
| POST | `/api/v1/services/:service/test` |
| GET/PUT | `/api/v1/services/config` |
| POST | `/api/v1/services/config/import-env` |
| GET | `/api/v1/services/models` |
| POST | `/api/v1/services/models/custom` |
| GET/PUT | `/api/v1/cover/config` |
| GET/PUT/DELETE | `/api/v1/cover/secret/:service` |

#### `pipeline` 域（daemon 调度，4 路由）
| 方法 | 路径 |
|---|---|
| GET | `/api/v1/daemon` |
| POST | `/api/v1/daemon/start` / `stop` |
| GET | `/api/v1/doctor` / `/api/v1/logs` |

#### `play` 域（互动游玩，5 路由）
| 方法 | 路径 |
|---|---|
| POST | `/api/v1/interaction/session` |
| GET | `/api/v1/play/runs/:worldId/:runId` |
| POST | `/api/v1/play/runs/:worldId/:runId/generate-image` |
| GET/PUT | `/api/v1/play/runs/:worldId/:runId/image-settings` |
| GET | `/api/v1/play/runs/:worldId/:runId/images/:file` |

#### `interactive-film` 域（互动电影/故事图，8 路由）
| 方法 | 路径 |
|---|---|
| GET | `/api/v1/interactive-films` |
| GET | `/api/v1/projects/:id/export` / `export/html` / `export/ink` / `export/json` |
| GET | `/api/v1/projects/:id/story-graph` |
| GET | `/api/v1/projects/:id/story-graph/analysis` / `delta` / `validation` |
| GET | `/api/v1/projects/:id/nodes/:nodeId/image` |

#### `forecast` 域（radar，2 路由）
| 方法 | 路径 |
|---|---|
| GET | `/api/v1/radar/history` |
| POST | `/api/v1/radar/scan` |

---

### 特殊端点（非业务，壳层职责或基础设施）

| 端点 | 处置 |
|---|---|
| `GET /api/v1/events`（SSE） | **壳层已实现 observer**：Rust 端复用现有 `observer/sse.rs` 模式。Phase 3 末随 agent 域一并迁。 |
| `GET /assets/*`（静态） | axum `ServeDir`，Phase 3 末切。 |

---

## 切换顺序建议（最小依赖风险）

```
Phase 1：translation(6) → genres/materials(5)            [11 端点，建立模板]
Phase 2：notify/project 配置(14) → prompts/skills(7)
        → state/sessions(9)                              [30 端点]
Phase 3：forecast(2) → interactive-film(8) → play(5)
        → pipeline/daemon(4) → llm(16)
        → agent/books(50+) → SSE/assets(2)              [85+ 端点，主力]
                                                      ─────
                                          Node sidecar 清空 → 移除
```

每步退出标准：该域端点在 Rust axum 上线 + Node 侧下线 + 端点契约测试 diff=0 + E2E 回归全绿。

---

## 待补充（Phase 1 启动时）

- [ ] 每个端点的请求/响应 DTO 登记到 `bindings/`（ts-rs 生成后回填此处链接）
- [ ] 标注异步/长任务端点（write-next/draft/create 等走 task-store，迁时需对齐任务模型）
- [ ] 标注需 SSE 推送进度的端点（迁时改 axum → Tauri event 或 axum SSE）
