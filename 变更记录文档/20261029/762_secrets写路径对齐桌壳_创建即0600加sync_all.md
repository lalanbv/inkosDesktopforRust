# 762 号：secrets.json 写路径对齐桌壳——创建即 0600 + sync_all 落盘（密钥面持久性与可读窗口闭环）

> 目标线：自主持续开发（安全可靠优先）。757 号路线①明确「密钥/配置类
> 开 durable」，760 号覆盖了 write_file_atomic 调用面，但 `llm/secrets.rs`
> 的 `save_secrets` 是手搓 temp+rename（221 号）不在该调用面——本批补齐。

## 一、两项实锤缺口

1. **持久性**：`std::fs::write(temp)` → chmod → rename，无 fsync——掉电后
   secrets.json 可零长/半截，代价=用户已存 API key 全部丢失（比普通配置高，
   221 号自己也这么定性）。
2. **可读窗口**：227 号「rename 前 chmod 0600 消除可读窗口」只消除了
   rename 前窗口；`std::fs::write` 以默认模式（umask 后通常 0644）**创建**
   临时文件，create→chmod 之间明文密钥同机其他用户可读。

对照：src-tauri `util::atomic_write_0600` 早已是 NamedTempFile 创建即 0600
+ `sync_all` 后 persist（M4 审计修复）——engine-rs 侧是落后方。

## 二、变更内容（2 文件）

- **`llm/secrets.rs` `save_secrets`**：`OpenOptions::write+create_new`（uuid
  临时名唯一，O_EXCL 额外堵符号链接/残留同名竞争）+ unix `mode(0o600)`
  创建即 0600（取代先 0644 再 chmod）→ `write_all` → `sync_all` → drop →
  rename → 父目录尽力 fsync。失败清理临时文件语义不变。
- **`utils/atomic_file_set.rs`**：`sync_parent_dir_best_effort_sync` 开
  `pub(crate)` 供 secrets 复用（不新增重复实现）。

未并入共享 `write_file_atomic_sync`：0600 创建模式是 secrets 专属语义，往
跨平台共享 API 里塞 mode 参数不值当；两者共用目录 fsync 助手即可。

## 三、调用面

`service_routes.rs` 4 站点（服务商密钥 CRUD，用户操作级频率），同步 IO
在 async handler 内——小文件毫秒级，与 757 路线③「按热路径分级」一致，
不改为 spawn_blocking。

## 四、验证

- `cargo test --lib llm::secrets` 12/12（含 `save_secrets_sets_0600_permissions`
  终态 0600 断言、`save_secrets_atomic_no_tmp_leftover`）；atomic_file_set
  15/15；全 lib **1504 passed**。
- clippy 双 crate 0 告警；gate:ts 八步全绿（rust-bin 指纹重建 15.5s）。
