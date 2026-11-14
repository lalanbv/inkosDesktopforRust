# 1031 号 · Clean 源头防线回归测试轮——Parse→Clean 管道契约锁定

> 1030 备案的「Clean 无单测锚」增量补齐——**Parse→Clean 管道契约例**（库行
> 为前提锁定）。core 1068/1068（+1）。

## 一、测试（+1）

`tool-arguments.test.ts` 增「Parse→Clean 管道：额外键在 Clean 步剥除」——锁定 1030 防线的**库行为前提**：1.x 的 `Value.Parse` 保留未知键（穿透形态实锤——下游 zod strict 暴露面的机制）+ `Value.Clean` 按 schema 剥除（防线步骤语义）。**双版本行为一致**（0.34/1.4.7 探针与测试同证）。

## 二、NEUTER 适用性如实记录

本测试锚定的是**库行为契约前提**（Parse 保留/Clean 剥除）而非产品分支——NEUTER（摘产品 Clean 调用）不可红：writer settle 投影为**双防线冗余**覆盖 kind 场景（1030 设计）。产品 Clean 调用的回归保护=全量套件+投影双防线，管道契约例的价值=前提漂移报警（typebox 未来版本若改 Parse/Clean 语义，本例红=升级审查触发点）。

## 三、门禁

core 1068/1068（+1）+ typecheck 0 红。

## 四、下一号自 1032 起

出口=earendil-mz 最终单包合并（远期）/ 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1031）。
