# 500 号：导演灵感卡 Rust 侧服务端写入（497 备案清偿·Rust 侧，运行待许可）

日期：2026-09-16　分支：develop　基线：1bec51b7（499 号）

## 实现

497/498 号"建书 → 导演灵感卡"专项的 **Rust 引擎侧**落地（镜像 498 TS 语义）：

- 新增 `write_director_inspiration_card(project_root, book_id, premise)`（book_create_routes.rs，纯 fs 参数化可单测）：写 `.inkos/director/{bookId}.json`——**已有灵感卡不覆盖**（幂等）、bookId 盖章、stage 缺省 `directions`、`inspiration {premise, keywords:[]}`、updatedAt = utc_now_ms（对齐引擎既有约定）；
- 接线：`create_book` 后台创建链的完成点——`complete_book_exists` 通过后、`book:created` 广播前；premise 取 `blurb`（缺省 title）；**失败不阻断建书**（eprintln 告警）；
- 新增 2 个 tempdir 单测（写入形状默认值 + 已有卡不覆盖）。

## 验证状态（诚实记录）

- `cargo check --tests` **0 错**（类型门禁通过）；
- **单测运行与全量 cargo test 仍被 Xcode 许可阻断**（exit 69，持续第 N 个循环）——本号 2 个新单测随 481 号欠账在许可解除后补跑；
- TS 侧零改动（workspace 3217 全绿状态延续）。

## 后续（cargo 解阻后一次性清算队列）

1. 全量 cargo test（481 号欠账 + 本号 2 单测 + 489 号 2 新单测）；
2. duel + bench:gate；
3. skills/genres 移植评估；resync Rust 侧残余评估。
