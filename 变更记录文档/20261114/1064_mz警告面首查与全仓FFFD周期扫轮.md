# 1064 号 · mz 警告面首查与全仓 FFFD 周期扫轮

> 1063 出口执行（触发条件制新观察翼首查+提交完整性卫兵周期扫）。零代码改动轮。

## 一、mz 安装警告用户面首查（1062 新增观察翼）

- `pnpm install --frozen-lockfile` 输出 **deprecat/mariozechner/warn 零命中**（exit 0）
- 定性：**pnpm frozen install 不查询 registry deprecated 元数据**——警告仅在
  pnpm up/新增依赖解析时出现，lockfile 锁定安装（用户日常路径）零警告
- 观察翼风险降级：**用户面当前零影响**（mz deprecated 对终端用户不可见）

## 二、U+FFFD 全仓周期扫描——净

- 主体面（packages/*/src + src-tauri/src + engine-rs + scripts + docs + 变更记录文档）：
  exit=1 无命中
- 三语 README（README.md/README.en.md/README.ja.md）：exit=1 无命中
- 勘误：本仓三语 README 形态=README.md/README.en.md/README.ja.md（无 README_zh.md
  ——首查路径笔误致 grep exit 2，核实后分段重测）

## 三、上游与触发条件（1063 腿内刚查，本轮不重复）

上游 0 behind / earendil 1.5.2=现版 / 事件翼未命中——触发条件制持续。

## 四、下一号自 1065 起

出口=下一巡检周期 / earendil-mz 单包合并（触发条件制已更新）/ 用户侧待办=Fork 推送（含 955-1064）。
