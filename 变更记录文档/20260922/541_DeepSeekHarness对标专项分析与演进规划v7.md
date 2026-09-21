# 541 号：DeepSeek Harness 对标专项分析与演进规划 v7（deepseek-ai/deepseek-harness）

日期：2026-09-22
提交：本文件与规划文档同批路径限定提交（纯文档，零代码改动）
类型：docs(规划)，产出 `开发时SpecCoding'sPlan/inkosDesktop/07_产品演进规划/DeepSeekHarness对标专项分析与演进规划v7.md`

## 选题来路

用户指定对标 deepseek-ai/deepseek-harness（dsh，MIT，developer preview，
~232k stars）做对照分析、优化规划与演进设计。与 539 号（Pi 专项，同日落
档）构成同日双外部对标；编号与 R 段避让均按「写档前最后一刻双查」执行——
实际查得 539 号与 R30–R37 已被 Pi 专项占用，本轮顺延取 540 号；
开档后收口前 git log 复核再度发现并行会话 11760bc0（Rust daemon 判定化）
亦标 540 号（其 539 号记录自述「下一号自 541 起」却仍取 540，第三次同日
双会话撞号实证）——按 534/536 号先例改号 541 清偿，R38–R46 段不受影响。

## 调研面（全部当日实测）

- dsh 官方中文文档 12 篇全文精读（architecture/cordis-primer/
  tool-execution-pipeline/agent-lifecycle/defensive-patterns/glossary/
  invariants/skills/token-meter/compaction/spill/slots）+ 6 篇结构扫描
  （session/tools/subagent/core/workflow/ptc-runtime/system-prompt）+
  3 篇生成目录结构扫描（capability-seams/module-graph/tool-catalog）
- 本项目架构只读 Explore 实证：工具分发 5 处 match 点
  （project_tools.rs:448 等）、skills 三件套、server 分层工厂、SSE
  双端事件表、packages 双端逐文件镜像、src-tauri 插件半实现
  （plugin/runtime.rs:3-18 execute 恒 ExecutionFailed 骨架）、studio api 三层

## 头号结论

dsh 的价值在**模式**不在框架：能力 seam 三角色、工具执行三段管线
（pre 守卫→around 超时重试 metrics→post 改写）、会话日志即真相
（「模型可见即已记录」不变量）、token-meter/spill 上下文三件套、
skills 分层注册表、运行时不变式、文档即生成物——全部可在既有包结构
内以 Rust trait/TS 接口同构落地，由差分器扩维守门。**不引入 Cordis、
不拆 53 包**（双端镜像现实下引入第三套框架=移植面再乘一倍）。

本项目三项领先对标确认：双端契约工程、写作领域纵深、Tauri 轻壳。
真缺口集中在：工具面无注册表无机械对照、守卫未归一、文档手写已实证
漂移（plugin-system.md vs plugin/runtime.rs）、调用级遥测有而请求面
计量无、大结果全量内联、会话状态三处散落。

## 规划骨架（R38–R46，详细设计见 v7 文档）

- **P0**：R38 工具注册表单点化（trait Tool+OnceLock 静态表+差分器工具
  面新维度）；R39 三段执行管线（守卫归一/approval 闸位预留/重试债清偿
  =streaming_client.rs:200）；R40 生成式目录三件（工具/端点/SSE）+
  verify 入 gate:ts + next-change-no.mjs 编号工具化（533-536 四连撞号
  机械根治）
- **P1**：R41 token 计量（usage 锚点修正+context-meter 端点+写作链预算
  观测）；R42 工具结果 spill（0700/wx/随机名/env 清洗，尽力而为语义）；
  R43 会话事件日志+deriveMessages 投影（仅聊天/agent 面，写作链持久面
  不动；RunLog/ContextLens/回放 fork 归一同一日志）
- **P2**：R44 skills 注册表升级（rank 显式表/invocation 双布尔/skills
  change→SSE 热刷新）；R45 LLM/subagent 提供方 seam 化（衔接 R30 适配缝
  升级三角色；537 号 daemon 立案载体）；R46 WASM 插件宿主 wit 最小面
  +plugin-system.md 漂移清偿
- **不采纳**：Cordis 本体/53 包/profile-patch/HMR/PTC run_code/agent-team/
  webhook/ACP/LSP/Electron/UI slots；与 v6 不采纳清单合并维护
- **与 R30–R37 零重叠声明**：R32 覆盖聊天面压缩故本轮不单列 compaction；
  R31 传输层守卫 vs R39 工具层守卫层次分界；R33 span schema 容纳 R41
  计量字段；R37 热载复用 R44 change 事件；R45 升级 R30 适配缝为显式 seam
- 0GC 口径沿用 v6 显式化：Rust 热路径 bench:gate 守住即成立；TS 回退端
  不追 0GC；本轮性能落点全在 Rust 面+bench 扩维

## 验收与状态

纯规划轮：无代码改动，无门禁义务。里程碑 M1=R38+R39+R40 一轮收口
（门禁 8 项+差分器工具面 0 分歧+bench 零回退+verify 负样本拦截）；
M2=R41+R42+R43（会话面/工件面差分扩维+崩溃恢复演练）；M3=R44–R46
按需逐项立项。下一号自 542 起。
