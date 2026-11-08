# 952 号 · 跨端快照回滚互灌测试锁轮 + import 后内存态勘察备案

> 951 闭环后的两向收尾：①跨端快照回滚互灌——TS 端快照构造（手写头无
> magic=V7 基础形态）与 Rust 端快照构造（Header::new_gnu）在**对侧**
> import 端点的接受性此前零覆盖（Node 端恢复留档→Rust 引擎回滚、反向
> 同）；双向测试锁落成，**零生产代码改动**（tar crate 按 V7 形态接受无
> magic 头、TS untar 对 GNU 基础头原生解析——950 读侧改造已覆盖）。
> ②import 恢复后引擎内存态一致性勘察——无患备案。
> 验证：Rust backup_routes 13/13（+1）+TS backup-routes 10/10（+1）
> +testgate 2060(+1)/584+clippy 双 0+audit:rust 双 crate 0 漏洞
> +engine-contract-diff 单步 95 端点零漂移+gate:ts 十步全绿+U+FFFD 净。

## 一、跨端快照回滚互灌（双向测试锁）

| 快照形态→回滚端 | 形态要点 | 用例 |
|---|---|---|
| TS 快照→Rust import | 手写 512B 头 257 区全零（V7/基础头）、裸名条目+裸名 manifest | import_accepts_ts_form_snapshot_rollback |
| Rust 快照→TS import | Header::new_gnu（"ustar "+" \0"）、裸名条目+裸名 manifest | 「import 接受 Rust 形态 pre-restore 快照回滚」 |

- 实证面：tar crate 对无 magic 头按 V7 形态接受（13/13 绿直接证明）；
  TS untar 对 GNU 头按基础头解析（950 起读侧 'L'/prefix 改造不涉基础
  条目形态）。
- TS 侧 simpleTar 加 gnuMagic 变体参数（与 tar-read.test.ts makeTar
  同形对偶）。
- 场景闭合：用户在双引擎间切换时，任一端留档的 pre-restore 快照均
  可在对侧回滚——备份域跨端回滚矩阵补齐。

## 二、import 后内存态一致性勘察（备案，无患）

- AppState 仅 version 字符串；BooksRuntime=hub/state(Arc<StateManager
  >)/router——无 books/works 内容内存缓存。
- StateManager 仅持 project_root（manager.rs:34-35），load_book_config
  等全部即时磁盘读——import 覆盖磁盘后读面自动一致。
- 唯一缓存面=effective_router_cache（books_routes.rs:58）：键含
  inkos.json/secrets.json mtime——import 覆盖配置即 mtime 变化=缓存自
  动失效重算，自愈设计。
- 结论：恢复端点无需缓存失效广播，入档备案。

## 三、插曲：gate:ts 高负载减速定性（非僵死）

- 首跑电池 engine-contract-diff「卡」16 分钟（CPU 瞬时抽样 0%）被杀；
  复跑仍超时。深察定性=**系统高负载减速非僵死**：load 4.2-5.6（当日
  多轮 testgate/clippy/vitest 叠加余波），差分器 95 端点慢速推进至完
  成（25x 减速，exit 0 零漂移）。
- 顺手清偿环境债：11 月 8 日两批 tsx 测试残留孤儿进程（01:08 起 4 对
  /04:25 起 5 个，/tmp 临时 root 形态佐证）全量清理——虽非本卡根因
  （净环境后仍慢），属该清之债。

## 四、备份域终态增补（951 全景后）

跨端回滚矩阵：双端 export 互灌（950）+双端快照回滚互灌（952）——备
份/恢复/回滚三环在双引擎任意组合下全闭合。

**下一号自 953 起；出口清空回归常备；用户侧待办=Fork 推送（含 948-952）。**
