# 1034 号 · bench 性能复扫轮——升级链（1014-1033）零回归，隔离复跑噪声排除

> 1029 出口的性能维度首验（983 先例形态：治理/升级链后的热路径 bench 对照）。
> **1014-1033 升级链（pi 双栈 0.73.1/1.5.2+typebox 1.4.7+治理三轮）动了 core
> 热路径邻域（agent/schema 面），性能维度此前未验证。**

## 一、bench:gate 结果

**exit=0 门禁通过**——全部基准带内（最大漂移 sensitive_words/scan_chapter
+19.5%）。二十六基准全谱：read_chain -1.8%、sse_broadcast +4.9~13.4%、
story_markdown_render -19.6~+8.0%、title_dedup +1.4~7.3%、tool_registry
-4.0~+5.9% 等——带内波动。

## 二、隔离复跑噪声排除（1012 纪律）

最大漂移项 sensitive_words 隔离复跑（criterion 单基准）：
`scan_chapter_builtin_only` = **-3.1%（p=0.00，快于基线）**——gate 的 +19.5%
=满载负载噪声（bench-gate-load-noise 记忆纪律：负载漂移隔离复跑回基线即噪
声）。scan_chapter 主项同词表同路径同判。

## 三、结论

**升级链（1014-1033）零性能回归**（隔离复跑实锤）——性能维度与代码/测试/
活体/走查四维度并列为升级总账的第五验证维度，全部通过。

## 四、下一号自 1035 起

出口=earendil-mz 最终单包合并（远期）/ 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1034）。
