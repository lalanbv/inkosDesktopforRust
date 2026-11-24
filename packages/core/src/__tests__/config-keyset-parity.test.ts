import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { DetectionConfigSchema, LLMServiceEntrySchema, ProjectConfigSchema } from "../models/project.js";

/**
 * 1604 号：跨语言键集 parity 锁——「Rust serde 键面 vs TS zod schema」一致性
 * 门禁（1600/1601/1602 三轮漂移抓虫 + 709/1429 更早的族根机制化）。
 *
 * fixture（fixtures/project-config-keyset.json）= engine-rs/tests/
 * project_config_keyset_fixture.rs 守卫断言的 ProjectConfig 最小合法 JSON
 * 反序列化→序列化值（serde default 全填充、camelCase 全键形态）。两侧守卫
 * 语义：Rust struct 键面/默认值变化 → Rust 守卫红 → 人工定性更新 fixture →
 * 本测试的键集对比与全文 parse 给出 TS zod schema 是否需要跟随的信号。
 *
 * 1666 号扩面：原守卫是**单向**的——只核「夹具里有的键 ⊆ zod shape」，对
 * 夹具没覆盖到的可选键没有信号。服务条目上 Rust 有两个类型：配置 schema 的
 * `LLMServiceEntry`（夹具来源）与 PUT 落盘用的 `ServiceConfigEntry`；后者多出
 * 的预算三键 / maxTokens 从不进夹具，strict 的 `LLMServiceEntrySchema` 也就
 * 没跟上——预算键一旦真落盘，Node 回退腿加载项目配置即崩。现由 Rust
 * `service_config_entry_full_key_sample()` 产出持久化形态全键样本
 * （`serviceEntryPersistedFull`），本文件对它做**键集相等**比对 + 值面 parse。
 */

const fixturePath = join(
  dirname(fileURLToPath(import.meta.url)),
  "fixtures",
  "project-config-keyset.json",
);
const fixture: Record<string, unknown> = JSON.parse(readFileSync(fixturePath, "utf-8"));
/** 服务条目持久化形态全键样本（不是 ProjectConfig 序列化的产物，全文 parse 前摘出）。 */
const { serviceEntryPersistedFull, ...configFixture } = fixture as {
  serviceEntryPersistedFull: Record<string, unknown>;
} & Record<string, unknown>;

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
    for (const key of Object.keys(configFixture)) {
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
    // 1605 号：fixture cover 扩展为对象形态（三键全量进 parity），null 形态
    // 回归锁移至下方独立用例（两形态都锁）。
    const parsed = ProjectConfigSchema.parse(configFixture);
    expect(parsed.name).toBe("fixture");
    expect(parsed.llm.cover?.service).toBe("kkaiapi");
    expect(parsed.llm.cover?.model).toBe("fixture-cover-model");
    expect(parsed.llm.services?.[0]?.name).toBe("FixtureSvc");
    expect(parsed.daemon.qualityGates?.maxAuditRetries).toBe(2);
    expect(parsed.foundation?.reviewRetries).toBe(2);
    // 1606 号：detection/researchSearch/modelOverrides 非默认值形态值面断言
    expect(parsed.detection?.threshold).toBe(0.8);
    expect(parsed.detection?.autoRewrite).toBe(true);
    expect(parsed.detection?.maxRetries).toBe(3);
    expect(parsed.detection?.protectedNames).toEqual(["fixture-name"]);
    expect(parsed.researchSearch?.enabled).toBe(true);
    expect(parsed.researchSearch?.apiKeyEnv).toBe("SEARCH_API_KEY");
    expect(parsed.modelOverrides?.reviser).toBe("fixture-model-b");
    expect(parsed.notify?.map((channel) => channel.type)).toEqual([
      "telegram",
      "wechat-work",
      "feishu",
      "webhook",
    ]);
  });

  it("cover:null 形态仍容（1604 nullable 回归锁，Rust Option 序列化形态）", () => {
    const withNullCover = {
      ...configFixture,
      llm: { ...(fixture.llm as Record<string, unknown>), cover: null },
    };
    const parsed = ProjectConfigSchema.parse(withNullCover);
    expect(parsed.llm.cover).toBeNull();
  });

  // ── 1666 号：服务条目**持久化形态** ↔ zod LLMServiceEntrySchema ──────────

  it("持久化形态全键样本（Rust ServiceConfigEntry）与 zod 服务条目键集相等，且值面 parse 通过", () => {
    expect(Object.keys(serviceEntryPersistedFull).sort()).toEqual(
      Object.keys(LLMServiceEntrySchema.shape).sort(),
    );
    const parsed = LLMServiceEntrySchema.safeParse(serviceEntryPersistedFull);
    expect(parsed.success, JSON.stringify(parsed.success ? "" : parsed.error.issues)).toBe(true);
  });

  it("预算键 / maxTokens 进 inkos.json 后，项目配置整体可解析（Node 回退腿加载不崩）", () => {
    // 活体走查实证的崩溃形态：PUT 把 dailyTokenBudget 真正落盘后，
    // /api/v1/agent 加载项目配置即抛 Unrecognized key(s)。
    const withBudgets = {
      ...configFixture,
      llm: {
        ...(configFixture.llm as Record<string, unknown>),
        services: [serviceEntryPersistedFull, { service: "deepseek", dailyTokenBudget: 0, maxTokens: 1.5 }],
      },
    };
    const parsed = ProjectConfigSchema.parse(withBudgets);
    expect(parsed.llm.services?.[0]).toMatchObject({
      dailyTokenBudget: 1000, monthlyTokenBudget: 50000, confirmAtBudget: false, maxTokens: 4096,
    });
    // 数值宽松：0 / 小数由归一层判「未设」，schema 层拒绝合法落盘值 = 读不了配置。
    expect(parsed.llm.services?.[1]).toMatchObject({ dailyTokenBudget: 0, maxTokens: 1.5 });
  });

  it("strict 语义保留：样本之外的真未知键仍被拒绝", () => {
    expect(LLMServiceEntrySchema.safeParse({ ...serviceEntryPersistedFull, typoKey: 1 }).success).toBe(false);
  });
});
