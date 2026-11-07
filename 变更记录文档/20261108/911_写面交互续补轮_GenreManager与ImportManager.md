# 911 号 · 写面交互续补轮：GenreManager CRUD（4 用例）+ ImportManager 导入流（3 用例）

> 910 方法学续补：写面页面有 mount 无 interaction 的最后两页——
> GenreManager（464 行 CRUD：create/edit/delete/copy）与 ImportManager
> （400 行导入流：chapters/canon/fanfic/spinoff/imitation/init 六面）。
> 本轮覆盖 GenreManager 全 CRUD+copy 与 ImportManager 主路径两面
> （chapters 成功/错误+spinoff init 全链），写面交互收尾真完成。
> 验证：新交互 7/7；studio 全量 771/0（+7）；typecheck 净；gate:ts
> 八步全绿；U+FFFD 净。

## 一、GenreManager（CRUD+copy，4 用例）

1. **create body 拆数组锁**：表单 chapterTypes「开局, 冲突, 收束」→
   POST /genres/create body `chapterTypes:["开局","冲突","收束"]`、
   `fatigueWords:[]`（parseCommaSeparated 拆逗号+trim+滤空语义）——
   与引擎 genres 种子契约同构。成功后关表单+refetch+零 alert。
2. **edit 包裹形锁**：PUT /genres/:id body=`{profile:{...表单字段},
   body:"规则正文"}`——表单字段进 profile 子对象、正文独立同级
   （引擎 PUT 语义镜像）。
3. **delete 确认链**：详情面板删除钮→ConfirmDialog（确认钮与面板钮
   同名 common.delete——`getAllByRole(...).at(-1)` 取对话框确认）→
   DELETE→refetch。
4. **copy alert 回执**：POST /genres/:id/copy→`window.alert` 呈现
   「Copied g1 to project genres/」（stub 断言）。

## 二、ImportManager（chapters 主路径+spinoff 全链，3 用例）

1. **chapters 成功流**：select（aria import.selectTarget）+正文
   （placeholder 定位）→POST /books/:id/import/chapters body
   `{text, splitRegex:undefined}`（空正则不传键=undefined 语义）→
   success 三态「Imported 3 chapters」（emerald 形态）。
2. **错误三态**：reject→destructive 形态+消息透传。
3. **spinoff init 全链**：postApi /spinoff/init body（title/parent/
   direction）→`waitForStudioBookReady` 就绪轮询（**全局 fetch stub**
   返回即就绪形态）→invalidateApiPaths+**nav.toBook("nb1") 导航**→
   spinoffDone 回执。非 main 路由首次被交互测试锁定（书创建→就绪→
   导航完整用户旅程的 API 面）。

## 三、测试自坑三条

- label 与 input 无 htmlFor/id 配对——`getByLabelText` 不可用，改
  「label 元素 nextElementSibling」定位（getAllByText 找 LABEL 形态）。
- 同名双钮族：详情面板钮与对话框确认钮同名（common.delete）、tab 栏
  钮与 tab 内容提交钮同名（import.chapters/spinoff）——统一
  `getAllByRole(...).at(-1)/[0]` 位置语义取位。
- postApi 单参调用经 mock 代理显式传 `undefined` 第二参——
  `toHaveBeenCalledWith` 严格 arity 不匹配，改 `mock.calls.some` 断言。

## 四、验证汇总

- 新交互 7/7（GenreManager 4+ImportManager 3）；studio 全量 **771/0**
  （+7）；typecheck 净；gate:ts 八步全绿；U+FFFD 净。
- clippy/testgate 零 Rust 改动，同日 907/909 连续复跑绿维持。

## 五、后续出口

- 写面交互收尾真完成：mount 全页+interaction（4 书写面页+WorkInspector
  +编辑器）+state（ChatPage）+a11y 全矩阵。出口清空回归常备。

**下一号自 912 起；出口清空回归常备；用户侧待办=Fork 推送 45 个提交。**
