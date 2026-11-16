# 1482 号：tauri resource 陈旧族教训沉淀轮（零代码改动）

## 目标与范围

/goal 自主持续开发续轮。巡检未到期（2026-11-17）。1479-1481 三部曲的
系统性教训（tauri resource 陈旧族）沉淀入 `desktop-static-bake-chain`
记忆——直跑验证必遇的跨会话知识，此前该记忆只盖 static 面未盖引擎
二进制面。

## 沉淀内容（1479-1481 段）

- **`cargo build --bin` 不刷新 tauri resource 拷贝**：直跑形态经
  resource_dir 可能命中数月前引擎二进制（1479 实证 8 月产物，watchdog
  静默缺席）；正式打包链无此面（脚本自带重编）
- **守卫已机制化**：`rustbin::warn_if_stale`（resource 命中分支，条目
  mtime 比对→启动告警）
- **验证手法坑**：touch 目录自身不改其条目 mtime（守卫读条目级）——
  造陈旧须 touch 子目录/文件；engine-rs 改动后直跑验证须 release 重编
  +手动 cp 到 `target/release/engine-rust/`
- **双 spawn 路径纪律**：`rustbin::build_launch`（Rust 直启）与
  `supervisor::build_launch`（Node 回退）是两条独立 spec 构造——env
  契约改动两处都要注入（1479 抓出单侧漏=死契约面）

零代码改动零门禁面，U+FFFD 净。

出口=下一巡检周期（2026-11-18 到期，明日触发全量巡检轮）/earendil-mz
单包合并；用户侧待办=**Fork 推送（含 955-1482，Fork 图形端操作）+
正式打包三脚本重烤（必须含 engine-rs release 重编）**。下一号自 1483 起。
