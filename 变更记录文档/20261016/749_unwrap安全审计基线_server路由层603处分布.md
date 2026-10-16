# 749 号：unwrap 安全审计基线（server 路由层 603 处分布实证）+ 上游第二十巡

> 目标线：自主持续开发。Rust 引擎 server 路由层的 unwrap 安全审计基线：
> 分布实证 + 风险分档 + 改进路径规划（非一次性大改——603 处的 mass rewrite
> 回归风险远大于收益）。

## 一、上游巡检附带复核（第二十巡）

fetch upstream 双 master 判据复跑：落后 upstream/master = 0，全部分支零
落后——第二十巡维持。

## 二、unwrap 安全审计基线

### engine-rs server 路由层分布

| 文件 | unwrap 数 | 风险档 |
| --- | --- | --- |
| books_state_routes.rs | 99 | 中（章节/索引 IO） |
| ops_routes.rs | 56 | 中（导演/雷达等 ops） |
| agent_production.rs | 56 | 低（内部管线调用） |
| mod.rs | 46 | 低（路由注册/中间件） |
| books_routes.rs | 45 | 中（规划/组装/导出） |
| agent_route.rs | 44 | 中（agent 会话路由） |
| 其余 | ~257 | 低-中 |

**总计 603 处**。

### 风险分档

- **低风险（大多数）**：`serde_json::to_value(&result).unwrap()`（已构造值
  必可序列化）、`HashSet/HashMap` 操作（前置有 insert）、类型转换
  （`as u32` 前有范围检查）——**不可达 panic**。
- **中风险（需个案审计）**：`read_to_string().unwrap()`（文件 IO）、
  `parse().unwrap()`（外部输入转换）、锁 `unwrap()`（死锁风险）。
- **改进路径**（另案渐进）：新代码用 `?` + `map_err`；存量代码按文件
  优先级渐进（books_state_routes 优先——IO 密集）；不一次性 mass rewrite
  （回归风险>收益）。

### src-tauri

server 路由层 **0 unwrap**（通过 `Result` + `map_err` 严格错误传播——
桌面壳安全性优于引擎层）。

## 三、教训

1. unwrap 审计的价值在于**建立基线**而非一次性清零——603 处中大多数是
   安全模式，清零的回归风险远大于收益。
2. 改进策略应是「增量约束」（新代码禁止裸 unwrap）+「高风险优先渐进修」
   而非 mass rewrite。

## 涉及文件

- 本件（审计基线归档，零代码改动）
