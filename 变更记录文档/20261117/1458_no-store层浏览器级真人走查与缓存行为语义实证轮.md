# 1458 号：no-store 层浏览器级真人走查与缓存行为语义实证轮

## 目标与范围

/goal 逐功能测试新增代码逻辑+代码审查+模拟真人测试。验证对象=1455/1456
两轮落地的 TS 腿 no-store 统一层（API 中间件+SPA 入口+摘键序修复）的
**消费面行为语义**——前两轮验收止于 curl 级头断言与 dispatch 层单测，
浏览器真链路（no-store 的最终受益面：缓存决策由浏览器做出）从未走查。
本会话与 1457（SPA 三安全头，并行会话在途）让号改 1458；本轮零代码改动。

## 走查环境（生产形态）

- TS 腿 dist server（`node dist/api/index.js`，gate:ts build 新鲜产物），
  root=仓根本体项目（inkos.json version=0.1.0 合法形态+books/b1+历史会话
  数据——真实度高于临时夹具）。
- IAB 真人走查（browser-use 惯用形）：DOM 级 el.click() 退化（侧栏按钮
  actionability 超时先例再现，1162 同族）、evaluate 页面内断言。

## 逐功能八面矩阵（全绿）

| # | 功能面 | 证据 |
|---|---|---|
| 1 | 首页完整渲染 | 导航/工作区/标签页全谱，API 卡片零错误态 |
| 2 | SSE 实时 | `sse-ok 实时`指示器（EventSource 在 no-store 头下正常连接，跨页存活）——1456 修复面的浏览器真链路 |
| 3 | run-log 不缓存语义 | 2 次 fetch → 2 个 resource entry、均 transferSize=368（每次走网络零缓存）+双响应 no-store——**跨会话陈旧响应被切断（1455 动机面 RunLogPanel 间歇消失机制闭环）** |
| 4 | assets 缓存保留 | 3MB 主 bundle 2 次 fetch → 仅 1 entry（二次无网络活动=缓存命中）——内容寻址缓存未被误伤，性能不回退 |
| 5 | services 族载入 | 模型配置页 38 服务商全谱（聚合/海外/国产/本地/CodingPlan）+封面配置区正常 |
| 6 | SPA 入口（469） | reload 真实重取入口+`fetch('/')` 返回 no-store |
| 7 | 空态面 | 日志「暂无日志」/书列表「还没有书」正常渲染（API 空数据不炸 UI） |
| 8 | 零错误 | error+unhandledrejection 双钩全程 0 命中 |

## 方法学沉淀：Resource Timing entry 计数对照法

浏览器级缓存行为断言的惯用形：**同方式 fetch 两次、对照 resource entry
计数与 transferSize**——no-store 端点每次 fetch 产生新 entry（走网络），
可缓存资源二次 fetch 不产生 entry（缓存服务无网络计时）。唯一变量=
响应头，无需 devtools 人工读 network 面板。比「只断言响应头存在」强一档：
头是手段，「浏览器实际不缓存」才是功能目的——本轮首次在目的层取证。

## 代码审查收尾（引证+增量）

- 1456 轮已锁：中间件顺序（use 先于全部 /api/v1 路由注册）、覆盖语义
  （Rust insert ↔ TS set 对称）、404 面（单测+活体）、project/files 单点
  （早退分支）——本轮零新发现，引证在位。
- 本轮增量=消费面验证补全：no-store 层从「头存在」升级「行为正确」实证。
- 走查期间并行会话落地 1457 SPA 三安全头（nosniff/no-referrer/DENY 对齐
  Rust static_routes）——与 no-store 层正交（assets/SPA 入口新增头），
  其轮自验；本号提交严格路径限定单文档文件避开其在途 server.ts。

## 结论

零新缺陷、零代码改动、零回归。no-store 层验证栈收口四层：dispatch 单测
（1456）→ curl 活体六面（1456）→ 双栈对照（1456）→ **浏览器行为语义
（本轮）**。生命周期：走查 tab 关闭、server 清场端口 0、tmp 零残留、
U+FFFD 不适用（零代码改动）。

## 门禁谱系与出口

HEAD=57a24fa9（1456）谱系在位（gate:ts 十腿 1456 轮全绿引证，本轮零代码
改动树未变）；下一号自 1459 起（1457 在途并行会话）。出口=下一巡检周期/
earendil-mz 单包合并。用户侧待办=Fork 推送+正式打包三脚本重烤+bench
静默补跑（可选）。
