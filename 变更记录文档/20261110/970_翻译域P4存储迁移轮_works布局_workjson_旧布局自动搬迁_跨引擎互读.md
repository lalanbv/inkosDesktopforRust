# 970 号 · 翻译域 P4 存储迁移轮：works/&lt;id&gt;/source + work.json + 旧布局自动搬迁 + 跨引擎互读

> 966 号施工图 P4（G9/G12）。此前 Rust 翻译项目存于 `translations/<id>/`，TS 2.0 存于
> `works/<id>/source/` + `works/<id>/work.json`——数据源不相交+manifest 模式互斥，同一
> 项目在「默认 Rust+故障回退 Node」两引擎下互不可见。本轮按 TS 收敛存储。
> 验证：Rust e2e translations70 9/9（+1 新迁移+互读例）+translation lib 28/28+75 确认流
> 例对齐+testgate engine-rs 2103（+1）/src-tauri 584+audit:rust 双 0（自检活体）+clippy
> 双 0+gate:ts 十步全绿+U+FFFD 净。

## 一、存储收敛（project.rs 重写 + run_store 根迁移）

- **create**：布局 = `works/<id>/source/{manifest.json, source|translated/chapter-NNNN.json,
  glossary.json, review-report.md}`（章节/报告/术语内容与 manifest 字段不变，仅根变）；
  同事务写 `works/<id>/work.json`（WorkManifest version=2、profileId="translation"、
  language=目标语、status=active、metadata {sourceLanguage,targetLanguage,sourceOrigin:
  "file"}；artifacts 由写入集构造=TS createCurrentArtifact 同形：id=路径去扩展换 '-'、
  kind=translationArtifactKind 逐字五形（translation-manifest/glossary/review/
  source-chapter/translation-chapter）、path=Work 内相对、checksum sha256:、current 一枚、
  metadata.sourcePath）。
- **run_store**：根函数改 `translation_project_dir = works/<id>/source`（TS 同名）；旧
  `translations/<id>/` 读面**一次性自动搬迁**——manifest 缺席时逐条目 rename 到新根
  （works/<id> 无 work.json 才迁=纯 Rust 时代产物保守判据；work.json 出现即不迁），
  迁后**合成 work.json**（title/language/时间取自 manifest，sourceOrigin="migrated"、
  artifacts 空——TS works/<id> 恒有 work.json，列表 profileId 过滤依赖）；任何一步失败
  回退旧根读不阻断。
- **list**：`translations/` 目录扫描 → `works/` 扫 work.json 且 `profileId=translation`
  （TS `listWorkManifests(root,"translation")` 同面）；迁移过的旧项目自此可列。
- run 快照/报告路径随布局（`works/<id>/source/status.json`、`…/review-report.md`）。

## 二、测试与必红

- lib：create 布局断言改 works+work.json 形（profileId/version/artifacts 数=2×章数+3、
  各 artifact current 修订+sha256）；progress 测试路径改 works；快照 artifacts/报告路径
  断言随迁。
- e2e 新例 `storage_migrates_legacy_layout_and_work_json_carries_artifacts`：①work.json
  形断言（kind 五形在场+current 修订+checksum 前缀）；②人工落旧 `translations/` 布局
  项目（works 下无 work.json）→ 首次详情读触发搬迁（旧目录消失、新根 manifest/
  translated 文件在位）→ 列表两项目（新旧）都可见=**跨引擎互读硬验收**（TS 2.0 建的
  项目=works+work.json，Rust 列/详情/运行全程读同一布局）。
- 75 确认流例路径断言改 works。**必红**：stash 两存储文件 → 恰 3 红（迁移例/全链/
  review 例——旧布局上搬迁断言与 works 路径全炸），pop 后 9/9。
- **修复实录**：首版迁移用整目录 rename → 旧布局内层 `source/` 撞新根 `source/` 目录名
  （文件落到 `source/source/…`，必红抓出）→ 改逐条目搬迁+迁后合成 work.json（列表过滤
  依赖，缺它则迁移项目从列表消失——第二轮必红抓出）。

## 三、真引擎活体（8970，工具协议 mock v3）

- API create → projectDir/manifestPath=`works/<id>/source/…`；盘面 `works/<id>/`=
  work.json+source 七件、`translations/` 不存在；列表 1 项。
- IAB 翻译页「运行翻译」：回执「翻译 2 段，审校 1 章。Skill：inkos-translation。报告：
  works/…/source/review-report.md」、章节卡「2 segments · 1 observations」、标题
  「【译】Chapter 1」——全链（结构化翻译+回喂重试+审校 2.0）在新布局逐值复验。
- 环境清偿：引擎/mock 杀净、临时根删除、IAB 标签关闭、stash 栈 0。

## 四、门禁

- clippy 双 0；testgate engine-rs 2103（+1）/src-tauri 584（INKOS_DUEL=1）+audit:rust
  双 0+必红自检活体；gate:ts 十步全绿（diff 78/0）；U+FFFD 净。

## 五、备案

- 迁移合成 work.json 的 artifacts 为空数组（历史文件不入工件账）——TS 侧工件修订面
  （targetHash 注入）只对后续写入生效；完整回填属 P5 后的工件域工程。
- TS 侧备份域 `works/translations/` 参考已覆盖新布局；无独立迁移脚本需求（读面自动迁）。

**下一号自 971 起；出口=翻译 P5（修订 revise_paragraph+create 三输入+导出对齐）/ C 类
残余备案项；用户侧待办=Fork 推送（含 955-970）。**
