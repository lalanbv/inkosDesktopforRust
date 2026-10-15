# 723 号：TS 客户端渲染卫生审计——sse 消息投影 useMemo 化（证据驱动小批）

> 目标线：自主持续开发。候选「TS 客户端侧深化」执行轮——按「审计先行、
> 证据驱动」原则做一轮有据小批，不做无证据大改。

## 一、审计结论（渲染卫生现状健康）

- **sse.messages 形态**：useState 只增数组但 **cap 100**（`prev.slice(-99)`
  + 新消息——use-sse.ts:112），消息量有界。
- **消费模式已控量**：useNewSSEMessages cursor 增量模式（seq 过滤只处理新
  消息）遍布 Sidebar/BookSidebar/ProgressSection/BookDetail 等主要消费者。
- **发现的卫生级缺口（3 处）**：组件体裸 filter 无 memo——Dashboard
  （logEvents/progressEvent 两处）与 DaemonControl（daemonEvents）。messages
  引用每消息更新即变，组件任意状态变化触发的重渲染都会重算 filter——
  **cap 100 下成本 µs 级**（卫生级，非架构级浪费）。
- **无架构级浪费证据**：无「千条消息全树重渲染」形态（cap 有界 + cursor
  增量）；更大动作（selector 化/消息分片/虚拟化）无 profiling 证据支撑，
  不做猜测性重构。

## 二、改造（3 处 useMemo 化，语义零变化）

- Dashboard.tsx：logEvents/progressEvent → useMemo（deps [sse.messages]）。
- DaemonControl.tsx：daemonEvents → useMemo（同 deps）。

## 三、验收

- studio **688/688** 全绿（Dashboard/DaemonControl 相关 9 测含交互场景）；typecheck 双 tsconfig 过；gate:ts 八步全绿。
- 改造为纯引用稳定性收敛（计算结果不变），无行为面变化——测试即回归锁。

## 四、教训

1. **深化先审计定级**：µs 级卫生问题与 ms 级架构浪费的处置不同——前者
   顺手收敛，后者需 profiling 证据立项；把卫生级当架构级做是过度工程。
2. sse 消费的三层卫生已各就位：cap 有界（store 层）+ cursor 增量（hook 层）
   + 投影 memo（组件层，本批补齐）。

## 涉及文件

- packages/studio/src/pages/Dashboard.tsx（两处 useMemo）
- packages/studio/src/pages/DaemonControl.tsx（一处 useMemo）
