# 725 号：0GC 第五选点 backup 导出链基线（健康归档）+ 上游定期巡检复核

> 目标线：自主持续开发。0GC 全貌续查（backup 链——未 bench 的请求处理链）
> + 上游定期巡检复核。

## 一、上游定期巡检复核（722 号判据复跑）

`git fetch upstream` 后按双 master 判据逐分支复核：**我方落后
upstream/master = 0，全部上游分支对我方零落后**（无任何分支存在我方缺
提交）——722 收口态维持，零动作。

## 二、backup 导出链 0GC 排查（第五选点）

- **链路定位**：GET /backup/export → run_export（server/backup_routes.rs，
  443 号逐字移植面）——staging 复制（含 secrets 排除）→ 双遍历 →
  manifest → tar::Builder + GzEncoder。
- **形态核查**：
  - gzip **Compression::default()（level 6）**=平衡档位合理（书稿文本
    压缩比收益真实；非过度压缩）；
  - staging 磁盘中转 = **443 号契约面语义**（TS 同构 + 失败清理回滚保险）
    ——契约面不属热点面，语义保持；
  - 每文件 `std::fs::read` 全读入内存——payload 本身即全量在内存（同量级）。
- **基线实证**：`backup_export/run_export_12ch`（12 章 × ~1600 字书稿 +
  inkos.json + .inkos 结构）= **3.57ms**——µs-ms 级健康无热点（体积线性
  放大、用户一次性导出操作，非热路径）。
- bench 落 hot_paths 新组 `backup_export`（run_export pub 化供 bench 消费
  ——注释注明契约面语义保持与压缩档位裁决）；clippy 0；lib 1499 全绿。

## 三、第五选点结论

0GC 全貌续查以 backup 链基线健康归档收口——请求处理链 bench 覆盖至此：
export（724）+ backup（725）+ 投影族（720）+ 台账渲染（719）+ 既有五组。
**0GC 轮总账**：711（-19.8%）/ 719（-99.97%）/ 720（全仓正则清偿）/
724（导出基线）/ 725（backup 基线）——已识别热点全部清偿，其余基线健康
入闸。**0GC 轮正式收口**。

## 四、教训

1. 压缩档位（default=6）这类「默认即合理」的参数面是排查时的常见假热点
   ——核查档位选择理由比调参更重要。
2. 契约面语义（staging 中转）与热点面（压缩计算）要分账——前者保持后者
   才是优化对象。

## 涉及文件

- engine-rs/src/server/backup_routes.rs（run_export pub 化 + 注记）
- engine-rs/benches/hot_paths.rs（backup_export bench 组）
