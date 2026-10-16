# 736 号：mining 候选全清偿实证——maxTokensCap 已被上游 v2.0 字段删除解决 + 上游第九巡

> 目标线：自主持续开发。41ca307e mining 候选末项「maxTokensCap 语义分离」
> 追溯定案：**已被上游 v2.0.0 彻底解决，无需工作**——mining 清单三项全清。

## 一、maxTokensCap 追溯定案（git 实证）

- **双端现状**：`maxTokensCap` 在 packages（TS）与 engine-rs（Rust）源码
  **零命中**——字段不存在。
- **历史定案**：上游 v2.0.0 提交 07fde1a3
  `feat!(core): drop LLMConfig.maxTokens / maxTokensCap fields (C1+C4)`：
  - 两字段物理删除（BREAKING）；每模型真实 maxOutput 来自 providers bank
    的 InkosModel.maxOutput；
  - 老配置 maxTokens/maxTokensCap 被 zod strip 静默丢弃（兼容不报错）；
  - per-call maxTokens（chatCompletion options）**保留**——语义分离在上游
    决策里即「字段级删除 + per-call 保留」，**本仓两端已同水位继承**。
- **41ca307e mining 候选的语境**：当时某 WIP 分支尚有该字段残留语义的
  审查发现——现该分支未并部分已被上游 master 演进覆盖（v2.0 删除先行），
  无「语义分离」工作需要做。

## 二、mining 清单全清偿（41ca307e 三项）

| mining 候选 | 状态 |
| --- | --- |
| choice-generator/choice-auditor 设计 | choice-generator 已随上游 feature/interactive-fiction 并入（721 实证）；choice-auditor 无独立实现（上游亦未落） |
| ServiceDetailPage 挂载自验 | 735 号清偿（3 测） |
| maxTokensCap 语义分离 | **上游 v2.0 已解决**（本件定案） |

## 三、上游第九巡

fetch upstream 双 master 判据复跑：落后 upstream/master = 0，全部分支零
落后——第九巡维持。

## 四、教训

mining 候选执行前先做**现状回查**（候选记录时点 → 现在之间，上游可能已
自行解决）——「待办」不等于「仍待办」，git 历史追溯三步（grep 现状 →
log 定案提交 → diff 语境）即可定案，避免为已解决问题立项。

## 涉及文件

- 本件（定案归档，零代码改动）
