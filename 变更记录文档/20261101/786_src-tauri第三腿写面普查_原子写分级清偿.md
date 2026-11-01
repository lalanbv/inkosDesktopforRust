# 786 号：src-tauri 第三腿写面普查——原子写惯用形分级清偿

日期：2026-11-01。前置：779 号（engine-rs 94 站）、780 号（TS core 52 站）原子写收口、782 号（engine-rs 同步 IO 分级）——**src-tauri 桌面壳是三腿中从未扫过的第三条腿**（25 目标 583 测；桌面壳写配置/密钥/更新器/插件沙盒，持久性面同样真实）。

## 一、普查（分类器 + 修正）

`fs::write|write_all|OpenOptions|File::create` 全 grep 65 命中 → 按首个 `#[cfg(test)]` mod 边界分类（782 号分类器）→ **分类器修正一处**：plugin/host_api.rs 的 82/89/101 是内联小 `#[cfg(test)]` 属性，大测试 mod 在 1039——按首个边界截断会把 400 行生产站误判为测试（400 号「禁 head 截断」教训的 mod 边界变体：**逐站核上下文**为准）。

生产站 12 处/10 文件定性：

| 站 | 定性 | 处置 |
|---|---|---|
| util.rs `atomic_write_0600` | 共享助手本体：NamedTempFile 0600+flush+**sync_all（M4 审计已修）**+persist=762 完整形态 | 基准形态 |
| updater/engine.rs `download_to`（tmp+persist 缺 fsync） | **Tier A**——flush 只到 page cache，断电可致 persist 后终名半写 | +sync_all（762 形态补齐） |
| commands/registry.rs 注册表缓存 | **Tier A**——缓存是 R6 单调新鲜性锚，torn 写=锚丢失 → 降级防护失效直到下次拉取（安全面） | → atomic_write_0600 |
| plugin/host_api.rs 插件 write_file | **Tier A**——插件数据文件撕裂保护；tmp 与终名同目录=沙盒 contained | → atomic_write_0600 |
| engine/node 下载 tarball（File::create 直写终名） | Tier B 设计在位——SHA256 独立锚（nodejs.org SHASUMS）**verify-before-use** + mirror 重试截断重写 + 每 mirror `remove_dir_all` 清半成品 + 系统 node 回退，torn 产物永不进入被用状态 | 落档 |
| engine/node zip 解压直写 | Tier B——同上清理链 + `bin.is_file()` 终检 | 落档 |
| observability/crash.rs 崩溃转储 | Tier C——best-effort 遥测（纳秒唯一名防交错 + LRU 有界 + 读侧容忍），崩溃期二次故障面最小化优先 | 落档 |
| project/health.rs 可写探针 | Tier C——`.health_check_tmp` 临时探针即写即删 | 落档 |
| bin/sign-bundle.rs 签名输出 | Tier C——发布期 CLI、人在环、verify-on-use | 落档 |
| isolation/macos.rs | **非站**——write_all 写 pfctl stdin 管道非文件（grep 假阳性） | 销账 |
| secrets/jsonio.rs | 假阳性（注释行）——实际写全走 atomic_write_0600 | 销账 |
| updater async 内阻塞 chunk 写 | Tier C（同步 IO 维度，782 第二轴）——更新/引导为用户操作频率，chunk 写走 page cache | 落档 |

## 二、Tier A 转换（3 站）

1. **updater `download_to` +sync_all**：`tmp.as_file().sync_all()` 后 persist——每更新一次的 ms 级开销（网络下载量级面前可忽略），对齐 util.rs M4 审计形态；下游 sha256 校验+原子替换 engine_dir 双保险不变。
2. **registry 缓存原子化**：`atomic_write_0600(cache_path, toml_str)`——R6 单调锚掉电存活；0600 语义对同用户缓存无害（仓内原子写惯用形一致为 0600：tempfile 创建即 0600，engine-rs `write_file_atomic` 同）；best-effort 包装不变（写失败仅 warn）。
3. **host_api 插件 write_file 原子化**：同上助手；配额语义不变（成功才计 content_len，失败 NamedTempFile drop 自动清 tmp + 既有 fetch_sub 回滚）；符号链接三层闭环/配额上限/沙盒路径校验全部不受影响（tmp 落在已校验 normalized 的父目录内）。

## 三、验证

- 目标测试：host_api 41 passed（write_file happy-path 增**零残留锁**：写后数据目录恰含 1 项）+ registry 42 passed（新增 `cache_atomic_write_roundtrip_no_residue`：原子写→`read_cached_index` 可解析 + 目录恰 1 项）+ updater 39 passed。
- clippy 双 crate 0 告警（--all-targets）；testgate engine-rs 1955 + src-tauri **584**（+1 恰为本轮新测）双 0 漏洞。
- engine-rs/TS 零触碰（rust-bin 新鲜度闸随 gate:ts 下轮照常）。

## 四、教训

1. **三腿纪律**：779（engine-rs）/780（TS）/786（src-tauri）——「双栈」实为三面，桌面壳与引擎同仓不同 crate，惯用形普查按 crate 边界逐一覆盖，漏一条腿=漏整个持久性面。
2. **分类器的 mod 边界要逐站核**：内联 `#[cfg(test)]` 属性与测试 mod 混布时，首个边界截断会把生产站误判为测试（host_api:400 实证）；次序应为 grep 粗分类 → 逐站上下文复核。
3. **有正确助手 ≠ 全站正确使用**：src-tauri 的 helper 自 M4 审计起就是 762 完整形态（含 fsync），债务在分布不在助手——普查价值正在于找出「未用助手的站」。
4. **Tier B 豁免要有完整防线证据**：node 下载直写终名得以豁免，靠的是 SHA256 独立锚 verify-before-use + 重试清理 + 回退四层防线成链，缺任一层都应转 A。
5. **src-tauri lib/bin 双树**：main.rs 直挂 `mod commands;`（第二模块树）+ 其余走 `inkos_desktop::` lib 路径——commands/ 内文件被两棵树各编译一次，`crate::x` 跨模块引用只在 lib 树解析（`cargo test --lib` 绿 ≠ 门禁绿；本轮 E0433 实证）。commands/ 内跨界引用须走 `inkos_desktop::` 绝对路径（registry.rs:6 既有先例），lib-only 模块（plugin/ 等）用 `crate::` 即可（jsonio.rs:41 先例）。
