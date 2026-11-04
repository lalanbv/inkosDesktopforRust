# 889 号 · 工程轮：M3c 上——生产适配器 + prompts + 三代理 + writer 主干

> 886 设计 §三 M3c 第一轮（M3c 拆 889/890：reviser 新形态链 + writer
> continueDraft/reviseDraft/applyRevisionPlan 留 890——本轮体量上限诚实裁定，
> 886 §三 M3c ≈900-1100 行的弹性分解）。

## 一、交付面

### 生产 ChatTurnFn 适配器（本轮第一件事，888 备案兑现）

`router_chat_turn_fn(router, agent)`：resolve(agent)→client 缓存→streaming_client
**非流式**收集（环轮次短；tool_choice 经 extra 顶层透传；`ChatTurnError.output_limit`
映射 `StreamError::OutputLimit` 新变体——finish_reason=length/max_tokens 的结构化
分流面，Display 文本与原 Protocol 逐字一致故既有断言不动）。`StreamedCompletion`
补 `finish_reason` 字段（五构造点全透传）。model 以 request.model 为准（调用方经
defaults 装配已解析——TS workerModel(client, modelId) 单参换模型同构）。

### utils/source_text.rs（新）

`splitSourceLines`/`sourceLineBodies`/`numberSourceLines` 移植（物理行保留终止符
——scoped 编辑面依赖）。

### models/observation.rs 扩展

ReviewObservationSubmission/ReviewSourceSelection 提交形态 + `resolve_observation_sources`
（REVIEW_SOURCE_RANGE_INVALID/REVIEW_SOURCE_REQUIRED 两码 details JSON 消息面与 TS
同形，nearbyLines 候选邻行）+ `validate_observation_sources`（无引用 REQUIRED/
MISMATCH 引用不在源内，issues 截 16）。**M3a 隐患修正**：ObservationCategory/
Assessment 枚举缺 `rename_all = "lowercase"`（mock 数据触达暴露——落盘面此前未被
外部数据打中）。

### production/short_fiction_prompts.rs（新）

prompts/short-fiction.ts 7 构造函数逐字移植（outline/writer/review×2/package
system+user；measurements JSON **spread 平铺**语义对齐；reference notes/source
分支全保真）。

### production/short_fiction_agents.rs（新，≈1100 行含测试）

- **schema 构造族**：outline 动态 chapter_N_plan 键+title const 面／draft batch/
  chapter/revision chapter/flat batch（openingHook 描述按 hookChars+含章 1 注入）/
  package/revision plan——TypeBox 形状逐字段对齐。
- **outline_create_outline**：rawContent 拼接（planMarkdown+逐章 `## 第N章` 标题段，
  en/zh 双语标题）。
- **packaging_generate**：四字段校验+rawContent 拼接（`# 标题`/`## Intro`/卖点列表/
  `## Cover Prompt`）。
- **reviewer_review_draft**：submitSourcedReview 等价——三条追加 system 消息逐字 +
  record_review_observation supporting（宿主截取回执）+ maxTurns 16 + unknown-codes
  validate + 依最终 codes 顺序 resolve。
- **writer_write_draft 主干**：批次循环（budget 分批）+flat/chapter schema 双形态+
  validateRequestedShortChapter 等价（标题精确/章号面/长度合同两码）+merge+每批
  onBatchComplete。

## 二、测试（5 用例全绿，mock 同 prompt+同响应→同产物字节）

outline golden（rawContent 拼接字节+schema 动态键+system 追加逐字）/packaging
golden（rawContent 四段字节）/writer golden（flat batch→merge→rawContent 与 887
金标准渲染串一致+charCount 重算 21）/reviewer 行区间 resolve（quote=`## 总纲`
1 基含尾截取）/reviewer 变异（终审未记录码→UNRECORDED 回喂→16 轮预算耗尽必红
attempts==16）。

## 三、验证清单

testgate 2038+584 / clippy 双 0 / gate:ts 八步全绿 / agents 5/5 / 纯函数 16/16 /
streaming 24/24 / U+FFFD 干净。测试自坑：json! 数组内联 format! 表达式触发
delimiter 解析问题（预构造标识符绕过）+Edit 手写 `)}),` 丢 object 闭 `}`+
mock 脚本 tool 名必须与各代理 result tool 名精确一致（环按名分发）。
**下一轮 = 890 号 M3c 下：reviser 新形态链（252 行+author-permission 闭包家族）
+writer continueDraft/reviseDraft/applyRevisionPlan 补全。下一号自 890 起；出口
不变；用户侧待办=Fork 推送 23 个提交。**
