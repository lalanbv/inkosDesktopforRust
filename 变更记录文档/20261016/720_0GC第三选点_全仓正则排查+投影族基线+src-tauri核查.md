# 720 号：0GC 对齐轮第三选点——全仓正则编译排查清偿 + settle 投影族基线 + src-tauri 核查

> 目标线：自主持续开发。719 号分段归因法的**方法论外推**：以「正则编译在
> 循环里」为信号的全仓静态排查 + settle 投影族 bench 基线 + 桌面壳核查。

## 一、全仓 Regex::new 排查（静态信号扫描）

非 OnceLock 惯用的调用点全仓仅 2 处漏网（其余 materials/references 主族/
play_runner/forecast/notify/projections 均已是 OnceLock 惯用）：

1. **references.rs extract_material_content**：素材标记正则函数内编译——
   OnceLock 预编译顺手清偿（素材导入面，量相关放大）。
2. **ai_tells.rs count_word**：**每次调用动态 `Regex::new(escape(word))`**
   （dim21/22 每词每章各一次，eval 全书扫描乘法放大）——**零正则化改写**：
   TS 源 `new RegExp(word,"gi")` 无词边界 = 大小写不敏感非重叠子串计数 →
   `to_lowercase + match_indices` 等价实现（词表纯 ASCII，unicode 折叠与
   (?i) 等价；ai_tells 8 测全绿语义不变）。

## 二、settle 投影族全貌 bench（第三选点基线，入 bench-gate 作回归闸）

| bench（100 规模） | 基线 | 判定 |
| --- | --- | --- |
| projections_hooks_100 | 115µs | 无热点（正常分配形态） |
| projections_summaries_100 | 56.9µs | 无热点 |
| projections_current_state_100 | 34.2µs | 无热点 |

**结论：settle 投影族无 719 级热点**（ms 级重复初始化已绝迹；投影族 µs 级
常态）。全仓排查 + 投影基线 = 第三选点以「清偿 2 处漏网 + 建回归闸」收口。

## 三、src-tauri 桌面壳对齐核查（复核性通过）

- clippy --all-targets **0**；test 2/2 全绿；TODO/FFIXME 扫描 **0**。
- engine-rs 复核：clippy 0、lib 1499 全绿；全 bench sweep 复核
  （story_markdown 组 +6-9% 负载噪声级波动，**基线不刷新**——711 号
  「基线是回退闸非成绩单」纪律；render_hook_snapshot 1.72µs 量级保持）。

## 四、教训

1. **方法论外推**：719 的「正则编译在循环里」是可静态扫描的信号模式
   （grep Regex::new 非静态上下文）——一次归因，全仓排查清偿同类。
2. 动态正则的零正则化改写要先核 TS 源语义（`new RegExp(word,"gi")` 无词
   边界=子串计数，与直觉的「整词匹配」不同——按源语义改写而非按直觉加强）。
3. 全貌 bench 基线的价值在「下次有对照」——µs 级常态组也是回归闸资产。

## 涉及文件

- engine-rs/src/references.rs（标记正则 OnceLock）
- engine-rs/src/agents/ai_tells.rs（count_word 零正则化）
- engine-rs/benches/hot_paths.rs（settle 投影族三 bench 基线）
