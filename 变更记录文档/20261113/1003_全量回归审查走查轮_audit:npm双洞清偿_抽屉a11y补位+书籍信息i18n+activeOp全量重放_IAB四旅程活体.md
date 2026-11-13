# 1003 号：全量回归审查走查轮——audit:npm 双洞清偿 + 抽屉 a11y/i18n 补位 + activeOp 全量重放 + IAB 四旅程活体

日期：2026-11-13。开工三查净（HEAD=c16a6c95、工作区空、当日变更记录目录最大号 1002 无并行撞号）。

目标：全量测试 996-1002 七轮新增代码逻辑 + 代码审查 + 模拟真人测试 + 修复不引入新问题。

## ① 全量门禁电池活体：抓到 audit:npm 真红 2 条

`pnpm gate:ts` 十步电池活体：build/typecheck/test/rust-bin/node-fallback-smoke/engine-contract-diff（双栈差分器双腿真跑，987 装配宏后契约面仍无漂移）/export-epub-smoke/work-alternation/check-routes 九步全绿；**audit:npm ✗ 拦截 2 条新增未白名单告警**（855/854 纪律下门禁真牙齿）：

- `basic-ftp` GHSA-5pm8-86m8-9m6r（moderate，MLSD `__proto__` fact 名 Uncaught TypeError 致 `Client.list()` DoS；漏洞区间 <=6.2.3，**补丁线 >=6.2.4 同 major**）；
- `@hono/node-server` GHSA-jpvv-r467-5p4w（moderate，请求路径反斜杠按路径分隔符解析绕过反代路径 ACL；漏洞区间 <1.19.18，**补丁线 >=1.19.18 同 major**）。

定性：basic-ftp 为 override 钉定传递链（get-uri→pac-proxy-agent，源码零引用）；@hono/node-server 已有 759 号钉定条目（GHSA-rmxm-3fg6-px4f，2.1.3 跨 major 语义不变），新 id 为同模块新告警。两洞均有同 major 补丁线 → **修洞优先于白名单钉定**，不需要新增 whitelist 条目。

## ② 代码审查：996-1002 新增面逐文件审，真问题 3 个

审查面 = 两个草稿模块（artifact-edit-draft / book-settings-draft）、BookDetail 接线、ArtifactView 接线、ProgressSection（999 修复形）、PendingHooksView（996）、ContextDock、BookSidebar 全文。草稿模块双卫兵（typeof+try/catch+字段形状校验）与保存/取消语义全部自洽；999/996 修复形干净。真问题 3 个：

1. **移动端抽屉关闭钮无可访问名称**（BookSidebar.tsx，873/1002 同族 critical）——1002 备案①「SR 用户走关闭钮路径」的前提不成立：该钮自身纯图标无名（备案复核转正先例=档案不回改，本号即修正记录）。同文件 1002 域核查遗漏面。
2. **抽屉标题「书籍信息」硬编码中文**（1831eaa3 拆分初始即如此，非 t() 键）——EN/JA 用户看中文，i18n 漏网。
3. **PanelView.activeOp「只看最后一条」增量形**（999 同族漏网）——PanelView 现仅存活于移动端抽屉（`{open && ...}` 条件挂载），抽屉开合卸载期间 store 累积的中间事件被跳过：write:start 落进 log 等未映射类型后重开抽屉，「正在写作中」指示丢失。999 普查口径=ContextDock 面板注册表四区，未含移动抽屉域。

走查另发现 DirectorPanel「阶段 · 已存章节」疑似 U+FFFD——源码字节核验为 `·`（U+00B7）正常、全仓 EF BF BD 扫描零命中，定性为快照管道显示伪影非落盘污染（证伪记录在案）。

## ③ 修复与红绿闭环

**audit 双洞清偿**：override 升级 `basic-ftp ^6.2.1→^6.2.4`（解析 6.2.6）+ `@hono/node-server ^1.19.15→^1.19.18`（解析 1.19.19），lockfile 刷新；复跑 audit:npm **8→6 advisory、拦截 0**、四夹具必红自检全活。759 号钉定条目保留（旧洞 2.x 补丁线语义未变）。

**FIX A/B（抽屉 a11y + i18n）**：关闭钮补 `aria-label={t("nav.closeContextPanel")}`；标题改 `t("nav.mobilePanelTitle")`；字典注册 2 key（双语）。**NEUTER1003-a11y 必活体**=摘名（aria-label={undefined}）恰红 2/2（`Unable to find an accessible element with role button and name...`）→互逆恢复复绿。

**FIX C（activeOp 全量重放）**：新纯函数 `deriveActiveOp`（export 供测试）逐事件重放幂等——挂载中与旧增量形逐事件语义等价（未映射事件不动累加器），卸载期事件不丢；组件删增量 useState+useEffect 改一行派生调用。测试 7 例（`BookSidebar.activeOp.test.ts`，node 零 mock）：卸载期累积主场景/audit 累积/revise 单事件/write:complete 收敛/tool:end 清空/idle/幂等。**NEUTER1003 必活体**=`messages.slice(-1)` 摘全量重放恰红 3 例（主场景/audit/幂等——全依赖多消息累积）4 绿（单事件/终态恒真）→互逆恢复 12/12 复绿（含既有交互 3 例回归）。

**守卫测试**：`BookSidebar.drawer.test.tsx`（jsdom，t 桩=键名哨兵 `[key]` 形态）——一次拦三种退化：按钮退化无 aria-label（查询失败红）/标题回退硬编码中文（哨兵缺席红）/t 接线脱落回落键名字面量（同形缺席红）；含 SR 关闭路径可达例。分支走 artifact（复用 998 实证 truth mock 形态）——抽屉头/关闭钮两分支同构，panel 分支派生逻辑由纯函数测试覆盖。

## ④ 模拟真人测试（IAB 生产引擎走查，1001 备案触发项清偿）

walkthrough-env 三件套（mock 1234 + 引擎 8787 + fixture 镜花水月）+ 浏览器真人模拟，四旅程全过：

1. **998 编辑草稿**：开「世界观设定」→编辑→输入未保存标记→关 dock→Cmd+Shift+D 重开（合成 KeyboardEvent，956 惯例）→textarea+编辑态+草稿全文完整恢复；
2. **1001 设置草稿**：目标章数 12→20→切页卸载→回设置页→字段恢复 20（未点保存真源仍 12，覆盖层语义正确）——1001 备案「IAB 走查若复现再立集成专项」的走查承诺兑现，未复现缺陷，修复面活体证实；
3. **1003 移动抽屉**：390px 视口→抽屉钮开→关闭钮可访问名称在位（修复前查无此钮）→标题渲染→点关闭收起（SR 路径可达）→切 EN→"Book info"/"Close context panel" 真接线、中文残留 0；
4. **996 伏笔池筛选**：点「悬念」→仅 1 条悬念卡→关 dock→重开→筛选保持（他类卡不回流）。

终态还原：筛选回「全部」、设置草稿清、 ArtifactView 草稿取消清、视口复原；走查进程已清场。

## ⑤ 门禁终态

studio 全套件 **158 文件 868 例全绿**（+9=activeOp 7+抽屉 2）；typecheck 双 tsconfig 过；audit:npm 拦截 0；audit:rust 双 crate 0 漏洞+必红夹具 7+1 自检活；U+FFFD 净；Rust 树零改动（引用 1000 定桩 cargo:testgate 2113+584 与本轮 gate:ts rust-bin/engine-contract-diff 活体腿）。提交路径限定七文件：package.json+lockfile+组件+i18n 字典+两新测试+本文档。

## 备案

1. BookSidebar 移动端遮罩 div 维持 1002 备案不设名（非语义按钮）；其前置「关闭钮路径」自本轮起真实成立（该钮已名）。
2. 条件挂载族第四面收口：996 筛选态/998 编辑草稿/999 进度态/1003 activeOp 指示——移动抽屉域并入族判据，后续新增抽屉面板组件按本形自查。
3. activeOp 重放 O(n) 同 999 备案（千条级再议窗口化）；deriveProgressState 已有同形先例无新增触发。
4. i18n 键名哨兵 t 桩形态（`[key]`）可供后续 stub-t 守卫测试复用——比硬编码断言多拦「接线脱落」一档。
5. 遗留 i18n 硬编码普查仅在本轮触碰域内做（抽屉头）；全站 zh 硬编码面未普查（873 曾扫文本样式面，i18n 面未专项），如需另立专项轮。

下一号自 1004 起；出口回归常备；用户侧待办 = Fork 推送（含 955-1003）。
