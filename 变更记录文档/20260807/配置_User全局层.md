# 配置：新增 User/Global 可写层

> 日期：2026-08-07
> 范围：`src-tauri/src/config/{types,paths,merge,loader,reload,watcher}.rs`、`src-tauri/src/commands/config.rs`
> 目标：补齐三层配置缺失的「用户全局可写层」，使 settings 面板独立打开时有合法持久化入口；用户偏好启动即生效

## 背景

原配置为三层架构 `System(只读) < Workspace < Project`。`update_config` 对 `System` 拒绝、对 `Workspace`/`Project` 要求已选上下文。而 settings 面板（`cmd_open_plugin_manager`）独立打开时不加载工作区/项目上下文——**三层均不可写**，全局偏好（日志级别/更新通道/网络代理等）无处安放。这是 VS Code 式「用户设置 vs 工作区设置」模型中缺失的最常用一层。

## 变更

### 架构：四层合并优先级 `System < User < Workspace < Project`

| 文件 | 变更 |
|------|------|
| `types.rs` | `ConfigLayer` 加 `User` 变体（serde `snake_case` → `"user"`） |
| `paths.rs` | `user_config()` → `app_data/config/user.toml` |
| `merge.rs` | `ConfigManager` 加 `user: Option<AppConfig>` 槽 + `set_user`/`clear_user`；`recompute_merged` 在 system 之后、workspace 之前合并 user |
| `loader.rs` | `load_user_config`/`save_user_config`/`apply_user_config`；`init_manager` **启动即加载** user.toml（核心价值：偏好无需打开面板即生效）；`apply_user_config` 仅在文件存在时 `set_user`，保持 `user=None` 干净语义 |
| `reload.rs` | `reload_user_config`（镜像 workspace/project：校验→应用→更新 last_valid） |
| `watcher.rs` | `ConfigChangeEvent::UserChanged` + `watch_user_config`/`unwatch_user_config`（监听 `config/` 目录，与 workspace 子目录监听不冲突——poll 按路径精确匹配） |
| `commands/config.rs` | `update_config` 加 `User` 分支（常驻可写、**无上下文门槛**）；`reset_config` 加 `User` 分支（`clear_user`）；`start_config_watch` 常驻监听 user.toml；`poll_config_changes` 处理 `UserChanged` → reload + emit `"config-changed"` |

无需新增 Tauri 命令——`update_config`/`reset_config`/`start_config_watch` 已在 `generate_handler` 注册，User 仅是新 layer 值。

### 设计决策：ConfigLayer 不套用 EnumStandard 占位约定

`ConfigLayer` 是 **serde 数据判别器**（变体经 IPC/watcher 序列化为有意义的外部值 `system`/`user`/`workspace`/`project`），非位标志/可迭代能力枚举。强加 `None=0`/`Max` 会引入 `none`/`max` 两个非法层级值，污染 IPC/TOML 契约并在 `update_config` 的 match 产生不可达分支。能力类枚举（如 `Capability`）才适用占位约定。已在 `types.rs` 加注释说明此判断。

## 验证

- `cargo clippy --all-targets -- -D warnings`：clean
- `cargo test`：lib **346 passed / 0 failed**（+8 新测试），全部集成测试 0 失败
- 9 个新测试钉死：
  - 优先级：`test_user_overrides_system`、`test_workspace_and_project_override_user`、`test_clear_user_falls_back_to_system`
  - 持久化：`test_save_and_load_user_config`、`test_init_manager_loads_user_config`、`test_init_manager_no_user_config_keeps_system_default`
  - 热重载：`test_reload_user_config_success`、`test_watch_user_config`
  - 命令流：`test_user_layer_update_and_reset_flow`（AppState 级 update→reset 回退）

## 已知取舍

- `reset_config(User)` 仅清内存（`clear_user`），不删 user.toml 文件——与既有 workspace/project reset 行为一致（均为内存清除）。若需「永久重置」语义，应作为所有层统一增强，而非 User 独有。
- `merge_into` 的字段级覆盖取舍（显式写默认值 = 不写）继承自原架构，User 层不改变该语义。

## 后续

- settings 面板消费 User 层的「配置编辑 UI」（表单调 `update_config("user", cfg)`）——User 层已就绪，UI 为纯前端工作。
