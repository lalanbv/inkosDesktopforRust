import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { DetectionConfigSchema, ProjectConfigSchema } from "../models/project.js";

/**
 * 1604 号：跨语言键集 parity 锁——「Rust serde 键面 vs TS zod schema」一致性
 * 门禁（1600/1601/1602 三轮漂移抓虫 + 709/1429 更早的族根机制化）。
 *
 * fixture（fixtures/project-config-keyset.json）= engine-rs/tests/
 * project_config_keyset_fixture.rs 守卫断言的 ProjectConfig 最小合法 JSON
 * 反序列化→序列化值（serde default 全填充、camelCase 全键形态）。两侧守卫
 * 语义：Rust struct 键面/默认值变化 → Rust 守卫红 → 人工定性更新 fixture →
 * 本测试的键集对比与全文 parse 给出 TS zod schema 是否需要跟随的信号。
 */

const fixturePath = join(
  dirname(fileURLToPath(import.meta.url)),
  "fixtures",
  "project-config-keyset.json",
);
const fixture: Record<string, unknown> = JSON.parse(readFileSync(fixturePath, "utf-8"));

/** 递归提取对象键集（段内嵌套展开，数组取首元素形态）。 */
function collectKeys(value: unknown, prefix = ""): string[] {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return [];
  const keys: string[] = [];
  for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
    keys.push(prefix ? `${prefix}.${key}` : key);
    keys.push(...collectKeys(child, prefix ? `${prefix}.${key}` : key));
  }
  return keys;
}

describe("ProjectConfig 键面 parity（Rust fixture ↔ TS zod schema）", () => {
  // 1429 号先例：TS 侧显式容键超集（Rust 无此字段的合法容错）。
  const toleratedExtraKeys = new Set(["inputGovernanceMode"]);

  it("fixture 顶层/段内键集 ⊆ TS zod shape 键集（容键白名单外零缺口）", () => {
    const shapeKeys = new Set(Object.keys(ProjectConfigSchema.shape));
    const missing: string[] = [];
    for (const key of Object.keys(fixture)) {
      if (!shapeKeys.has(key) && !toleratedExtraKeys.has(key)) missing.push(key);
    }
    expect(missing).toEqual([]);

    // 段内抽查：llm/daemon/detection 段键集（1600-1602 抓虫面回归域）
    const llmShape = Object.keys(
      (ProjectConfigSchema.shape.llm as { shape: Record<string, unknown> }).shape,
    );
    for (const key of Object.keys(fixture.llm as Record<string, unknown>)) {
      expect(llmShape).toContain(key);
    }
    // daemon 段经 ZodDefault 包装（zod v3 内层在 _def.innerType）
    const daemonShape = Object.keys(
      (
        ProjectConfigSchema.shape.daemon as unknown as {
          _def: { innerType: { shape: Record<string, unknown> } };
        }
      )._def.innerType.shape,
    );
    for (const key of Object.keys(fixture.daemon as Record<string, unknown>)) {
      expect(daemonShape).toContain(key);
    }
    const detectionShape = Object.keys(DetectionConfigSchema.shape);
    for (const key of Object.keys(fixture.detection as Record<string, unknown>)) {
      expect(detectionShape).toContain(key);
    }
  });

  it("fixture 全文 parse 通过（值面活体：null/默认值形态与 zod 容值对齐）", () => {
    // 1604 抓虫面：Rust Option 无 skip_serializing_if 序列化 cover:null，
    // zod .optional() 只容 undefined——nullable 补齐后此处才绿。
    const parsed = ProjectConfigSchema.parse(fixture);
    expect(parsed.name).toBe("fixture");
    expect(parsed.llm.cover).toBeNull();
    expect(parsed.daemon.qualityGates?.maxAuditRetries).toBe(2);
    expect(parsed.foundation?.reviewRetries).toBe(2);
    expect(parsed.detection?.protectedNames).toEqual([]);
  });
});
