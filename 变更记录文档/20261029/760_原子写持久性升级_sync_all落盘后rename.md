# 760 号：原子写持久性升级（sync_all + 父目录 fsync）+ 手搓双轨收敛

> 目标线：自主持续开发（安全可靠优先）。757 号路线建议第 1 条落地：
> 「现原子写=rename 原子性（防撕裂），未 fsync——掉电场景仍可能零长文件」。

## 一、风险定性：文档承诺 > 实际保证

199 号以来的文档口径是「目标文件要么旧内容、要么完整新内容」，但实现只
保证 **进程崩溃** 下的该不变量（rename 对观察者原子）。掉电/内核崩溃下：

- rename 项（元数据日志）可先于数据块落盘被重放 → 目标呈 **零长/半截**；
- 多文件事务里 target→backup 与 staged→target 两段 rename 非原子持久 →
  最坏形态是 **目标缺失**。

755 号把「坏 JSON → panic」改成「坏 JSON → 静默 fallback/404」后，上述
撕裂形态的代价正是 **无声丢数据**——持久性升级是护栏闭环的必要一环。

## 二、变更内容（engine-rs，2 文件）

1. **`utils/atomic_file_set.rs`**：
   - `write_tmp_durable`/`write_tmp_durable_sync` 新私有助手：
     `File::create → write_all → sync_all` 替代 `fs::write`（数据到稳定
     存储后才 rename）；
   - `sync_parent_dir_best_effort`（async+sync 双形态，unix 实装/Windows
     跳过）：rename 成功后尽力 fsync 父目录，堵「rename 项丢失=目标缺失」；
     **尽力语义**——失败不回滚已成功写入，最坏回退 199 号保证；
   - `write_file_atomic`/`write_file_atomic_sync`：接入上述两助手（对外
     签名零变化）；
   - `commit_atomic_file_set`：`write_content`（staged 写入）sync 落盘后
     才进 rename 段；全部就位后对触碰目标父目录去重 fsync；
   - 模块文档新增「持久性（760 号）」节；新增 2 测（零长内容边界，双形态）。
2. **`server/project_config_routes.rs`**：`save_raw_config` 的手搓
   temp+rename（221 号）收敛到共享 `write_file_atomic`（757 号教训：
   同语义双轨=漏网信号），随其获得持久性升级 + Windows 文件锁重试；
   bool 契约、mtime 触发 effective_router 缓存重建联动不变；`uuid` 依赖
   从该文件清零。

## 三、默认翻转而非开关的依据（757 号建议的对齐与修正）

757 号建议「加 sync_all 开关（配置类开、高频工件类关）」。本轮对 46 站点
全量定性后**翻转默认**：调用面实测全部为用户操作/章节步进级频率（无
per-token 热循环；events/transcript 走 `append_json_line` 追加不经此路径），
fsync 成本（SSD 亚毫秒级）可忽略——统一语义消除「哪些站点忘了开」的
长期维护税。若未来出现真高频写点，再加 relaxed 变体而非回退默认。

## 四、验证

- `cargo test --lib`：**1504 passed**（基线 + 2 新测）；全目标 test gate：
  engine-rs 45 目标 **1938 passed**（=757 号基线 1936+2，对账吻合）+
  src-tauri 25 目标 583 passed。
- clippy 双 crate **0 告警**；golden 原子写差分锁（错误码映射纯函数）未触碰。
- gate:ts 八步全绿——**契约差分 0 分歧**（只改落盘方式不改文件内容），
  export-epub-smoke 走真实落盘路径实证。
