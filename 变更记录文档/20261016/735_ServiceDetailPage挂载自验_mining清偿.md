# 735 号：ServiceDetailPage 挂载自验（41ca307e mining 候选清偿）+ 上游巡检复核

> 目标线：自主持续开发。41ca307e 收口记录 mining 候选「ServiceDetailPage
> 挂载自验」清偿：模型配置详情页组件挂载面补测（状态推导纯逻辑面已由
> service-detail-state.test 483 行覆盖，本件补挂载面）。

## 一、上游巡检附带复核（第八巡）

fetch upstream 双 master 判据复跑：落后 upstream/master = 0，全部分支零
落后——第八巡维持。

## 二、挂载自验（service-detail-page-mount.test.tsx，3 测）

依赖 mock：fetchJson（use-api；页面 /services/config 与 store /services
调用共用）+ zustand 真 store setState 预填。

1. **未知服务挂载**：services 空 → h1 回退 serviceId 渲染 + 表单骨架在
   位（不崩）。
2. **custom 前缀服务**：预填 services + /services/config matched →
   自定义名渲染（resolvedCustomName 链）+ 连接表单核心字段在位
   （password/Base URL）。
3. **fetchJson 拒绝**：.catch 静默链不崩。

## 三、验收

- studio **692/692**（+3）；typecheck 双 tsconfig 过（Nav 类型补
  toServices）；gate:ts 八步全绿。
- mining 候选清偿进度：choice-generator 已随上游并入（721 巡检实证）、
  ServiceDetailPage 挂载自验本件清偿、maxTokensCap 语义分离留 Rust 对齐
  轮同做。

## 涉及文件

- packages/studio/src/pages/service-detail-page-mount.test.tsx（新增 3 测）
