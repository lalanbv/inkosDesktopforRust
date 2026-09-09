# 266 号：wasm 执行测试 fixture 入库——并行批记录归并（原编号 265，撞号让位）

> 编号说明：与并行会话已提交的 265 号（44750168，发布门禁脚本验证）撞号，
> 按「先提交占号」让位改编 266。交叉评审结论：批准——fixture 与 263 号轮
> 端到端验证过的产物 SHA-256 逐字节一致（5f8b8f2f…），双路径与全量门禁
> 独立复验均绿（576 测/clippy 0/3/3）。

# 265 号：wasm 执行测试 fixture 入库——新克隆环境不再静默跳过（wasmtime 升级行为验证兜底）

- 日期：2026-09-10
- 分支：develop
- 关联：263 号（wasmtime 升级交叉评审）、264 号（并行批记录归并，遗留 #1 已由 263 号轮次更正）
- 编号衔接：承接 264 号顺延 265
- 推送核验：HEAD 4a938d76；本地领先 origin/develop 61+；本批待提交

## 一、背景与真缺口定位

263/264 号轮次确认：`tests/wasm_plugin_execution.rs` 即真实 component 执行集成测试（echo/ping/未知命令三用例），升级 wasmtime 36 后 3/3 真跑通过——"无执行用例"断言已更正。

本轮复查发现**残余缺口**：测试产物取自 `examples/wasm-plugin/target/`（gitignore 范围），**新克隆/换机环境文件不存在 → 3 用例静默 skip**，wasmtime 升级在这些环境失去行为级验证兜底。

## 二、本批改动（待提交）

1. 组件入库：`tests/fixtures/inkos_example_plugin.wasm`（66KB，wasip2 component，受跟踪）。
2. `example_wasm()` 双路径：优先 examples 的新构建产物（本地开发迭代），缺失回退受跟踪 fixture。
3. 文件头注释同步（不再依赖 wasm 工具链）。

## 三、验证

| 场景 | 结果 |
|---|---|
| fresh 产物存在（examples target） | ✓ 3/3 通过（0.96s，走新构建） |
| fresh 产物移除（模拟新克隆） | ✓ 3/3 通过（0.18s，走 fixture） |
| src-tauri 全量 cargo test | ✓ 576 通过 0 失败 |
| src-tauri clippy --all-targets | ✓ 0 |

## 四、方法论备忘（第二次同类误判）

本会话两次"声明缺失"后均被证伪（255 号 e2e 空壳、263 号执行测试缺口）。共同根因：**搜索面不足即下"不存在"结论**（一次是 cwd 相对路径落空，一次是只查 src/ 未查 tests/）。已将教训固化至会话记忆：任何"缺失/不存在"结论必须先做全仓搜索（src + tests + scripts + 文档），并交叉验证第二信息源。

## 五、遗留

历史瘦身、推送、两项默认值、ja A/B——仍待用户决策（承接 264 号）。
