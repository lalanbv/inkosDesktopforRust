# 60 号变更记录：skills / prompt-packs 轻域端点组（Phase3 · server 域）

- 日期：2026-08-14
- 范围：engine-rs（`src/skills/external_loader.rs` 新建、`src/skills/mod.rs` 扩展、`src/server/skill_routes.rs` 新建、`src/server/mod.rs` 路由挂载、`Cargo.toml` 依赖、E2E 测试）
- 契约源：`packages/studio/src/api/server.ts` L4209-L4268（六端点）+ L756-L969（实现函数）+ `packages/core/src/skills/external-loader.ts`（245 行）+ `packages/core/src/prompts/prompt-pack.ts` + `packages/core/src/interaction/action-envelope.ts`（SkillIdSchema）

## 背景

strangler 迁移第 60 号。skills/prompt-packs 是 Studio 的提示词与技能管理面：
GET 列表 + prompt 覆盖编辑（PUT/DELETE）+ 技能导入/删除。此前 Rust 侧已有
`prompts/`（builtin 12 prompt / 3 pack + 三级覆盖加载）与 `skills/mod.rs`
（AgentSkill 类型 + 注册表），但缺 external-loader（磁盘扫描）与全部 HTTP 端点。

## 交付

### 核心域（`src/skills/external_loader.rs`，~470 行含测试）

- `parse_agent_skill_document`：SKILL.md frontmatter 解析（单 BOM 剥离、
  CRLF/CR→LF 归一、`---\n` 开头 + `\n---` 闭合、serde_yaml 解析）；
  name/description 必填（trim 非空、UTF-16 码元上限 64/1024）；
  `normalize_external_skill_id`：NFKD + lower + 非 [a-z0-9] 段折叠 `-` +
  去首尾 + 非字母开头补 `skill-` 前缀（错误消息逐字）
- `load_configured_agent_skills`：五目录扫描（`INKOS_SKILL_DIRS` 显式 external →
  `~/.openclaw/skills`、`~/.agents/skills` user → 项目 `.agents/skills`、`skills`
  project）；单技能失败进 diagnostics；显式目录缺失诊断、非显式缺失静默（ENOENT 语义）
- `discover_skill_dirs`：绝对路径校验 + manifest 直查 + 2 层深度扫描 + 排序去重
- `list_project_skill_ids`：项目技能 id 集（目录名 SkillIdSchema 校验、SKILL.md 普通文件）
- 新依赖：`unicode-normalization = "0.1"`（NFKD，TS `String.normalize("NFKD")` 对应物）

### `src/skills/mod.rs` 扩展

- `normalize_skill_id_strict` + `SkillIdError`：SkillIdSchema 校验
  （`z.string().trim().min(1).regex(/^[a-z][a-z0-9-]*$/i)` → 小写化），消息为 zod 默认文本

### 端点（`src/server/skill_routes.rs`，~560 行）

| 端点 | 契约 |
|---|---|
| GET /api/v1/skills | `{skills, diagnostics}`；注册表归并（id 排序）+ project 标记 |
| GET /api/v1/prompt-packs | `{packs: 3, prompts: 12}`；project 覆盖优先 |
| PUT /api/v1/prompt-packs/:promptId | 404 未知 id / 400 非 JSON / 400 content 非串；写覆盖文件后回 `{prompt}` |
| DELETE /api/v1/prompt-packs/:promptId | rm force（ENOENT 静默）→ `{prompt}`（回 builtin 态） |
| POST /api/v1/skills/import | dataUrl 文件组校验链 → staging `.import-{uuid}` 原子落盘 `{skill}` |
| DELETE /api/v1/skills/:skillId | 404 SKILL_NOT_FOUND / 删目录 `{ok:true}` |

校验链逐字：路径安全（trim、`\`→`/`、`./+` 前缀剥离、绝对/盘符/`\0`/段空/`.`/`..`
拒绝，消息带原值）、大小写不敏感重复路径、128 文件 / 单 2MB / 总 8MB 上限（413）、
唯一 SKILL.md、folder 前缀约束、manifest 解析 400 INVALID_SKILL_MANIFEST、
目录冲突 409 SKILL_EXISTS。`decode_base64_lenient` 手写 6-bit 累积解码器
（Node `Buffer.from(s,"base64")` 宽松语义：忽略一切非 base64 字符），
零新依赖；`parse_data_url` 对齐 `^data:([^;,]+)?(?:;[^,]*)?;base64,(.*)$`
（`;base64,` 匹配点前禁逗号，payload 可含逗号换行）。

## parity 要点

- 错误形状 `{"error":{"code","message"}}`（ApiError onError 逐字）
- `undefined` 字段不序列化（StudioSkill.path / StudioPromptPackPrompt.path 用
  `skip_serializing_if`）
- prompt 覆盖路径 `{root}/prompt/{seg}/{last}.md`（`prompt_override_path` 复用 59 号前已有件）
- GET /skills 的 toStudioSkill：project id 命中 → source 强制 "project"、editable=true、
  path 为 posix 相对路径
- 空数组/空串语义：files 缺失与空数组同错（TS `Array.isArray → length===0`）
- axios/hono `c.req.json().catch` → Rust `from_slice` else 分支（非 JSON 400 两段消息逐字）

## 偏差备案

- **YAML 语法错误消息**：serde_yaml 与 js-yaml 的报错文本不同（消息透传，code
  INVALID_SKILL_MANIFEST 一致）；E2E 不覆盖语法错分支
- **GET /skills 的 user 层目录**：TS `homedir()` 永不失败（返回空串时 join 产生相对路径）；
  Rust `HOME`/`USERPROFILE` 均缺失时跳过 user 候选（实际桌面环境不触发）
- **zod min(1) 消息**：空串消息取 zod 3 默认形态 "String must contain at least 1
  character(s)"（该分支实际由 regex 分支前置，不达）
- **递归 async**：`discover_skill_dirs_below` 用内部 `pinned()` Box::pin + Send
  绕过 E0733（axum Handler 要求 Send future）

## 暂缓件

- TS `getPromptPackGuidance`/appendPromptPackGuidance 已有 Rust 对应件（未挂端点，无需）
- skills 的 user 层覆盖端点（TS 亦无此端点）

## 下一步（61 号候选）

- `GET/PUT /project/files|artifacts` 文件浏览面（server.ts L4270+）
- Node sidecar 下线核对（Rust 侧业务端点累计 79 个）
- resolveEffectiveLLMConfig 全链（env 层，54/56 号偏差备案遗留）

## 影响面

- Rust 业务端点 73 → 79 个；lib 测试 941 → 953；e2e 52 → 60；
  export-bindings 1100 → 1112；TS 基线不变（1798）
- 全量基线：`cargo test --lib`（953）/ `golden_leaf`（76）/
  `e2e_write_next_contract`（60）/ `--features export-bindings --lib`（1112）/
  `clippy --lib --tests --bins`（零警告）/ TS vitest（185 文件 1798 测试）
