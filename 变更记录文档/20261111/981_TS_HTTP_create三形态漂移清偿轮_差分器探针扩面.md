# 981 号 · TS HTTP create 三形态漂移清偿轮：Rust 972 对齐 + 差分器探针扩面

> 常备出口「双栈漂移族扫」选题：勘察 976 镜像疑点（author-report Rust 侧无平
> 行面=闭合）后转 Node 回退端/差分器面——**实证 TS HTTP `POST /translations/
> create` 漂移**：Rust 972 已升三形态（sources/sourceText/glossary+守卫码面），
> TS HTTP 面仍为 filePath 单形态（三键被 body 解构静默忽略）。本轮 TS 单面修
> 复+差分器三形态探针扩面（958 备案「探针须随协议扩展同步」清偿）。
> 门禁：差分器双腿三轮（981 形绿/HEAD 形恰红/恢复形绿）+flow 8/8+gate:ts
> 十步全绿（回填提交信息）；Rust 树零改动（clippy/testgate/audit 引用 979
> 收口——树同则面不变，965 先例）。

## 一、漂移实证（勘察链）

- 976 镜像疑点闭合：author-report 任务报告面=TS 独占消费（Rust 只发射），
  零漂移。
- 差分器探针滞后发现：engine-contract-diff 的 $TRANSLATION 解析器仍用旧三键
  载荷（filePath/sourceLanguage/targetLanguage/title）——975 三输入未进探针面。
- **TS HTTP create 漂移实证**（server.ts:8428）：body 解构仅五键（filePath/
  sourceLanguage/targetLanguage/title/segmentMaxChars），sourceText/sources/
  glossary 键**静默忽略**；无 filePath 即 400 MISSING_FILE_PATH。而 TS 工具链
  完全齐备（TranslationCreateParams 四键+createTranslationProjectFromFile 三
  形态+works/<id>/work.json sourceOrigin 落盘——970 互读基底），缺口收敛为
  HTTP handler 单面。Rust 972 面语义基准（translation_routes.rs:173-300）：
  守卫序=MISSING_FILE_PATH（全缺，70 号契约保留）→CREATION_SOURCE_CONFLICT
  （并给）→MISSING_LANGUAGES；形态判定按键存在性（sources 含空数组）。
- 码面澄清：CREATION_SOURCE_CONFLICT 双栈一致（TS 代理工具 agent-tools.ts
  先例+Rust 972 对齐它；project.ts 的 TRANSLATION_SOURCE_AMBIGUOUS 为另一内
  联层）——勘察中一度误判，逐文件定性排除。

## 二、修复（TS server.ts handler 重写）

- body 类型+三键（sources/sourceText/glossary）；形态守卫序逐字对齐 Rust
  972（MISSING_FILE_PATH→CREATION_SOURCE_CONFLICT→MISSING_LANGUAGES）。
- parameters 透传八键（schema 现有键；sourceName/workId Rust HTTP 面支持但
  TS schema 无此键=备案协议键，探针不触碰）。
- **错误映射 catch**（新增）：creation-source 码面→状态码（守卫码五枚 400/
  工件缺失两枚 404/其余 500 TRANSLATION_CREATE_FAILED 兜底）——对齐 Rust
  creation_source_error 972 映射；此前 TS 面未捕获（非法 filePath 走全局
  500 无码面）。
- **空 sources handler 先拦**（400 REQUIRED）：harness 层 TypeBox 校验
  （minItems:1）会先行 500 包装取不到 REQUIRED 码面——活体探针抓出后补
  （首版只透传未拦，flow 测试 500 vs 400 红实证）。

## 三、测试与探针

- flow 测试新例（studio-api.flow.test.ts，8/8）：inline 创建（200+works/<id>/
  work.json metadata.sourceOrigin=inline+source/glossary.json 术语逐字落盘
  ——baseDir=works/<id>/source 落盘形实测校正两次：metadata 在 work.json
  非 manifest 响应、glossary 在 source/ 子目录）+歧义 400 CONFLICT+空
  sources 400 REQUIRED+全缺 400 MISSING_FILE_PATH。
- 差分器三探针（SHARED_FACE 写面区，958 探针后）：inline 快乐路径（双腿各自
  落 works/<id>，写面只比状态码）/歧义（守卫先于文件访问零副作用）/空 sources
  ——协议扩展与探针面同步（958 备案清偿）。

## 四、必红两形

- **NEUTER981 env 形**：中和三形态判定（sourceText/sources 判定归零回退
  981 前 filePath 单形态守卫链）→flow 新例恰红 1（既有 7 例绿=中和面限定
  正确）；撤除复绿 8/8，实现面 NEUTER981 残留 0（grep 实证）。
- **stash HEAD 形**：stash 三文件→HEAD 差分器 python 重插 inline 探针→
  双腿跑**恰红 node=400 rust=200**（TS 旧单形态拒 sourceText vs Rust 972
  支持——漂移本身被探针抓获）→checkout 丢弃重插→stash pop→复绿三探针全
  ✓（200/400/400 双端对称）；栈 0。

## 五、活体（差分器双腿三轮真跑）

- 981 形双腿真进程活体：TS studio server+Rust inkos-engine-server+mock，
  三探针双端同码（200 inline/400 CONFLICT/400 REQUIRED）+「声明超集键
  node=[kind,workId]」注记=既有响应键差声明口径；695 豁免记账漂移行
  （works/episodes/profiles 等）为迁移进度信号非本轮回归（958-980 各轮
  同面）。

## 六、门禁与备案

- gate:ts 十步全绿（数字回填提交信息）；U+FFFD 净；Rust 树零改动（git
  status 实证）——clippy/testgate/audit 引用 979 收口全绿（树同则面不变）。
- 备案：①sourceName/workId 为 Rust HTTP 独有协议键（TS schema 无，探针不
  触碰，如需走 schema 扩展专项）；②$TRANSLATION 解析器旧三键载荷保留（读
  面锚稳定形态，三形态已由独立探针覆盖）；③registry 坏元素形态（缺 workId
  等）双腿码面未锁（schema 校验面差异，探针避开——负路径专项如需）。

**下一号自 982 起；出口回归常备；用户侧待办=Fork 推送（含 955-981）。**
