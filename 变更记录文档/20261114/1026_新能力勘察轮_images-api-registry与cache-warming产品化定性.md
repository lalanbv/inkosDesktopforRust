# 1026 号 · 新能力勘察轮——images-api-registry 与 cache-warming 产品化定性

> 1025 出口。升级态（earendil 1.5.2）解锁的新能力产品化勘察（1013 评估形
> 态）——**零代码改动轮**，两能力均定性暂缓（触发条件备案）。

## 一、1025 走查备案复核（误报排除）

books 端点 `chaptersWritten` 「形态变化」复核=**瞬态非缺陷**（write-next 进行中/索引未更新的窗口值；升级态下 `chaptersWritten: 3` 正常在位+targetChapters: 12 ✓）。第 3 章计数正确。

## 二、earendil 1.5.2 新能力勘察

| 能力 | API 面 | 仓内现状 | 定性 |
|---|---|---|---|
| **images-api-registry** | `registerImagesApiProvider/getImagesApiProvider`（ImageApi 可插拔 provider 注册） | cover 生成链**自持**（generateImageFromPrompt+cover-download-origin 900 防线，export-epub 验收） | **暂缓**——触发=需要 earendil 图片 provider 生态（新图片源接入）时评估接入 |
| **cache-warming** | `getPromptCacheTtlMs/getCacheWarmingRequestOptions/isCacheWarmingReplayable`（TTL/idle 续暖/节省阈值 0.05——**LLM 上下文缓存预热的成本优化**） | writer 运行时=mz 0.73.1 线，**无 cache-warming**（dist 探针零命中——earendil 1.5.2 独有） | **暂缓**——触发=mz 线同步该能力，或 writer 链迁移 earendil（=earendil-mz 单包合并远期出口的一部分） |

**共同前提**：真实 API 成本压力数据缺失（mock 走查无成本面）——产品化动机待真实使用数据支撑。

## 三、下一号自 1027 起

出口=earendil-mz 最终单包合并（远期，两能力触发条件的共同前置）/ 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1026）。
