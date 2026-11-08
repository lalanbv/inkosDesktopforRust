# 951 号 · pre-restore 快照回滚闭环轮：快照附 manifest（双端）

> 950 轮勘察中发现的关联缺口立案：import 确认时留档的 pre-restore 快
> 照是裸 tar（无 manifest）——回灌 backup/import 必 400（manifest 缺失
> 先验），回滚保险是「留档了但不可执行」的半成品。本轮双端快照附带裸
> 名 manifest，快照可直接经 backup/import?confirm=1 回灌=回滚闭环。
> 验证：TS backup-routes 9/9+Rust backup_routes 12/12（回滚闭环双端实
> 证）+testgate 2059/584+typecheck 净+clippy 双 0+gate:ts 十步全绿
> +U+FFFD 净。

## 一、患面与修复

- **患面**：TS 快照构造（手写 tar）与 Rust 快照构造（tar::Builder）
  均只打包 overwriting 文件、无 manifest 条目；import 端点 manifest
  缺失先验 → 快照回灌 400。回滚=手工解包，保险半成品。
- **修复**：双端快照末尾附带裸名 `backup-manifest.json` 条目（version
  1+generator inkos-desktop+kind pre-restore-snapshot 标识；TS 侧另含
  overwriting 清单供人工检视）——双端 import 端点本就兼容裸名查找与
  写回跳过（444 号「能工作的契约」），零端点改动即闭环。
- **部分回滚边界（备案）**：恢复引入的新文件不在快照中（快照只含被
  覆盖文件）——保守不引入删除语义；完整回滚需手工清理新文件。

## 二、回滚闭环实证（双端）

- TS：import confirm（覆盖 11 种子文件+快照 12 条目）→ 快照灌回
  confirm=1 → 200 restored 11 → book.json 回到「导入前」状态。
- Rust：同形——快照条目 2（文件+manifest）→ 回灌 → book.json 回
  original。修除断言锚：Rust 用例快照条目数 1→2。

## 三、插曲

- TS 回滚断言 restored 初版写死 1——实际第一次 import 覆盖面=全部
  11 种子文件（export 包条目全已存在），快照 11 文件+manifest；改派
  生断言（snapshotEntries.length-1），顺带实证快照保留全部被覆盖文
  件（回滚面=完整覆盖面 ✓）。

## 四、备份域终态（938→951 全景）

export 十条目+sessions 参数真化+打包前 sync（948/949 排序/隔离/目录
过滤）+restore 两段式+长路径三通道跨端互灌（950）+回滚保险可执行
（951）——备份/恢复/回滚三环全闭合，四向互灌矩阵全锁。

**下一号自 952 起；出口清空回归常备；用户侧待办=Fork 推送（含 948-951）。**
