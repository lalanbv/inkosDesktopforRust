# 906 号 · work 域 UI 整合勘察轮：整合面健康结论 + WorkInspector 交互面补强（5 用例）

> work 域 L0-L3 后端全链收官（M1a/b+M2+M3+M4）后的 UI 整合面勘察：UI 消
> 费端点模板清单化逐条对照引擎路由表=**零缺口**（works 全家桶/skills 列
> 表·trust·import·:id·documents/profiles 集合+:id/episodes 引擎全实
> 现）；mount 冒烟+axe a11y 结构审计覆盖在位；TODO/FIXME 欠账面净。
> 勘察结论=整合面健康；本轮实质增量=WorkInspector 交互面补强（mount
> 冒烟只验渲染，写面交互此前零覆盖）。
> 验证：新交互测试 5/5；studio 全量 760/0；gate:ts 八步全绿（test
> 96.3s）；U+FFFD 净。

## 一、勘察面（对照法）

- UI 端点模板清单化（fetchJson/useApi 模板串提取）：works 域 6 形态+
  skills 5 形态+profiles 2 形态+books 域 30+ 形态。
- 引擎路由表（server/mod.rs）逐条对照：`/api/v1/works` 全家桶（列表/
  :id/artifacts/revisions/adopt）、`/api/v1/skills` 五条（列表/trust/
  import/:skillId/documents）、`/api/v1/profiles` 双条（集合/:id）、
  episodes——**零缺口**（差分活体 72/0 已证行为等价，本勘察证消费面
  连通性）。
- 测试覆盖：WorkInspector 在 page-mount-smoke-2（mount）+a11y-structural
  （axe 结构审计）在位；交互面（写操作流）此前零覆盖=本轮补强点。

## 二、WorkInspector 交互测试（新文件，5 用例）

1. **adopt 语义锁**：打开非当前版本→先比较（adopt 钮渲染门槛=
   comparison 在场，组件有意要求先比较再采用）→adopt POST body 断言
   `{expectedCurrentRevisionId:"r1"}`——冲突检测锚必须是组件当前认知的
   current 而非被采用版本；成功后清选+refetch。
2. **防改历史锁**：非当前版本编辑钮 disabled；当前版本可编辑，保存走
   `/project/artifacts/works/w1/works/w1/story.md` PUT（双 w1=组件
   workId+revision.path 拼接形，按实现断言），body 含
   `expectedRevisionId` 乐观并发锚。
3. **diff 行序锁**：比较当前版本→差异区 `− 当前行\n+ 所选行`（fixture
   两版本仅首行不同，diff 精确单行对）。
4. **错误呈现**：fetch reject → role=alert 呈现且抽屉保留。
5. **episode 弹层**：点击→events 弹层渲染 seq·type→关闭收起。

## 三、测试自坑三条

- `getByText` 精确匹配整节点文本：pre 内「第一行 当前版\n第二行」对
  子串断言不可见——改谓词形态 `(el)=>el.tagName==="PRE"&&includes`。
- adopt 钮渲染门槛：无 comparison 时不渲染（非 bug，有意门槛）——
  测试须先比较再采用，并把该门槛作为断言的一部分。
- mock 路由完备性：beforeEach 只路由 GET revisions，写面用例须自行
  覆盖路由（PUT/POST reject → operationError → refetch 不调=假失败）。

## 四、验证汇总

- 新交互测试 5/5；studio 全量 **760/0**（36.71s）；typecheck 净。
- gate:ts 八步全绿（test 96.3s——threads 三包序列 112.8→112.9→
  106.2→97.5→100.1→96.3s 持续健康）；U+FFFD 净。
- clippy:gate 双 0；cargo:testgate 2044+584（零 Rust 改动按纪律复跑）。

## 五、后续出口

- UI×work 域整合面收官结论：后端端点、消费面、覆盖面三点齐备。
- 出口清空回归常备：上游协议扩展或外部事件（pi1.0 key）或新真实需求
  或下一巡检周期。

**下一号自 907 起；出口清空回归常备；用户侧待办=Fork 推送 40 个提交。**
