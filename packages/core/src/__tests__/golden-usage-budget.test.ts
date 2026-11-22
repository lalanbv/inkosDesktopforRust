//! R55/1660 号：用量账本与预算投影 golden 锚面（TS 生产向量）——投影
//! （日/月窗口+多服务隔离）/warn/over+确认/软关/无配置负路径/月窗口更紧。
//! Rust 侧 = engine-rs/tests/golden_usage_budget_diff.rs 同文件差分。
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  budgetStatus,
  usageProjection,
  type ServiceBudget,
  type ServiceUsage,
  type UsageLogRow,
} from "../utils/usage-budget.js";

const here = dirname(fileURLToPath(import.meta.url));
const fixture = JSON.parse(
  readFileSync(resolve(here, "golden/usage-budget-vectors.json"), "utf-8"),
) as {
  rows: UsageLogRow[];
  dayKeys: string[];
  todayKey: string;
  projection: Array<{ serviceKey: string } & ServiceUsage>;
  statuses: Array<{
    name: string;
    usage: ServiceUsage;
    budget: ServiceBudget | null;
    status: { serviceKey: string; daily: number; monthly: number; ratio: number; level: string; confirmRequired: boolean };
  }>;
};

describe("usage budget golden（R55/1660 号）", () => {
  it("投影：日/月窗口累加+多服务隔离", () => {
    const keyByTs = new Map(fixture.rows.map((row, index) => [row.ts, fixture.dayKeys[index]!]));
    const projection = [...usageProjection(fixture.rows, (row) => keyByTs.get(row.ts)!, fixture.todayKey).entries()].map(
      ([serviceKey, usage]) => ({ serviceKey, ...usage }),
    );
    expect(projection).toEqual(fixture.projection);
  });

  it.each(fixture.statuses.map((v) => [v.name, v] as const))("预算判定: %s", (_name, vector) => {
    expect(budgetStatus(vector.name, vector.usage, vector.budget ?? undefined)).toEqual(vector.status);
  });

  it("golden 非空防空转", () => {
    expect(fixture.statuses.length).toBeGreaterThanOrEqual(5);
  });
});
