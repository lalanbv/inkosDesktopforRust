# 600 号：TESTING.md 门禁清单补 587 根因修复与探针最小化纪律（文档随批）

- 日期：2026-10-02
- 类型：docs(入库 TESTING.md，零代码)
- 路线：587（gate build 前置）+592/593（探针最小化/布局核验）教训的文档面落地
- 状态：✅ 收口

## 补齐

`packages/studio/TESTING.md`（466/451 号建立的测试惯例文档，入库文件）门禁清单节：

1. gate:ts 行更新——587 号起 **build 前置第 1 步**（typecheck/test 消费新鲜 core dist，core 源改动后无需手动 filter build）；
2. 新增「瞬态假红排查」小节三条：
   - 「源码已对但类型/测试红，隔离复跑绿」=dist 陈旧类假红特征（`--fast`/单跑组成项时先 filter core build 再复跑）；
   - **探针/实测环境最小化纪律**：验证某层行为必须移除同能力 env 兜底（测 Rust 层 3 secrets 兜底须去 `INKOS_LLM_BASE_URL`——592 env 伪象）与非标配置布局（inkos.json services 必须写 `llm.services` 段——589/593 教训）；
   - 门禁窗口禁并行重负载（既有惯例显式化）；
3. engine-contract-diff 行补注扩维（会话面/计量快照面/技能写面周期，581/576/585 号）。

## 下一号自 601 起
