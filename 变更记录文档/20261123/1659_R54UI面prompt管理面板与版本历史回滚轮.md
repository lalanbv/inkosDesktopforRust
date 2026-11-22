# 1659 号：R54 UI 面——PromptPacksPanel（列表/编辑/版本历史/diff/一键回滚）+ IAB 全链活体

> 日期：2026-11-23 ｜ 类型：UI 功能轮（R54 收官；消费 1658 引擎端点）
> 定位：studio prompt 基础面从零起（1658 实证零 UI/端点）；端点真源=Rust 引擎腿（产品主路径 164 号），**TS Node 回退腿缺该族端点→面板降级空态文案**（TS 腿补齐挂下轮，builtin 12 条单源方案=export-bindings TS 绑定生成）。

## PromptPacksPanel（ProjectSettings 挂载，daily-goal 卡后）

- **列表**：3 pack 卡片（title/description+prompt 行，覆盖态「已覆盖」标签；`data-prompt-item` 锚）。
- **编辑**：选中 prompt→内容 textarea（builtin 或覆盖内容）+「保存覆盖」（PUT——regressionHint=prompt-changed→**评测提示 notice**「建议跑一轮行为评测」=v8 接入先测纪律 UI 通道）+「恢复 builtin」（DELETE，ConfirmDialog 两步——1650 壳内 confirm 死路惯例）。
- **版本历史**：GET versions（ts 降序行：action/ts/preview）——空态文案「保存覆盖后开始记录」。
- **diff**：版本 content vs 当前 content→`diffChapterText`（core text-diff 复用=R53 面同形态红绿投影+摘要 `n kept · n − · n +`）。
- **一键回滚**：POST restore（ConfirmDialog 确认 ts/action 文案——「回滚本身也记入历史」）→reload+版本刷新+notice。
- 降级：`data-prompt-packs="unavailable"`（端点不可达=Node 回退腿）。

## IAB 全链活体（Rust 引擎 4569 + vite dev 4567，test-project）

- 面板 ready→选 longform.writer→**builtin 内容入编辑器**（"You are InkOS's long-form chap…"）+无版本空态 ✓。
- 程序化改内容+保存（真 PUT）→**评测提示 notice** ✓+版本行 `put·2026-11-22T16:42:30Z·#…` 出现 ✓。
- diff（版本一 vs 当前=同文）→`3 kept · 0 − · 0 +` 全 kept ✓。
- **回滚链**：存版本二→回滚到最早版→**编辑器=版本一内容** ✓+历史 3 行（put/put/restore 居首）✓+回滚 notice ✓。
- 走查残留清理（覆盖文件+版本账本+空目录）。

## 测试与勘误

- 组件测 5：列表选择/保存 PUT 载荷+评测提示/diff 红绿/回滚 {ts} 载荷+确认弹层/不可达降级。
- **零参调用噪声**：React 19 双 effect/mockReset 时序下 mock 偶发收到 (undefined,undefined)（工厂层无帧=非组件直调；容错吞掉后功能链全绿证实无害）——诊断走守卫+栈回溯+工厂层三层定位（成本超值备案：此类噪声先容错后验证功能链，勿深考古）。
- ready 等待补齐（diff/回滚用例点 item 前未等渲染）+不可达用例 mock 分路（噪声 resolve/真路径 reject）。
- vitest evaluate 内 TS `as` 断言不编译（纯 JS 内核）——去类型语法。

## 门禁记录

- gate:ts 十腿全绿（test 116.0s 含面板 5 测全量）。
- cargo:testgate：本轮零 Rust 变更（fmt --check 无改动）——1658 全绿态保持（1651/1655 先例）。
- U+FFFD 净。

## 出口

- **R54 全量收官**（1658 引擎面+1659 UI 面；TS 回退腿端点+export-bindings 单源挂下轮）。
- 下号自 1660 起；出口=R55 用量预算警戒（先立 per-service 账本：日/月软顶+80% 告警+100% 发送前确认）+观察项巡检。
