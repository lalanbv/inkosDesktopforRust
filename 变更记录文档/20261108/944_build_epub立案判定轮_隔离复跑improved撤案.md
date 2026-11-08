# 944 号 · build_epub 立案判定轮：隔离复跑 improved（-2.75%）——943 备案撤案（单轮噪声定案）

> 943 备案「build_epub +8.6% 连续多轮同向才立案」的直接执行：隔离复跑
> （cargo bench --bench hot_paths -- export_artifact 单组）结果
> **-2.75%（Performance has improved）**——连续同向未成立，**撤案**
> （+8.6% 为单轮噪声，bench-gate-load-noise 纪律执行先例）。隔离值
> 1.1328ms 与 932 首轮 1.161ms 同水准。零代码改动轮。
> 验证：隔离复跑 improved；gate:ts 十步全绿（当日 914-944 十四轮连
> 绿）。

## 一、立案判定（943 备案的闭环执行）

| 轮 | 形态 | change | 判定 |
|---|---|---|---|
| 943 | 与首轮对照 | +8.6%（1.161→1.261ms） | 备案（待第二同向） |
| 944 | 隔离复跑（单组 export_artifact） | **-2.75%（improved）** | **撤案**（噪声定案） |

- 隔离值 1.1328ms 与 932 首轮 1.161ms 同水准（±2.4%）——无性能回退
  维持。

## 二、净结论

- build_epub 立案撤案（噪声定案）——bench-gate-load-noise 纪律的执
  行先例：criterion regression 报告需隔离复跑二次确认，单轮标记不立
  案。零代码改动轮。

## 三、验证

- 隔离复跑 improved；gate:ts 十步全绿（当日十四轮连绿）。

**下一号自 945 起；出口清空回归常备；用户侧待办=Fork 推送 64 个提交（至 cc375e2f）。**
