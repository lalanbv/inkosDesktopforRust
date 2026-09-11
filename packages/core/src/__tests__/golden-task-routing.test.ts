//! G16/345 号：按任务模型路由 golden 断言（Phase B 批次三末项）。
//!
//! 唯一事实源 = `golden/task-routing-vectors.json`；Rust 侧
//! `engine-rs/tests/golden_task_routing_diff.rs` 读同一文件差分（解析一致 duel）。
//! 四组断言：解析决策表（逐字段回退 + 来源标注）、迁移兼容（无 routing 全
//! fallback）、任务独立性、契约形状。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  TASK_MODEL_KINDS,
  resolveTaskModel,
  type TaskModelKind,
  type TaskModelRouting,
} from "../models/task-routing.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "golden/task-routing-vectors.json"), "utf-8"),
) as {
  fallback: { model: string; service: string; temperature: number; maxTokens: number };
  resolve: Array<{
    name: string;
    input: {
      task: TaskModelKind;
      bookRouting: TaskModelRouting | null;
      projectRouting: TaskModelRouting | null;
    };
    expected: {
      task: string;
      model: string;
      service: string;
      temperature: number;
      maxTokens: number;
      sources: Array<{ field: string; source: string }>;
    };
  }>;
  contract: unknown;
};

const F = vectors.fallback;

describe("task model routing (G16)", () => {
  it("resolves every task model per shared decision-table vectors", () => {
    for (const vector of vectors.resolve) {
      const got = resolveTaskModel({
        task: vector.input.task,
        ...(vector.input.bookRouting ? { bookRouting: vector.input.bookRouting } : {}),
        ...(vector.input.projectRouting ? { projectRouting: vector.input.projectRouting } : {}),
        fallbackModel: F.model,
        fallbackService: F.service,
        fallbackTemperature: F.temperature,
        fallbackMaxTokens: F.maxTokens,
      });
      expect({ ...got, sources: got.sources }, vector.name).toEqual({
        task: vector.expected.task,
        model: vector.expected.model,
        service: vector.expected.service,
        temperature: vector.expected.temperature,
        maxTokens: vector.expected.maxTokens,
        sources: vector.expected.sources,
      });
    }
  });

  it("keeps five task kinds independent", () => {
    // writing 的覆盖不得影响其他任务（同 routing 下逐一断言 model 回退）。
    const projectRouting: TaskModelRouting = {
      tasks: { writing: { model: "writer-only" } },
    };
    for (const kind of TASK_MODEL_KINDS) {
      const got = resolveTaskModel({
        task: kind,
        projectRouting,
        fallbackModel: F.model,
        fallbackService: F.service,
        fallbackTemperature: F.temperature,
        fallbackMaxTokens: F.maxTokens,
      });
      expect(got.model).toBe(kind === "writing" ? "writer-only" : F.model);
    }
  });

  it("migration compat: absent routing resolves everything to fallback", () => {
    for (const kind of TASK_MODEL_KINDS) {
      const got = resolveTaskModel({
        task: kind,
        fallbackModel: F.model,
        fallbackService: F.service,
        fallbackTemperature: F.temperature,
        fallbackMaxTokens: F.maxTokens,
      });
      expect(got.model).toBe(F.model);
      expect(got.service).toBe(F.service);
      expect(got.temperature).toBe(F.temperature);
      expect(got.maxTokens).toBe(F.maxTokens);
      expect(got.sources.every((s) => s.source === "fallback")).toBe(true);
    }
  });

  it("freezes the machine-readable contract shape", () => {
    expect(vectors.contract).toEqual({
      kinds: [...TASK_MODEL_KINDS],
      fields: ["model", "service", "temperature", "maxTokens"],
      levels: ["book-task", "book-defaults", "project-task", "project-defaults", "fallback"],
      migrationCompat: "absent-routing-resolves-to-fallback",
    });
  });
});
