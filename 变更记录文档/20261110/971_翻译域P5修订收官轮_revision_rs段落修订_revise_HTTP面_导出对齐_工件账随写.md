# 971 号 · 翻译域 P5 修订收官轮：revision.rs 段落修订 + revise HTTP 面 + 导出 exportSourcePaths + 工件账随写

> 966 号施工图 P5 收官期（G8 修订域为主；G10 create 三输入中的 sources/sourceText 需
> creation-source/ingest 域，留独立轮；G11 导出对齐本轮完成 exportSourcePaths+工件账，
> epub 生成器对齐留备案）。
> 验证：Rust e2e translations70 10/10（+1 修订全链例）+translation lib 28/28+testgate
> engine-rs 2104（+1）/src-tauri 584+audit:rust 双 0（自检活体）+clippy 双 0（nonminimal
> bool/lint 三处清零）+gate:ts 十步全绿+U+FFFD 净。

## 一、修订域（G8）

- **端口**：`TranslationModelPort.revise_segment`（TS `reviseSegment?`——缺省实现返回
  `Translation model has no revision method`）；入参 `ReviseSegmentInput{source_language,
  target_language, chapter_title(译后), segment(带现译), neighbors(前后邻段带现译),
  glossary, instruction}`。
- **LLM 实现**：`submit_translation_revision` 结构化提交（system 逐字
  "Revise only the supplied translated paragraph…"；user=请求 JSON；validate=target
  trim 非空否则 `TRANSLATION_TARGET_EMPTY` 回喂重试）；技能块构造时注入（structured 环
  同款）。
- **revision.rs**（TS `reviseTranslationSegment` 逐字）：paragraph=`last` 或 1 基段号
  （0 → `TRANSLATION_PARAGRAPH_REQUIRED`）；章/段缺失 →
  `TRANSLATION_CHAPTER_NOT_FOUND`/`TRANSLATION_SEGMENT_NOT_FOUND`；未译 →
  `TRANSLATION_TARGET_MISSING`；改写后章译文+manifest 同事务（sync accept：审校面
  reviewSummary/observations 清空=TS undefined 覆写）；回执=TS 逐字段（changed/
  paragraphCount/isLastParagraph/sourceSegmentIndex/previous·target Hash(sha256:)/
  source·previous·target Excerpt(400 字)/reviewRequired/exportRequired/source·other
  Preserved）。
- **HTTP 面**：`POST /translations/:id/revise`（TS 修订走代理工具无独立路由——Rust 自有
  扩展，错误码=TS 码面映射：校验 400/缺失 404/不可用 501/其余 `TRANSLATION_REVISION_
  FAILED` 500）；技能装配与 run 同链。
- **工件账随写**（971 关键修复）：run 结尾补 TS `captureWorkSourceState`+
  `syncWorkSourceArtifacts{acceptPaths: changed}`——run 直写的章译文/manifest/报告翻到
  新 current 修订（此前 create 后账面永不过期，revise 的 sync 撞 "Source changed before
  revision commit"；必红 stash runner 恰该例红）。artifacts id 对齐 TS baseDir 相对派生
  （manifest/glossary/review/source-chapter-N/translated-chapter-N——首版全路径派生与
  sync 侧 filename 派生撞车，e2e 抓出后改正）。

## 二、导出对齐（G11 部分）

- `TranslationExportResult.exportSourcePaths`（TS 逐字段=章译文路径数组）。

## 三、测试与必红

- e2e 新例 `revise_paragraph_flow_and_export_source_paths`：run 产审校面 → 修订段 2 →
  changed/计数/isLast/哈希/excerpt 逐字段断言 + 修订请求 system 逐字+技能块 + 译文落盘
  + manifest 审校面清空 + work.json translated 工件修订在场 + 缺章 404/空白指令 400 +
  导出 exportSourcePaths。**必红**：stash runner+project → 恰该例红（sync 冲突 500），
  pop 后 10/10。
- mock：工具协议 mock 补 submit_translation_revision 分支（段带现译+instruction 断言）；
  活体 mock 同步补（revision 调用 skillBlock:true 实证）。

## 四、真引擎活体（8971，mock v3+修订分支）

- API 全链：create（works 布局）→ run（结构化翻译+回喂重试+审校 2.0）→ revise
  paragraph=last → changed=true/previousTarget=【译】It echoed./targetExcerpt=【润】…
  → export exportSourcePaths=works/…/translated/chapter-0001.json；盘面译文第三段=
  【润】…、manifest 审校面 None、work.json translated 工件 3 修订。
- IAB 翻译页：项目在列、章节卡「3 segments · 0 observations」（修订后审校清空）。
- 环境清偿：引擎/mock 杀净、临时根删除、IAB 标签关闭、stash 栈 0。

## 五、门禁与插曲

- clippy 双 0（len>=1 comparison 改 !empty）；testgate engine-rs 2104（+1）/src-tauri 584
  （INKOS_DUEL=1）+audit:rust 双 0+必红自检活体；gate:ts 十步全绿（diff 78/0）；U+FFFD 净。
- 插曲：门禁期间 cargo-audit 的 yanked 联网核验挂起（128 分钟 sleep 态）与 clippy 叠加
  拖满两次前台超时——杀僵尸进程后复跑即绿（945 号 yanked 超时同族，非代码回归）。

## 六、备案

- create 三输入（sources/sourceText/glossary+歧义守卫）需 creation-source/ingest 域，
  独立轮；epub-gen-memory 与 Rust 手搓 EPUB2 的字节级对齐不做（容器兼容已验，zip 读回
  断言在位）；修订工具未入 agent 工具注册表（TS 为代理域工具，Rust HTTP 面先行——代理
  工具注册表属 chat 域工程）。

**下一号自 972 起；出口=翻译 P5 余项（create 三输入/代理修订工具）/ C 类残余备案项；
用户侧待办=Fork 推送（含 955-971）。**
