# 982 号 · registry 坏元素负路径锁定轮：双腿 404 同码（981 备案清偿）

> 981 备案清偿（「registry 坏元素形态双腿码面未锁——schema 校验面差异」）。
> 漂移实证：Rust 面 serde 契约（deny_unknown_fields+camelCase 三键）拒坏元素
> →WORK_RESOURCE_ID_INVALID **404**；TS 面 harness TypeBox 校验拒→**500**
> TRANSLATION_CREATE_FAILED 兜底——码面与状态码双漂移。本轮 TS handler 逐
> 元素拦截面（对齐 Rust 语义）+双端用例锁定+差分器负路径探针。
> 门禁：差分器双腿五探针 200/400/400/404（+981 三形态）双端同码+flow 8/8+
> Rust e2e 三形态用例绿+clippy 双 0/testgate/gate:ts（数字回填提交信息）。

## 一、漂移实证（勘察链）

- Rust 契约（creation_source.rs:22-29）：`CreationArtifactReference` serde
  `deny_unknown_fields`+`rename_all=camelCase`——workId/artifactId 必需
  string、revisionId 可选 string、**多余键即拒**；坏元素经
  `serde_json::from_value` 失败→`WORK_RESOURCE_ID_INVALID`→
  creation_source_error（972 映射）→**404**。
- TS 面：harness 层 TypeBox 校验（CreationSourceReferences minItems:1+
  CreationSourceReference additionalProperties:false）拒坏元素→异常包装
  （"params decode failed"）→handler catch 兜底 **500**——码面丢失。
- 空串键语义双端一致：serde String 无长度校验放行→双腿同落工件查找面
  （ENOENT→SOURCE_NOT_TEXT 400）——TS 拦截面同语义放行。

## 二、修复与测试

- **TS handler 逐元素拦截面**（server.ts create，981 空数组守卫后）：非对象/
  缺 workId/缺 artifactId/revisionId 在场非串/含未知键→404
  WORK_RESOURCE_ID_INVALID（"invalid source reference"）。
- **Rust e2e 三形态用例**（create_three_source_forms_with_lineage_and_glossary
  内，空 sources 用例后）：缺 artifactId/多余键 extra/revisionId 非串→三断言
  404+码面（单跑 0.20s 绿）。
- **TS flow 用例扩**（既有 981 新例内循环三形态）：同三形态→404+码面。
- **差分器负路径探针**（第五探针）：缺 artifactId 坏元素→双腿 404 同码锁定。

## 三、必红两形

- **NEUTER982 env 形**：中和拦截面（循环源归零空数组）→flow 坏元素用例
  恰红 1（既有 7 例绿）；撤除复绿 8/8，NEUTER982 残留 0（grep 实证）。
- **stash HEAD 形**：stash 四文件→HEAD flow 重插坏元素循环（含探针
  console）→**恰红 HEAD-FORM-BAD 500**（TypeBox schema 500 包装≠404——
  漂移本身被抓）→checkout 丢弃重插→stash pop→复绿 8/8；栈 0。

## 四、活体（差分器双腿真跑）

- 五探针双端同码：inline 200/歧义 400/空 sources 400/**坏元素 404**——
  981 三形态+982 负路径全锁定（exit=0）。

## 五、门禁

- clippy:gate 双 0（e2e 用例零 lint）；cargo:testgate/gate:ts/audit:rust
  （数字回填提交信息）；U+FFFD 净。

## 六、备案

- 981 备案面 1（sourceName/workId 协议键专项）定性：双端 HTTP 均无消费者
  （TS UI/代理面均不传；Rust 面支持但无人用）——API 完整性面低收益，
  维持备案不专项（触发条件=上游协议扩展或新真实需求）。
- 空串 workId/artifactId 负路径双腿同落工件查找面（未单独锁——语义同源，
  探针边际价值低）。

**下一号自 983 起；出口回归常备；用户侧待办=Fork 推送（含 955-982）。**
