# 789 号：durable 进度解析容忍降级——index-less legacy 书覆盖第 1 章风险清偿（788 登记项）

日期：2026-11-01。前置：788 号登记项——TS `resolveDurableStoryProgress` index.json 缺失即返回 0，index-less legacy 书上 `getNextChapterNumber`=1，write-next 以「第 1 章」覆盖既有产物（Rust 同场景=文件前缀容忍合并）。

## 一、修复（packages/core/src/state/state-bootstrap.ts）

- **index 缺失/损坏（ENOENT/SyntaxError/非数组/number 非法）→ 容忍降级到文件名 `N_` 前缀连续前缀**——durable 产物链权威=章节文件本身（Rust `load_durable_artifact_chapter_numbers` 镜像）；真 fresh 书（无 chapters 目录）两表皆空 → 0，语义不变。
- **index 有效时保留 2.0 一致性守卫**：index/files 不一致即抛（fail-closed 完整性面，比 Rust 的无守卫合并更严）——**刻意保留的不对称**，写入档注记（防止有效 index 与产物 disagree 时静默写出版本间隙）。
- 实现中途测试矩阵抓到一个真 bug：解析结果赋值守卫笔误（`if (indexNumbers !== null)` 恒假）导致有效 index 恒被丢弃——由「disagree 仍抛」测试现形，修正为 `allValid` 旗标赋值。测试矩阵价值再兑现。

## 二、测试（src/__tests__/durable-progress.test.ts，7 项矩阵）

fresh 0 不变 / **index-less → 文件前缀 2（本轮修复）** / 损坏 index → 文件前缀 / 非连续文件 1,3 → 前缀 1 / valid 一致 → 不变 / **valid 不一致 → 仍抛（守卫保留）** / **写流探针：StateManager.getNextChapterNumber index-less 书返回 3 非 1**（works 布局播种——StateManager.bookDir=`workDirectory(root,id)/source`）。

## 三、消费面影响

- `StateManager.getNextChapterNumber`（TS 写链编号权威）——本轮修复目标 ✓
- `forecast/context-builder`——index-less 书 forecast 上下文从 0 修正为真实进度（同受益）
- 788 号 bootstrap 内部容忍版解析保留（其不抛不一致——ensure 面对半态书须容忍；两者语义分工已注记）

## 四、验证

- vitest **971/971**（964+7，全 flow 套件零回归）+ tsc 归零。
- gate:ts 八步全绿含契约差分 0 分歧（有效 index 书行为不变——修复仅触及缺失/损坏分支）。
- Rust 零触碰。

## 五、教训

1. **测试矩阵的「不变量」组与「变更」组缺一不可**：本轮 7 项里 3 项钉旧行为不变（fresh/valid/守卫），2 项钉新行为，2 项写流证明——正是「守卫保留」组抓出了赋值守卫笔误（新代码把有效 index 也丢成 null，恰被不变量测试现形）。
2. **测试基建要先核被测物的真实布局**：StateManager 的 works 布局（`workDirectory/source`）与裸 bookDir 不同——写流探针第一次就因播种路径错层而假红；探针必须穿过真实装配层而非绕过它。
3. 登记项清偿闭环：788 定界（snapshot 修复本轮+编号解析器登记）→ 789 处置（容忍降级+守卫保留+写流证明）——发散簇按轮拆解的节奏成立。
