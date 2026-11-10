# 972 号 · 翻译域 create 三输入轮：sources/sourceText/glossary + 歧义守卫 + creation_source lineage

> 966/971 备案余项：create 三输入形态（TS `CreateTranslationProjectInput` 的
> sources/sourceText/glossary + `loadCreationSource` 歧义守卫与 lineage）。新模块
> `agent/creation_source.rs`（TS `agent/creation-source.ts` 翻译域切片）。
> 验证：Rust e2e translations70 11/11（+1 三形态例）+translation lib 28/28+testgate
> engine-rs 2105（+1）/src-tauri 584+audit:rust 双 0（自检活体）+clippy 双 0+gate:ts
> 十步全绿+U+FFFD 净。

## 一、creation_source.rs（新模块，agent.rs 占位转实）

- `load_creation_source(project_root, target_work_id, sources, source_text, source_path,
  source_name)`：TS `loadCreationSource` 逐字切片——①歧义守卫（多形态并给 →
  `CREATION_SOURCE_CONFLICT`）；②registered 引用：`read_artifact_revision`（lineage 钉定
  旧修订 resume 用）+ 文本类型守卫（`SOURCE_NOT_TEXT`）；③inline 文本（缺省名 source）；
  ④项目内文件：`works/<id>/…` 路径须为注册工件（`SOURCE_ARTIFACT_REQUIRED`），否则
  `ingest_material` 归一；绑定校验（draft 已有 source-material.md 时正文与 lineage 必须一致，
  `CREATION_SOURCE_CONFLICT`）。
- **备案**：TS inline 文本的 `currentExecutionAuthorRequest` 交叉校验（作者逐字）依赖聊天
  执行 ALS——HTTP 直连 create 无执行作用域（TS 直连面同样仅会话内生效），不移植。
- `source_path` 绝对路径 → `CREATION_SOURCE_PATH_ABSOLUTE`（TS 抛错同语义，码面 Rust 化）。
- **路径教训（必红探针抓出）**：ingest 分支传 `safe_child_path` 解析后的绝对路径会在
  `read_material_source` 内二次 `safe_child_path` 时 root+abs 拼接（Rust 逻辑归一不做
  TS `resolve` 的绝对替换）→ 改传原始相对 `sourcePath`。

## 二、create 接线（translation_routes.rs + project.rs）

- HTTP：`sources`（数组）/ `sourceText` / `filePath` 三形态；缺失检查先于语言（70 号契约
  序）；歧义守卫在 HTTP 预检层（filePath 与 sources/sourceText 并给 → 400
  `CREATION_SOURCE_CONFLICT`——filePath 走原链不进 load_creation_source，守卫前移）；
  形态判定按**键存在性**（`sources` 键在场含空数组 → `CREATION_SOURCE_REQUIRED`，空数组
  非缺省）；仅 sources/sourceText 走 creation_source，filePath 保持 70 号原链
  （extractTranslationSource 直读，PDF/EPUB 章切不经 material 归一）；`workId`/`sourceName`
  透传。
- project.rs：`CreateTranslationProjectInput` 增 `resolved: Option<&CreationSourceResolved>`
  + `glossary`；resolved 形态正文落 `source/source-input.md`（TS sourceText 同款 writes），
  manifest `source.path` 指向它（首版空串致该形态无法追溯——e2e 抓出）；初始 glossary 落池
  （merge 后）；lineage 进 work.json；metadata.sourceOrigin=file/inline。

## 三、测试与必红

- e2e 新例 `create_three_source_forms_with_lineage_and_glossary`：①sourceText+glossary
  （落池断言+work.json sourceOrigin=inline+lineage 空）；②registered 引用（lineage
  derived-from 三键逐字=base 工作与章节工件+revisionId）；③歧义 400/空 sources
  `CREATION_SOURCE_REQUIRED` 400；④file 形态回归；列表全项目在列。
- **必红**：stash 全部实现（含未跟踪 agent/）→ 新例**编译失败**（三输入字段在旧类型上
  不存在=行为不可达红）；NEUTER972 中和 HTTP 歧义守卫 → 恰该例红（冲突请求 500 非 400）；
  撤除后 11/11，残留 0。
- 过程修复实录：①首版 registered sources 走 ingest 分支读 upload 文件（`safe_child_path`
  绝对二次拼接 ENOENT——探针打印抓出）→ 改 registered/file 分离；②file 形态被误导入
  creation_source（material 归一改变分段）→ handler 按 file_path 有无分流；③形态判定
  按非空而非键存在（空 sources 误报 MISSING_FILE_PATH）→ 改键存在性。

## 四、真引擎活体（8972，mock v3）

- API 全链：①file 形态建底座 → ②sourceText+glossary（落池断言+sourceOrigin=inline+
  lineage 空）→ ③registered 引用 base 的 `source-chapter-0001`（lineage derived-from 三键
  逐字、source.path=source-input.md）→ ④歧义 400 `CREATION_SOURCE_CONFLICT` → 列表
  3 项目。
- 环境清偿：引擎/mock 杀净、临时根删除、IAB 标签关闭、stash 栈 0。

## 五、门禁

- clippy 双 0；testgate engine-rs 2105（+1）/src-tauri 584（INKOS_DUEL=1）+audit:rust
  双 0+必红自检活体；gate:ts 十步全绿（diff 78/0）；U+FFFD 净。

## 六、备案

- TS `bindCreationSource`（source-material.md 绑定）属创作 Work 通用域，翻译域按
  project.ts 现状（不写 source-material.md）不引入——若后续翻译进统一 Work 生产线再接。
- inline 文本作者逐字校验（SOURCE_REFERENCE_REQUIRED）依赖执行 ALS，不移植（§一）。

**下一号自 973 起；出口=代理修订工具注册表 / epub 生成器字节对齐 / C 类残余备案项；
用户侧待办=Fork 推送（含 955-972）。**
