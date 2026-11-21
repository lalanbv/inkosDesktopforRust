# 1649 号：全量审查与真人走查轮——reorder 恒等成员文件丢失（数据丢失级）清偿 + 走查三链活体

> 日期：2026-11-22 ｜ 类型：全量验证轮（1648 新增面独立代码审查 + IAB 真人走查 + 同族面扫 + 全量门禁）
> 定位：对 1640-1648 累积面做全量审查（区别于 1648 逐功能矩阵）——重点审查结构事务族的「staging 收走面 vs 回位面」对齐性，IAB 真人走查逼出 **reorder 恒等成员文件永久丢失**（数据丢失级），修复后浏览器复走红转绿。

## IAB 真人走查（Rust 引擎 4569 + vite dev 4567 代理，五章 fixture）

- **merge 按钮（⇲ 合并到上一章）**：真人点击第 3 章并入第 2 章 → 章清单 [风起, 灵脉(并入云涌), 收束, 归途] 四行重编号正确——**1648 修复的 merge 重编号路径浏览器链活体通过**；引擎侧盘上 index/文件双面核验一致 ✓。
- **move-up 键盘通道**：收束上移 → 新序即时渲染 ✓。
- **导出子集下载链**：取消第 2 章勾选（已选 4/5）→ 点导出下载 → 「已下载」notice、md-zip blob 回流零错误（1648 后端接线后的 UI 全链）✓。
- 走查路由面备考：BookDetail 挂 `#/book/:id/settings`（`#/book/:id`=ChatPage 工作台）；首启语言 dialog 每新 fixture 必现（localStorage 域内）。

## 缺陷清偿：reorder 恒等成员文件永久丢失（数据丢失级，Rust 独有）

- **发现链**：走查第二次 reorder 点击后页面一切超时——装页内 alert 钩子（1646 惯用形）复现取到文案 `Chapter 1 file not found`；盘上取证发现**第一次 reorder [1,3,2,4,5] 后 0001/0004/0005 三个章文件静默消失**而 index 仍 5 行 = index/文件面发散 + 用户正文丢失。此前「卡死」表象 = 错误路径 `alert()` 在 IAB 弹**原生阻塞对话框**（前端无 bug，错误 UX 既有形态）。
- **根因**：reorder 的 `collect_chapter_files(&chapters_dir, &numbers, &index)` 传**全部章号** → staging 收走全部章文件 → rename 循环 `mapping.get(old)` 只命中**变号章**（恒等对被 `continue` 跳过）→ 恒等章滞留 staging → 成功路径 `remove_dir_all(staging)` **永久删除**。split/merge 的 stage 面本就只收受影响章（affected_numbers，:449/:675 实证）——**reorder 是唯一收走面≠回位面的事务**。
- **逃逸面定性**：既有测试全用**全员变号**排列（duel [3,2,1]、lib [3,1,2]、e2e [2,1]）——恒等成员从未被任何测试触达；走查 [1,3,2,4,5]（1/4/5 恒等）第一次踩中。1646 的 staging 审查修的是**失败路径**回滚面，成功路径的「收走/回位不对齐」在当时未爆。
- **修复**：reorder 的 staging 收走面收窄至**变号章**（`affected = renumber.iter().map(from)`，与 split/merge 受影响语义对齐）；恒等章从未离开原位。产物面同步收窄（恒等产物不动，消除原实现的多余 IO 与自改名 no-op）。
- **回归**：Rust lib `reorder_with_identity_members_keeps_unchanged_files`（[1,3,2]：恒等章 0001 原位+变号章交换落位+index 三行+**二次 reorder 可行**——修复前此时 collect 404）+ TS 同款（TS 无 staging 天然免疫，锁行为防回归）——修复前红（恒等文件消失）后绿。
- **浏览器复走红转绿**：修复后真人链两次连续 reorder（第二次=原卡死步）零 alert、序正确往返、盘上五章文件全保留。

## 同族面扫（全量审查）

- **delete_chapter**：显式拒绝删中间章（"Only the latest chapter can be deleted"）只删末章+状态回滚——无重编号路径，排除同族位 ✓。
- **chapter_filter/chapterFilter 接线**：Rust 三处（export_save 解析/GET 解析/build 内建）+ TS 四处（tool schema/server GET/server export-save/core）全闭合——v10 增补轮（ffcfc54d）备案的「两处 None 接线缺口」确认已在 1648 清偿 ✓。
- **split/merge stage 面实证**：curl 引擎直跑 split（恒等章 1 保留、六章齐全）/ merge（恒等章 1/2 保留、五章齐全）——盘面取证排除 ✓。
- **revision 跨引擎不可比**（updatedAt 墙钟）与 **merge UI 不带 expectedRevision**（缺省跳过逐步采纳）维持 1648 备案。

## 门禁记录

- cargo:testgate 全绿：audit:rust 双 0 + 必红自检活体（9/1 两夹具）+ engine-rs **65 目标 2174 passed**（+1=恒等成员回归）+ src-tauri 26 目标 600 = **2774**。
- gate:ts 十腿全绿；fmt/clippy `--all-targets` 双 0（轮内 1 处 rustfmt 漂移清偿）；U+FFFD 两触碰文件净；最终形态复跑 lib 12/12 + e2e books49 10/10。
- change-no 双查=1649 无并行占用。

## 出口

- 全量审查+真人走查收口：结构事务三面的 staging 收走/回位对齐性全绿（split/merge 实证 + reorder 修复）；1648 三缺陷无回归。
- 下号自 1650 起；出口=R50 批二 docx + R51 目标日历 + R52 编辑器 overlay（1638 批四）。
