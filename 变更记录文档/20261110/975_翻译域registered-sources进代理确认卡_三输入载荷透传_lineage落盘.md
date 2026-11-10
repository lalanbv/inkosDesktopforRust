# 975 号：翻译域 registered-sources 进代理确认卡——三输入载荷透传 + lineage 落盘

> 日期：2026-11-10。上游 972 号备案清偿（HTTP create 三输入已落地，代理确认卡面
> 未接入——本号补齐）。966 施工图 P5 余项全部收口。
> 门禁：e2e 224/224+testgate engine-rs 2110（+1）/src-tauri 584+audit:rust 双 0
> （自检活体）+clippy 双 0+gate:ts 十步全绿+U+FFFD 净。

## 一、载荷面（agent_route.rs strict schema）

`translationCreate` strict schema 补四键：`filePath`（StrNonEmpty）/`sourceText`
（StrNonEmpty）/`sources`（**PayloadField::ArrayObj 新变体**——数组且元素为对象，
校验 `is_array()`；此前 Object 变体与实际数组形态不匹配，首轮 e2e 抓出）/
`glossary`（ArrayObj）。既有 `sourceLanguage`/`targetLanguage`/`title`/
`segmentMaxChars` 不动。

## 二、确认卡（agent_production.rs 2971 区块）

`translation_create` 确认卡三形态守卫：`filePath`/`sourceText`/`sources` 任一在场
即放行（全缺→既有双语文案守卫不变）；params.insert 八键
（filePath/sourceText/sources/glossary/sourceLanguage/targetLanguage/title/
segmentMaxChars）。歧义形态（filePath+sourceText 并给）不在此层拦——经执行面
`load_creation_source` 的 `CREATION_SOURCE_CONFLICT` 透传（与 TS 层序一致：守卫
在 load 内、HTTP 预检层 filePath 并给走 400，代理面统一 502 AGENT_ACTION_FAILED
包裹码面消息）。

## 三、执行体（agent_production.rs execute_translation_create 重写）

签名 Option 形态（file_path/source_text/sources/glossary: Option<&Value>）→
`load_creation_source(root, None, sources.and_then(as_array→as_slice), source_text,
file_path, None)` → `resolved: Some(&resolved)` 直传（首版 `resolved_ref.as_deref()`
同型解引用 clippy 抓出）→ glossary 术语解析（source/target 必需 trim、note 可选，
filter_map 跳过残行）→ `CreateTranslationProjectInput { file_path: None,
resolved, glossary, ... }`。clippy 四修：`#[allow(too_many_arguments)]`（10/7）、
`format!` 无参、`map().flatten()` 改 `and_then`、同型 deref。

## 四、lineage 落盘链（972 号既有机制复核确认）

`load_creation_source` registered 分支 `read_registered`（read_artifact_revision+
lineage 钉定旧修订+SOURCE_NOT_TEXT 类型守卫含 **xhtml 放行**——章节文件
application/xhtml+xml 是 translation-create registered 源主形态，975 补）→
`CreationSourceResolved { text, name, lineage }` → project.rs resolved 分支单章
ExtractedTranslationSource+正文落 `source/source-input.md`+`lineage.clone()` 进
work.json 构造（972 号已实现，本号零改动复核确认——压缩期探针误报勘误：
work.json 一直写对，探针读错目录）。

## 五、测试（e2e_write_next_contract.rs）

`translation_create_three_forms_confirm_flow`（+1，translation75 组 3/3）：②
sourceText+glossary → source-input.md 在场+glossary 落池+sourceOrigin=inline+
lineage 空；①registered sources（引用 ② 的 `source-chapter-0001` 工件）→
work.json `lineage.len()==1` derived-from 三键逐字（sourceWorkId/sourceArtifactId/
sourceRevisionId）；歧义 filePath+sourceText → 502 `CREATION_SOURCE_CONFLICT`。
manifest 定位经 `toolExecutions[0].details.manifest`（agent 路由响应形）；
manifestPath 为项目根相对路径（与 root 拼接——e2e 进程 CWD 与项目根无关）。

## 六、必红两形

- **NEUTER975**：project.rs `lineage: lineage.clone()` 中和为空数组 → 恰
  three_forms 的 lineage 断言 1 红（left:0 right:1）→ 撤除后 translation75 3/3
  绿+全仓 NEUTER975 残留 0。
- **stash 形**：stash 实现面三文件（creation_source/agent_production/
  agent_route）→ HEAD 形态 strict schema 拒 `glossary` 键 → 400
  INVALID_ACTION_PAYLOAD 恰红（行为不可达）→ pop 后 3/3 绿。

## 七、真引擎活体（8976，mock 18975）

①inline 创建（sourceText+glossary[Mana→魔力]）→ work.json sourceOrigin=inline+
lineage 空+profileId=translation+glossary 落池；②registered 引用
`source-chapter-0001` → derived work.json lineage 一条 derived-from 三键逐字+
sourceRevisionId 为串+source-input.md 正文含原文；③歧义 filePath+sourceText →
502 CREATION_SOURCE_CONFLICT 码面消息逐字；④GET debug/tools 33 条全唯一+
revise_paragraph 在册（973 回归）；⑤GET /translations 列表 2 项目（响应形
`{"translations":[...]}`——解析首版取错键误报 0，勘误）。环境清偿：引擎/mock
杀净、临时根删除、stash 栈 0。

## 八、门禁与插曲

- e2e 224/224（+1）；testgate engine-rs 52 目标 2110（+1）/src-tauri 584；
  audit:rust 双 0+已知漏洞锁自检活体（7/1 RUSTSEC 检出）；clippy 双 0（四修见
  §三）；gate:ts 十步全绿；U+FFFD 净。
- 插曲：`test:gate` 脚本名不存在（实为 `cargo:testgate`）；压缩期探针误报
  lineage 丢失（work.json 一直写对，探针读错目录+轮次陈旧态混叠）——复跑即绿，
  未改 lineage 链一行；clippy 四修集中在 execute_translation_create 新签名。

## 九、备案

- TS 全量面差异承 972 号备案不变：`currentExecutionAuthorRequest` 交叉校验（ALS
  依赖）与 `bindCreationSource`（翻译域不写 source-material.md）不移植。
- 歧义守卫在代理面统一 502 包裹（HTTP 直连面 400 不变）——两路码面一致、状态码
  按 transport 惯例分层，与 TS 直连/会话两路同构。

## 十、下一号与出口

下一号自 **976** 起；出口=C 类 SSE 残余项逐事件定性收编（director/foundation/
repair-state 等）/EPUB 字节级细节（如需）。用户侧待办=Fork 推送（含 955-975）。
