# 1007 全量回归审查走查轮——5xx 无 detail 映射空格漂移修复 + 测试样例生产派生化

- **编号**：1007
- **日期**：2026-11-14
- **类型**：fix（全量回归审查走查轮——996-1006 新增面全量测试/代码审查/模拟真人走查收口）
- **范围**：`packages/core`（provider.ts 导出面 + index.ts 导出清单）+ `packages/studio`（error-copy.ts 三处正则 + error-copy.test.ts 样例派生化）；engine-rs / src-tauri / cli 零改动

## ① 门禁电池（改动前基线，全绿）

- gate:ts 全腿拆分实跑：build + typecheck + test + audit:npm + rust-bin + node-fallback-smoke + engine-contract-diff + export-epub-smoke + work-alternation（33/33）+ check-routes（孤儿 0）
- audit:npm（6 白名单 0 拦截 + 四夹具自检活）/ audit:rust（双 crate 0 漏洞 + 7+1 必红自检活）/ clippy:gate（双 crate -D warnings 零告警）
- cargo:testgate：engine-rs 2114 + src-tauri 584 全绿；cargo:ignored-sweep：2/2 + 9/9 挂起用例实跑绿（清单精确匹配）
- U+FFFD 全源码树扫净

## ② 代码审查（996-1006 diff 全量细读 + 「谁解析」双扫）

- 跨端契约腿逐文件审毕：provider.ts 六块 EN 化、write_next_route 广播/快照锚同步、service_routes 双形标记测试、author-report 三形判定、isWriteStoppedMessage 三形、server.ts 双解析点——全合规
- 「谁解析」扫：write-stopped 字面消费面=author-report/isWriteStoppedMessage 双点均已三形；「Upstream details: 」标记消费面=server.ts:2164 + service_routes.rs:1107 双点均已双形；agent_production.rs:788 `(?i)` 分类正则面向 Rust 引擎自有错误文案（TS wrapLLMError 产物不流入该分类面，zh token 属上游中文错误容错保留）——零第四锚定面漏网
- 「谁断言」扫：zh 旧块字面仅存于 error-copy 替换产物与分类正则容错 token，无死锚测试

## ③ 真缺陷（审查抓到，1006 教训自证）

1. **[P1] 5xx 无 detail 映射正则永不命中生产形态**：error-copy.ts 无 detail 分型正则要求 `error)\.\n`，但生产模板 `` `API returned 5xx (upstream service error). ${detail ? … : ""}\n` `` 在无 detail 时产出 **`error). \n`（句点后有空格）**——映射静默失效，zh 用户在 5xx no-body 场景（该分支的最常见触发形态）看到整块 EN 原文。更关键：**手写测试样例犯了同一个漏空格漂移**（自称「与 provider.ts 生产字面同形」实则不同形）→ 测试假绿。恰是 1006 号教训「映射正则活体=生产同形样例（手写样例漂移则静默失效）」在当轮自身面内现形。
2. **[P2] detail 含换行时整块映射失效**：400/5xx 有-detail 分型捕获组 `[^\n]*` 不跨行——上游 body message 理论可含换行（多段校验信息），跨行即整块回落 EN 原文。

## ④ 修复（红绿闭环）

- **样例生产派生化（根治类）**：core `wrapLLMError` 导出（provider.ts + index.ts，行为零改动仅扩公开面）→ error-copy.test.ts 九例样例全部改为 `wrapLLMError(生产触发形态, errorCtx).message` 派生（400 有/无 detail、403、401/429、连接失败、5xx 有/无 detail、409 手打保留并注明理由——core 不产出该消息无法派生）。测试解析走 vitest alias→core src（零重建依赖）；studio typecheck 消费 core dist（gate build 步先行）
- **正则三处**：5xx 无 detail 分型补空格 `\. \n`（生产精确形）；400/5xx 有-detail 捕获组 `([^\n]*)` → `([\s\S]*?)`（惰性 + `\.\nCommon causes:`/`\.\nPossible causes:` 尾锚定，误触面=detail 内嵌完整锚序列的病理形态，可忽略）
- **红绿**：导出前红 9（wrapLLMError 未定义，红相第一层）；导出后恰红 3（5xx 无 detail + 两条跨行——派生样例活体实证）→ 修三正则 → 12/12 绿；另 6 条既有块派生后仍绿=生产字面与映射正则真实同形的首次真值确认（此前只验证过手打同形）
- **NEUTER1007 恰红分形**：5xx 无 detail 正则退回漂移形 → 恰红 1 例（正是回归锚例）→ 互逆恢复 12/12 复绿

## ⑤ 模拟真人走查（IAB + 生产引擎 + fixture，walkthrough-env 一键编排）

引擎新鲜度门禁活体（rust-bin 同源）后拉起：mock:1234 + 引擎:8787 + fixture 书「镜花水月」（write-next 全链实跑，遥测 11 次 0 失败）。四旅程全过：

1. **移动抽屉 a11y（1003）**：390px 视口 → 抽屉标题「书籍信息」+ 关闭钮可访问名称「关闭上下文面板」在位 → 命名钮点击关闭 → 重开钮回归（SR 关闭路径可达实证）
2. **编辑草稿跨卸载恢复（998）**：伏笔池进编辑（textarea 可访问名称定位=1002 补位活体）→ 输入草稿 → 关抽屉（卸载）→ 重开 → 编辑态与内容完整恢复（evaluate 读 property 实证）→ 取消编辑清草稿正常
3. **设置草稿跨页恢复（1001）**：BookDetail 每章字数 3000→3200 → 回首页（卸载）→ 重进设置页 → 3200 恢复；不保存，真源未动
4. **伏笔池筛选跨开合保持（996）**：「悬念」筛选 → 仅身世条目 → 关抽屉重开 → 筛选保持（内容面单条目 + 按钮选中态=HOOK_KIND_COLOR 双实证）

附带冒烟：Dashboard 全渲染（写作数据/遥测/备份/书籍列表/其他创作）、章节表操作钮列齐、EN/zh 切换钮在位。

## ⑥ 门禁终态（修复后复跑）

- core build + 1062 全绿（导出零锚定扰动）/ studio typecheck 双 tsconfig 过 + 全套件 160 文件 884 例全绿（+3 净增）/ cli typecheck + 81 全绿（core dist 下游消费面）
- U+FFFD 四改动文件扫净；engine-rs / src-tauri 零改动（无 Rust 面）

## 备案

1. ArtifactView 内容 fetch 无 stale-response 防护（目标快速连切时旧响应可后到覆写 content）——998 之前即存在的预存类，非本轮引入；修复宜加 effect cleanup ignore 标记，待专项
2. cli zh 展示面对 provider EN 块无承接映射（TuiCopy 无 runtime-message 键）——1006 号「错误协议全 EN 族」政策的既定延伸面（cli 普查轮已定性零修复），非本轮缺陷；如需 zh 承接另立专项
3. book-settings-draft.load 对 status 仅验 string 不验枚举成员——sessionStorage 手改才可触发的防御面轻微缺口，显示层无害
4. 409 CHAT_REQUEST_ALREADY_RUNNING 样例维持手打字面锚（单行短句、生产者 studio server.ts 单点、core 不产出）——派生化不适用

## 教训

1. **「生产同形样例」必须派生不能手打**：手打样例与正则可以同漂成假绿对（本轮 5xx 空格双漂实证）——凡「映射正则 ↔ 生产字面」锚定关系，样例一律从生产函数导出（wrapLLMError 导出的唯一动机）；不可派生的（跨包单点字面）才允许手打并注明理由
2. **审查「谁解析」扫要覆盖分类器/token 正则**：EN 化改字面时，消费面不止显式解析点（server/service_routes），还有宽 token 分类正则（agent_production）——本轮实证其输入面不含 TS 产物故豁免，但该排查步不可省
3. **走查断言读 React 受控 textarea 用 property 非 attribute**（getAttribute("value") 为 null，evaluate 读 .value）
4. **[active] ARIA 标记=焦点非选中态**：跨重挂载验证选中态要读 class/aria-pressed + 内容面双证

## 下一号

自 1008 起；出口回归常备；用户侧待办=Fork 推送（含 955-1007）。
