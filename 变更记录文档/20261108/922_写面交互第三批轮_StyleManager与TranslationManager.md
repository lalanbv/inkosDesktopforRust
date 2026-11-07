# 922 号 · 写面交互第三批轮：StyleManager（3 用例）+ TranslationManager（3 用例）——DoctorView 只读不补入档

> 910/911 方法学第三批：mount 无 interaction 剩余页面勘察定级——
> TranslationManager（484 行/4 POST）+StyleManager（167 行/1 POST+
> analyze 一流）有写流=补；DoctorView（133 行/0 POST 只读诊断展示）
> 不补入档。本轮覆盖 TranslationManager 列表选中+run 流+错误呈现与
> StyleManager 分析/导入两流+错误呈现。
> 验证：新交互 6/6；studio 全量 777/0（+6）；typecheck 净；gate:ts
> 十步全绿；U+FFFD 净。

## 一、StyleManager（3 用例）

1. **分析流**：空文本守卫（analyze disabled）→POST /style/analyze
   body `{text, sourceName:"sample"}`（sourceName 空时组件默认
   sample）→guide 产物渲染+导入区出现。
2. **导入到书流**：select 选书→postApi `/books/:id/style/import`
   body `{text, sourceName}` →success 回执。
3. **错误呈现**：reject→destructive notice。

## 二、TranslationManager（3 用例）

1. **列表默认选中首项**（`selected ?? translations[0]` 语义）→详情
   自动拉取（manifest 章节渲染，列表+详情双在位断言）。
2. **run 流**：POST /translations/:id/run body `{batchSize:8}`（写死
   批量常量契约）→成功回执。
3. **run 失败**→error status。
- 上传/create 链依赖 File 对象（jsdom 无 DataTransfer）不在此覆盖，
  备案留手动走查（911 同款备案形态）。

## 三、测试自坑两条

- **组件 Nav 形态差异**：TranslationManager 的 Nav 只有 toDashboard
  （无 toBook——toBook 是 ImportManager 的 Nav 形态），测试 nav 参数
  超集传参=TS2353（typecheck/gate 同拦）。
- 同文案多元素（列表卡片+详情面板同名标题/章名）：getAllByText
  长度断言更有信息量（双在位=列表选中+详情拉取同时成立）。

## 四、验证汇总

- 新交互 6/6；studio 全量 **777/0**（+6）；typecheck 净；gate:ts 十步
  全绿；U+FFFD 净。

## 五、后续出口

- 写面交互续补完成度：StyleManager/TranslationManager 入列后剩余
  mount 无 interaction 页面=DoctorView（只读 0 POST，不补）与其他
  展示型页面——写面交互覆盖至此真收官。
- 出口清空回归常备。

**下一号自 923 起；出口清空回归常备；用户侧待办=Fork 推送 55 个提交。**
