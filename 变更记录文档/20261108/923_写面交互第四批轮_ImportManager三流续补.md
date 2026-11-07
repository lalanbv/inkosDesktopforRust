# 923 号 · 写面交互第四批轮：ImportManager 三流续补（fanfic/imitation/canon）——ImportManager 六流全谱完成

> 922 方法学第四批：ImportManager 此前仅 chapters 主路径+spinoff init
> 覆盖（911），本轮续补剩余三流——**fanfic init（fetchJson POST 五字
> 段契约）/imitation init（postApi 四字段契约）/canon book 导入
> （postApi {fromBookId}）**，ImportManager 六流（chapters/canon/
> fanfic/spinoff/imitation+错误三态）全谱完成。
> 验证：ImportManager 6/6（+3）；studio 全量 780/0；typecheck 净；
> gate:ts 十步全绿；U+FFFD 净。

## 一、续补三用例（追加至既有 ImportManager.interaction.test.tsx）

1. **fanfic init 全链**：initialTab="fanfic"→title/正文（placeholder
   定位）→POST /fanfic/init **body 五字段契约**
   `{title, sourceText, mode:"canon", genre:"other", language:"zh"}`
   （mode/genre 取下拉默认首项）→fanficDone 回执+invalidate+nav.toBook。
2. **imitation init 全链**：initialTab="imitation"→title/idea/ref
   三输入→postApi /imitation/init body
   `{title, referenceText, storyIdea, genre, language}` 四字段→
   imitationDone 回执。
3. **canon book 导入**：initialTab="canon"→双 combobox（第一个
   canonFrom 来源/第二个 canonTarget 衍生，位置语义取位）→postApi
   `/books/:id/import/canon` body `{fromBookId}`→中文回执
   「母本导入成功」（tr 真双语 zh 形态——首测误断英文文案）。

## 二、测试自坑两条

- **tr 真双语断言语言形态**：useI18n mock lang="zh" 下回执=中文文案
  （「母本导入成功」），照组件英文串断言必败——断言用双语或正则并
  联。
- fetchJsonMock 路由完备性再证：新流用例必须自带 POST 路由
  （beforeEach 只路由既有流，新流 unrouted reject→假失败）。

## 三、验证汇总

- ImportManager 6/6（chapters×2+spinoff+fanfic+imitation+canon）；
  studio 全量 **780/0**（+3）；typecheck 净；gate:ts 十步全绿；U+FFFD
  净。

## 四、后续出口

- ImportManager 六流全谱+写面交互第三批=写面覆盖终局；出口清空回归
  常备。

**下一号自 924 起；出口清空回归常备；用户侧待办=Fork 推送 56 个提交。**
