# 875 号：工程轮——work 实体夹具播种 + work/work-chat 子流程页 axe 审计（critical×2 清零）

日期：2026-11-03。性质：**实质工程轮**（873 号十八页面集最后未覆盖面：work/work-chat 子流程页）。

## 一、前置勘察：work 域只活在 TS 腿

- 前端 WorkInspector 取 `GET /works/:id`（work.json + harness.sqlite 的 episodes），版本抽屉取 `/works/:id/artifacts/:aid/revisions/:rid`（**服务端校验 sha256 checksum**，不符 409）
- Rust 引擎**整个 works/episodes/profiles 域 404**——engine-contract-diff 早已登记为「上游新增域：TS 有/Rust 404=设计内声明」（695 记账），Dashboard 的 Rust 腿降级 712 号已处理（works 区不短路首页）
- 推论：work 页走查/审计必须 `walkthrough-env --engine node`

## 二、夹具播种（scripts/walkthrough-fixture.mjs §2.9）

**根因级发现**：fixture 在 2.0 TS 腿（`!forkContract`）与报告模式两处于 §3.6 实体播种**之前** `process.exit(0)`——实体播种从来只在 Rust 全链路径执行，而 `/works` 恰恰只活在 TS 腿。首版把播种块写在 §3.7 → TS 腿根本走不到（活体实证：`/works` 只见迁移出的 `镜花水月`）。**修正=函数化 `seedWorkEntity()` 置于腿分支之前**，两腿都播种（Rust 腿文件无害、引擎不读）。

播种内容：`works/walk-diff/work.json`（profile=short-fiction 内置、`outline` 生成物 **r1 superseded + r2 current 双版本**——覆盖版本选择/比较/采用面；checksum/byteLength 按真实内容计算）+ 两版本文件 + `harness.sqlite` 一条 episode（2 事件）。episode 走 **core dist 的 CreativeEpisodeStore**（与 Node 回退端同实现同 schema，不复刻表结构）；dist 缺失时告警跳过（不致命）。幂等：`getEpisode` 存在即跳过（复用根二次运行实证通过）。断言：TS 腿 `/works` 不见 walk-diff 即 `die`；Rust 腿 404 只提示。

**顺带修正首版潜伏 bug**：首版 `create({status:"completed"})` 后再 `finish()`——`finish` 只更新 `status='running'` 行，必抛（因首版从未执行而未现形）；改为 running→finish。

## 三、axe 审计（Node 端 8788，新 origin=默认 V1 壳）

**覆盖面意外收获**：8788 是新 origin，localStorage 空——**V1 整栏 Sidebar 才是新用户默认壳**（navLayoutV2 仅显式 "true" 启用），而 872/873 审计都在 870 走查开过 V2 的 8787 上进行，**默认壳此前从未被审**。本轮实测：V1 壳首页（含 works 区「雨夜碎镜」卡）零违规。

七个交互态逐一审计（DOM 级 el.click 驱动、受控 select 用原生 setter+change 事件）：

| 态 | 首轮 | 修复 |
|---|---|---|
| WorkInspector 主页 | 0 | — |
| 版本抽屉（当前版本） | **button-name(critical)** + landmark-unique | L217 纯图标 X 关闭钮 → `aria-label` 关闭预览；抽屉 `<aside>` 与侧栏 aside 撞成两个无名 complementary 地标 → `aria-label` 版本预览 |
| 抽屉编辑态 | **label(critical)** | L221 编辑 textarea → `aria-label` 编辑版本内容 |
| 抽屉比较/采用面（选 r1） | 同上两条（无新增） | — |
| 执行记录事件层 | 0 | — |
| 创作配置展开（配置编辑+方法文档） | 0（上游已有 aria-label） | — |
| work-chat（空会话） | **color-contrast(serious)** 4.15 | ChatPage 空态引导与玩法选择提示 `muted-foreground/70` → `foreground/70`（共享组件空态——新会话才渲染，872/873 审的书页有消息故漏网） |

终态（新鲜 bundle 七态复测）：**全部零违规**（除全站恒定备案 region×2）。`/70` 族全仓 43 处但 872/873 审过的页面未报（对比度依赖底色与字号）——**只修实测命中，不做无证据全量扫除**。

## 四、守卫固化 + 变异证明

`a11y-structural.test.tsx` 追加 WorkInspector 抽屉用例（mock `fetchJson` 返回版本载荷，user-event 打开抽屉→axe→点编辑→axe，并直接断言抽屉 aria-label——landmark-unique 需侧栏并存才触发，孤立组件改断言属性）。**逐项变异**：去 X 钮名称 → 精确红 `button-name(critical)x1`；去 textarea 名称 → 精确红 `label(critical)x1`；去 aside 名称 → 红 aria-label 断言；恢复后三处在位计数=3、6/6 复绿。

## 五、方法学新坑：Node 回退端缓存 index.html

dist 重建后 reload，页面 `#root` 完全为空——**服务端引用旧哈希 bundle 404**。根因 `server.ts:8799` 启动期一次性 `readFile` 缓存 `index.html`，静态资产却每请求读盘。**生产无影响**（桌面包内 dist 运行期不可变）；走查 SOP 补充：**Node 端重建 dist 须重启后端**（Rust 引擎每请求读盘无此问题）。

## 六、验证

| 面 | 结果 |
|---|---|
| studio vitest | ✓ **755 passed（141 文件）**（754+1） |
| `gate:ts` 八步 | ✓ 全绿 |
| engine-contract-diff 独立运行 | ✓ 共享面 71/1（唯一分歧=695 记账 quality-trend 既有）/ fork-only 4/4 / 上游新增 9/9——与基线一致；**夹具在报告模式失败会被吞**（stdout ignore + 「部分失败（预期内）」继续），故单独核验：吞错行与 stderr 告警均未出现=两腿新播种块真实执行成功 |
| 夹具幂等 | ✓ 复用根二次运行 `work 播种就绪` |

## 七、范围备案

- 版本抽屉为手写模态层（fixed inset+点背景关闭），未实现 WAI-ARIA dialog 全模式（初始焦点/焦点陷阱/Esc 关闭/焦点归还）——本轮取最小语义修复（命名），不贸然加 `aria-modal`（无焦点管理时反而让读屏用户失联）；完整 dialog 模式待真实键盘/读屏用户反馈
- Rust 腿 works 域迁移：695 上游新增未迁域，规模大，按既有记账维持

**下一号自 876 起；出口不变；用户侧待办=Fork 推送 7 个提交。**
