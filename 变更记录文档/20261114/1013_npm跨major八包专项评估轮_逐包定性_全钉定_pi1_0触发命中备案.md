# 1013 号 · npm 跨 major 八包专项评估轮：逐包定性，全钉定 + pi1.0 触发命中备案

> 1012 出口清偿（「npm 跨 major 8 包专项评估（978 形态逐包定性）」）。本轮=评估
> 定性轮，**零代码改动**（package.json/lockfile/源码全不动——1012 已落 undici
> exact 6.29.1 钉定）。口径=三包 package.json 直接依赖 × 现版本 × npm latest，
> major 不同（或 0.x minor 大跳）即跨界。调研面=用面勘察（grep 密度）+ 上游
> release notes（undici/zod/vite/pi/unpdf/lucide 六路）。结论：**全部钉定**，
> pi 三件触发条件**已命中形态**转专项备案（933 号出口「pi1.0 key」兑现）。

## 一、逐包定性台账（决策=全部钉定）

| 包 | 现版→latest | 用面 | 上游 breaking 核心 | 决策与触发条件 |
|---|---|---|---|---|
| **undici** | 6.29.1(exact)→8.12.4 | **单站** ProxyAgent（proxy-fetch.ts，278 号安全钉定） | v7/v8 大版本 breaking 未系统化文档；**v6.29.1 已 backport v8.12.x 同批安全修复**（TLS renegotiation DoS 等）=安全等价 | 钉定 exact。触发：ProxyAgent API break 或 6.x EOL/新安全事件 |
| **vite** | ^6.4.6→8.3.8 | dev 构建链（vite.config.ts+插件生态） | **Rolldown/Oxc 全量替换 esbuild/Rollup**（rolldownOptions/manualChunks/commonjsOptions no-op/CJS interop 统一）；**Node 22.12+ 要求 vs 桌面壳 node_min=22.0.0 硬冲突** | 钉定。触发：桌面壳 node_min 抬升专项落地+插件生态兼容确认（二条件同时） |
| **zod** | ^3.25.76→4.6.7 | **42 文件 schema 底座** | 19 处 `z.record(`（单参→双参**破坏性**）+157 处 `.strict()`（deprecated 非移除）+5 处 addIssue；errorMap/.merge/nativeEnum 等其余敏感面 **0 命中** | 钉定。触发：安全压力或需 zod4 能力（treeifyError 等）→中低成本专项（社区 codemod+19 record 机械改） |
| **unpdf** | ^1.9.1→2.1.0 | 3 文件 PDF 提取（materials/ingest+translation/source） | **Node 24+ 要求 vs node_min=22 硬冲突** | 钉定。触发：node_min 24 专项（与 vite 触发同族联动） |
| **pi 三件** @earendil-works | exact 0.87.1→1.5.2 | **agent 运行时核心**（pi-dispatch/forecast-tools/skill-tool） | **pi 1.0.0 已约一月前落地**（1.0→1.5.2 活跃迭代）；可见 breaking=shouldStopAfterTurn 移除→finishTurn hook；维护者含 badlogic（=mariozechner 本人，**org 迁移即两套 pi 包并存根因**） | 钉定 exact。**触发条件已命中形态**（933 出口备案「上游协议扩展/pi1.0」）→**pi 1.5 升级专项立项为下一 backlog 候选**（agent 全链+@mariozechner 0.67.1 双栈遗产处置一体） |
| **lucide-react** | ^0.577.0→1.72.0 | 59 文件纯展示图标 | 1.x 持续图标改名（1.68 \*-warning→\*-exclamation-point、1.71 circle-arrow-\* 族）；**repo 现用 AlertTriangle 在改名族** | 钉定。触发：需要新图标/现用图标被删→59 文件图标名清理（TS 编译期可拦=机械） |

**共同前提**：audit:npm 6 advisory 全白名单（1012 复验）——八包均无安全紧迫性；
升级唯一正当动机（安全清偿）不存在，978「不盲推」纪律落定为全钉定。

## 二、pi 专项备案（评估轮唯一触发命中）

- **背景链**：832 号段上游 2.0 合并双栈定线（TS=2.0 基底）→ pi 栈两套并存
  （@mariozechner 0.67.1 旧线 + @earendil-works 0.87.1 fork 线）；933 号出口
  备案「常备出口=上游协议扩展/外部事件 pi1.0 key」。
- **现状**：上游 earendil-works/pi 已发 1.0.0→1.5.2（约一月内），活跃维护，
  breaking 面可见一条（hook 形态）。触发条件命中，但升级=agent 运行时核心
  （dispatch/agent loop/工具钩子全链）+双栈合并处置——**专项体量，非评估轮
  内执行**。
- **立此备案**：pi 1.5 升级专项进入 backlog 候选（1014+），执行形态=先单轮
  契约勘察（1.5.2 API 面与 0.87.1 diff+用面影响清单）再决策升级/分步。

## 三、门禁（零代码改动轮）

gate:ts 十腿全绿（1012 刷新后基线延续）+ 工作树净（本轮仅新增本文档）。备案
复查：undici exact 6.29.1 钉定在位（1012 落定）✓。

## 四、下一号自 1014 起

出口=**pi 1.5 升级专项·契约勘察轮（backlog 候选）**/ 用户侧待办=Fork 推送（含 955-1013）。
