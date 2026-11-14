# 1055 号 · bench 复扫与漂移定性轮（静默窗口顺延项执行）

> 1054 出口执行（静默窗口顺延项优先）。零代码改动轮，零基线接触（无 --update）。

## 一、bench:gate 复扫（窗口开启后通过）

- 窗口核查：loadavg 3.43→19%/核 < 20% 阈值（并行会话 1053 已提交负载释放；其间
  一次 21%/核再拒=闸门持续在岗）——窗口开启后整跑通过
- **bench 门禁通过**：28 基准全部带内（基线 2026-11-01T02:57Z，阈值 +30%），
  27/28 在 ±8.4% 内双向漂移
- 点状漂移 top2：sensitive_words/scan_chapter **+14.8%**、sse_broadcast/dispatch/8 **+13.4%**

## 二、漂移定性（1034 判例同款流程，结论不同形）

| 步骤 | 结果 |
|---|---|
| 隔离复跑 sensitive_words 组 | 两变体 **No change**（p=0.28/0.27）——值稳定复现，非采样抖动 |
| 相对基线 | scan_chapter 18.26µs vs 基线 15.85µs=+15.2% **未回基线**（与 1034「隔离回基线」不同形） |
| **git 考古终证** | 基线点 481ce5fd（11-01 10:57）..HEAD 对 `agents/sensitive_words.rs`+`utils/language.rs`+`server/sse.rs` **diff 零改动** |
| 双向漂移佐证 | 多基准负漂移（projections_summaries -19.2%/hooks -8.4%/chapter_workspace -6.4%/run_export -5.9%） |
| 同族不同步 | builtin_only +3.4% vs scan_chapter +14.8%（正则段对缓存态敏感特征） |

**定性=环境噪声（非真回归）**：代码零改动+测量变=测量环境漂移（负载/内存布局/
缓存态）；engine-rs 基线后改动面（server 路由/写链 agents/pipeline）与 bench
基准面（纯函数微基准引用三模块）零相交——**性能维度零回归实锤**。基线不洗。

## 三、上游与触发条件周期复查

- 上游 0 behind / mz 双件 0.73.1 仍冻结（2026-05-07）/ earendil pi-ai 1.5.2=现版
- 触发条件三条件全未命中——触发条件制持续

## 四、下一号自 1056 起

出口=earendil-mz 单包合并（触发条件制持续）/ 下一巡检周期 / 用户侧待办=Fork 推送（含 955-1055）。
