# Golden 向量采集方案（差分测试地基）

> Phase 0.4 产出：为 Rust 移植建立「同输入 → 同输出」的差分测试地基。
> 原则：TS 现有测试即活文档；不重写测试，只额外 dump 输入/输出快照供 Rust 对照。

## 目标

每个被移植的纯函数/端点，都有一组 `(input, expected_output)` JSON 向量：
- **TS 侧**：现有 vitest 测试运行时顺带 dump（或独立 golden 套件）
- **Rust 侧**：`engine-rs/tests/golden/<domain>/*.json` 读入，调用移植实现，`assert_eq!`

任一侧语义漂移 → 差分失败。这是「零功能丢失」的硬指标。

## 三层策略（按域性质选择）

| 域性质 | 策略 | 示例域 |
|---|---|---|
| 纯函数（无 IO/无 LLM） | vitest dump → Rust 差分 | utils, translation, models |
| 文件/状态相关 | 固定 fixtures + 快照 | state, prompts, skills |
| LLM/Agent（非确定） | 录制真实响应回放 + 行为契约 | llm, agent, agents |

## 采集方法 A：纯函数 dump（Phase 1 主力）

在 `packages/core` 加一个独立 vitest 套件（不动现有测试），导入目标函数，跑预置输入，写 JSON：

```ts
// packages/core/src/__golden__/utils.golden.ts
import { describe, it } from "vitest";
import { writeFileSync, mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { normalizeWordCount /* 等纯函数 */ } from "../utils.js";

const OUT = resolve(__dirname, "../../../../engine-rs/tests/golden/utils");
mkdirSync(OUT, { recursive: true });

interface Case<I, O> { name: string; input: I; expected: O }

const cases: Case<unknown, unknown>[] = [
  { name: "normalize-wordcount-cn", input: "一万二千三百四十五", expected: normalizeWordCount("一万二千三百四十五") },
  // …更多用例
];

describe("golden dump: utils", () => {
  it("writes vectors", () => {
    writeFileSync(resolve(OUT, "vectors.json"), JSON.stringify(cases, null, 2));
  });
});
```

Rust 侧：
```rust
// engine-rs/tests/golden_utils.rs
#[derive(serde::Deserialize)] struct Case { name: String, input: serde_json::Value, expected: serde_json::Value }

#[test]
fn utils_match_ts_golden() {
    let vectors: Vec<Case> = serde_json::from_str(include_str!("golden/utils/vectors.json")).unwrap();
    for c in &vectors {
        let rust_out = inkos_engine::utils::normalize_word_count(/* from c.input */);
        assert_eq!(serde_json::to_value(&rust_out).unwrap(), c.expected, "case={}", c.name);
    }
}
```

采集触发：`pnpm --filter @actalk/inkos-core test src/__golden__/`，向量随 TS 实现演进而更新（活文档）。

## 采集方法 B：端点契约（Phase 3 主力，strangler 切换守门）

同一 HTTP 请求分别打 Rust axum 与 Node Hono，响应 JSON 结构化 diff：

```ts
// packages/studio/e2e/contract-diff.spec.ts（新增，复用现有 playwright 框架）
for (const ep of ENDPOINTS_MIGRATED_TO_RUST) {
  test(`${ep} Rust==Node 响应一致`, async () => {
    const [rust, node] = await Promise.all([
      fetch(`http://127.0.0.1:${RUST_PORT}${ep}`),
      fetch(`http://127.0.0.1:${NODE_PORT}${ep}`),
    ]);
    expect(await rust.json()).toEqual(await node.json());
  });
}
```

差分失败 = 切换不可推进。CI 门禁。

## 采集方法 C：LLM 回放（Phase 3 llm/agent 域）

LLM 输出非确定，无法直接 diff。改录真实流式响应 chunk 序列，Rust 侧回放验证：
- 解析正确性（chunk → 结构化事件）
- 工具调用 JSON 提取
- 重试/退避触发点

录制件存 `tests/golden/llm/recordings/*.jsonl`（脱敏，去 API key）。

## 起步动作（Phase 1 启动时）

1. 在 `engine-rs/tests/golden/` 建目录结构（按域）
2. 选 `utils` 第一个纯函数，跑通方法 A 全链路（TS dump → Rust diff → CI）
3. 把该链路作为模板，复制到其余纯函数域
4. 域内函数 100% 覆盖 golden 向量后，该域方可接入 axum 切流量

## 边界（不迁的不测）

- `play`/`interactive-film` 的图像生成端点：依赖外部模型，归方法 C
- 静态资源 `/assets/*`：无逻辑，不差分
- WASM 插件运行时：已有 41 项加固测试 + perf bench，不在本方案范围
