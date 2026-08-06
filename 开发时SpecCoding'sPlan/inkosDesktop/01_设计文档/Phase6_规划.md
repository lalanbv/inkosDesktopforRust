# Phase 6 规划：增量更新 + 遥测 + 前端管理面板

> 日期：2026-08-06
> 前置：Phase 4（项目管理）+ Phase 5（插件系统）已完成并验证

## 现状评估

### 已完整交付（验证通过）
- 引擎管理：Node bootstrap + 双通道 updater（M3）✅
- 项目选择/启动 + picker 前端 ✅
- 可观测性：tracing 日志 + panic hook + 诊断命令（M4a）✅
- secrets 管理（keychain + 文件同步，M2b）✅
- 工作区管理（M5a）✅
- 配置分层系统（M5b）✅
- 项目管理：SQLite 索引 + 检测 + 扫描 + 健康 + 生命周期 + picker 集成（Phase 4）✅
- 插件系统：进程隔离（真执行）+ WASM 沙箱骨架 + 权限模型（Phase 5）✅

### 验证指标
- `cargo test`：311 lib + 全部集成测试，0 失败
- `cargo clippy --all-targets -- -D warnings`：零告警
- `cargo build --release`：成功

## Phase 6 候选项（按价值/成本排序）

### 6.1 增量更新优化（delta patch）—— 高价值/高成本
**缺口**：当前 updater 全量替换 engine bundle（数十 MB）。商业级应支持二进制 delta（仅传差异）。
**方案**：bsdiff/courgette 算法，服务端预生成 delta，客户端校验后应用。
**成本**：2-3 周（含服务端 delta 生成管道）。
**收益**：更新包从 ~30MB 降到 ~1MB，弱网体验显著提升。

### 6.2 遥测与性能监控 —— 中价值/中成本
**缺口**：有日志但无指标聚合（启动耗时、插件调用耗时、扫描耗时）。
**方案**：tracing 已埋点，加 metrics exporter（Prometheus 或本地聚合），picker/托盘展示。
**成本**：1 周。
**收益**：可观测性从"事后排查"升级到"实时监控"。

### 6.3 插件 Component Model 完整执行（M7g-phase2）—— 高价值/高成本
**缺口**：WASM 路径加载/编译为真，但类型化调用需 wit 接口 + bindgen。
**方案**：定义 `inkos.wit`（host + plugin 接口），`wasmtime::component::bindgen!` 生成链接器。
**成本**：2 周。
**收益**：插件可走强隔离 WASM 沙箱（而非进程隔离），安全性跃升。

### 6.4 前端管理面板 —— 中价值/中成本
**缺口**：picker 仅项目选择，无设置面板（配置/插件/工作区管理 UI）。
**方案**：独立 settings.html（沿用 picker 的 vanilla JS + withGlobalTauri 风格，零构建链）。
**成本**：1-2 周。
**收益**：用户可图形化管理插件/配置，无需改 projects.json/plugin.toml。

### 6.5 生产签名配置（M4c，可选）—— 低成本
**缺口**：updater pubkey 已配置，缺私钥签名管道（CI 侧）。
**方案**：CI 注入签名密钥，`tauri signer sign` 签发布产物。
**成本**：2-3 天（CI 配置）。
**收益**：发布产物可验证来源，防篡改。

## 建议优先级

1. **6.5（签名）** —— 成本最低，安全合规基础，先做
2. **6.4（前端面板）** —— 用户可见价值最高
3. **6.2（遥测）** —— 为后续优化提供数据
4. **6.1（增量更新）** —— 价值高但需服务端配合
5. **6.3（WASM 完整执行）** —— 进程隔离已满足当前安全需求，可后置

## 不建议立即做

- SEA 优化（M4f）：已被 sidecar 方案替代，Node 单文件打包与当前架构冲突
- 插件热重载：进程隔离下重启进程即可，热重载收益有限
- 插件间通信：无明确用例驱动，YAGNI
