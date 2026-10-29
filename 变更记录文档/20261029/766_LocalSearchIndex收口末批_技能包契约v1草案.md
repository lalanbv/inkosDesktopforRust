# 766 号：LocalSearchIndex 剩余消费方清偿（异步面 sqlite 收口末批）+ 技能包契约 v1 草案文档化

> 目标线：高性能 0GC 收尾 + 可扩展性轴零风险切片。763-765 号系列后
> 台账仅存 LocalSearchIndex 工具/检索消费方与路线④文档缺口。

## 一、LocalSearchIndex 三处消费方 spawn_blocking 化

| 站点 | 原形态 | 清偿 |
|---|---|---|
| `materials.rs` retrieve_materials | 索引 new（.inkos/retrieval.db，含 migrate）+ replace_scope + search + 映射整体在 runtime 线程 | sqlite 段整体入 spawn_blocking（打开失败→空Vec 语义保留），映射留在异步侧；`rusqlite::Result` 包装随之消解（无失败态残留） |
| `memory_retrieval.rs` retrieve_memory_selection | 磁盘索引 new + replace_scope + search 直跑（520 号磁盘 FTS 重建） | 单阻塞闭包返回 `(hits, opened)`——`opened` 供后续 `db_path`/`retrieval_trace` 投影（原 `index.as_ref().map(\|_\|…)` 仅用存在性） |
| `skill_tool.rs` retrieve_skill_resources | `:memory:` 索引 populate+search | 同批搬移（open 廉价但属同一不变式：Connection 不出现在 runtime 线程） |

### grep_walk 定性（入档豁免）

`interaction/project_tools.rs` grep 工具执行器的同步 walk：**有界设计**
（深度 ≤3、文件数 ≤500、扩展名白名单），且工具轮次由多秒级 LLM 调用
主导——阻塞占比可忽略；执行器为同步签名（registry ToolDef 形态），
异步化属注册表架构改造，性价比不支持。**豁免入档**。

## 二、技能包契约 v1 草案（路线④零代码切片）

`skills/external_loader.rs` 模块文档新增契约草案节：事实契约字段
（`name`/`description`/`disable-model-invocation`/2MB 上限）+ **承诺状态
=草案**，显式标注 `minEngineVersion`、信任模型、分发形态三项待用户
定夺后另行收口。用户三项决策未答（本轮 AskUserQuestion 无响应），
**不替用户拍板**：信任门 UI 与分发形态不实现；契约不正式冻结。
`docs/` 目录被 gitignore，契约文档落模块文档（入库存放位）。

## 三、验证

- `cargo test --lib` **1504 passed**；全目标 testgate：engine-rs **1938** +
  src-tauri **583** passed；clippy 双 crate **0 告警**（拦下
  `unwrap_or_else(|_| None)` → `unwrap_or(None)` 一处并已修）。
- gate:ts 八步全绿（rust-bin 指纹重建；契约差分 0 分歧）。
- 收口声明：async 宿主内同步文件 IO（763）+ MemoryDb（764/765）+
  LocalSearchIndex（本号）后，**已知 sqlite/文件阻塞面全部清偿或入档
  豁免**，无隐性存量。

## 四、待用户输入（路线④解锁条件）

1. 信任模型：外部源显式确认 vs 免确认全信任 vs 全部显式信任（已给
   推荐项：外部源确认+项目惯例目录默认信任）。
2. 分发形态：裸目录约定（推荐）vs .inkskill 打包 vs 注册表。
3. 契约范围：现有字段冻结+minEngineVersion（推荐）vs 最小冻结 vs 广契约。
