# 1030 号 · translation 域回归审查 + Parse 穿透普查 + Clean 源头防线

> 1029 出口执行（translation 链的 writer 同型风险扫描——升级态回归审查增量）。

## 一、探针实锤（1.4.7 Value.Parse 额外键行为）

- **默认对象（无 additionalProperties 声明）：额外键保留**（`{a,extra}` 穿透）；
- **additionalProperties:false：Parse 直接抛 ParseError**（额外键 fail-loud 进修复回路）。

即 1.x 下 worker-agent:363 的 `Value.Parse`（非 Decode）不剥键——**所有「无 additionalProperties 声明的 resultTool schema + 下游 zod .strict() 消费」组合=额外键穿透面**（writer settle 的 kind=实锤案例，1020 已投影修复）。

## 二、暴露面普查与系统性修复

submitStructured 全调用点普查（forecast/translation review/reviser/short-fiction/writer）：
- **translation review**：parameters 带 `{additionalProperties:false}`（:20）——1.x Parse 层已拒额外键 ✓ **安全**；
- **reviser**：result 消费=白名单式字段访问（Object.keys(properties)）——额外键无害 ✓；
- **forecast/short-fiction**：消费面各自 schema 校验（flow 测试全绿）✓；
- **writer settle**：1020 投影已修 ✓。

**系统性防线（本号落地）**：worker-agent Parse 后统一 **`Value.Clean`**——探针实锤 Clean 双形态均剥 unknown 键（无声明对象剥 kind/additionalProperties:false 剥 extra）——**源头消除全部 submitStructured 链的 LLM 越权键穿透面**（1020 投影=语义面保留，Clean=源头防线，双防纵深）。

## 三、门禁

typecheck 0 红 + 三包全绿 **core 1067 + studio 893 + cli 81** + gate:ts --fast 三腿绿（完整十腿基线=1029 当日全绿）。Clean 防线验证=探针双形态+全量无回归（投影双防线冗余故无单测锚——防御加固性质记录）。

## 四、下一号自 1031 起

出口=earendil-mz 最终单包合并（远期）/ 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1030）。
