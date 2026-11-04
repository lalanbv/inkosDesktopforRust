# 890 号 · 工程轮：M3c 中——author 权限基座（reviser 依赖闭包家族五模块）

> 886 设计 §三 M3c 第二轮（M3c 实测体量远超预估拆 889/890/891 三轮：author-
> permission 闭包家族 510 行 TS + reviser 252 行 + writer 修订链 ≈400 行均超勘察
> 期口径）。本轮落 **author 权限基座**（纯逻辑为主 + select 环），ReviserAgent
> reviseChapter 与 writer continueDraft/reviseDraft/applyRevisionPlan 留 891。

## 一、交付面（production 域五新模块 + 基础件）

### text_range_edits.rs

行区间/选区编辑合同四件：textRangeEditContract（行区间→`range_N_content` 字段+
apply 保留区间外字节）/textSelectionEditContract（唯一目标文本绑定，AMBIGUOUS/
NOT_FOUND 两码）/textScopedSelectionEditContract（选区转 offset+区域内二次定位+
protectedPrefix/Suffix+非重叠守卫）/textOffsetSelectionEditContract（恢复态直接
offset 转合同）。singleLine 片段回喂禁换行（ARTIFACT_EDIT_SHAPE_INVALID）。
RangeEditContract/ScopedSelectionContract 字段 pub（890 权限模块克隆面需要）。

### chapter_document.rs

chapterDocumentHeading/Body/render——剥壳循环 break 条件逐字对齐（行空或章号≠
number），复用新增 `read_chapter_heading`。

### utils/chapter_splitter.rs 增 read_chapter_heading

TS `readChapterHeading` 对应：`#{0,6}` 头正则+中文序数解析（零〇○Ｏ０一二三四五
六七八九十 + 十/百/千/万 单位制，`value*10+digit` 同构）+罗马序数（小前大减）+
en 数字。chapter_document 剥标题壳依赖。

### author_edit_scope.rs

sourceUnits（段落 p1/行 p1.l1 双编址；**段落 text=保留终止符的行连接**——TS 同）+
AuthorTextScope 契约（serde strict）+ authorTextScopeContract（wholeDocument=整篇
行区间/selections 按 unitId 地址化→scoped 合同；inline=行 ID 或行内片段→
singleLine；幽灵 unitId→TARGET_NOT_FOUND）+ mergeAuthorTextScopes（并集不加源字
节+最近似单元映射）+ authorEditPermission。

### source_locator.rs

SourceLocatorAgent.select：LocationSchema 提交（kind/boundary/count/targets）+ 两
段 system 逐字 + EDIT_LOCATION_INVALID/EDIT_TARGET_AUTHORITY_MISSING 校验 + 段落
过滤器（标题/分隔线段排除）+ paragraphs first/last 有界计数展开。

### author_text_permission.rs

narrowAuthorTextPermission（操作区间∩既有权限=intersecting slots；删除槽边界归
属规则逐字；空并集→OPERATION_BOUNDARY）+ matchesProtectedFragments（前缀/后缀
锚定有序 indexOf）+ resolveAuthorTextPermission 纯逻辑面（select 为注入回调；受
保护片段当前文重映射：BASELINE_CONFLICT=受保护文本被改/AMBIGUOUS=多重映射；缓存
文件读写留 891 接线）。

## 二、测试（author_edit_scope 6 用例 + 既有面回归）

sourceUnits 编址金标准（段落 text 含终止符——TS 同）/wholeDocument apply 字节恒
等/行选区 scoped（singleLine=true）/变异三类（幽灵 unitId→TARGET_NOT_FOUND、
wholeDocument+selections 矛盾、行内片段多行回喂→SHAPE_INVALID）/merge 并集。
**测试期望修正×2**：段落 text 与 scoped 替换均**吞行尾终止符**（TS 单元 text 含
\n、replacement 无 \n 则结果无尾换行——首版期望漏 \n 语义已按 TS 修正）。

## 三、验证清单

testgate 2043+584 / clippy 双 0 / gate:ts 八步全绿 / production 68 用例 / U+FFFD
干净。**下一轮 = 891 号 M3c 终：ReviserAgent.reviseChapter（spot-fix 行区间替换
合同+replacementBudget+submitRewrite 长度合同）+writer continueDraft/
reviseDraft/applyRevisionPlan。下一号自 891 起；出口不变；用户侧待办=Fork 推送
24 个提交。**
