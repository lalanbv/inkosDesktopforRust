//! 跨引擎 work 域金标准（TS 侧）。与 Rust 侧 `engine-rs/tests/work_domain_cross_engine.rs`
//! **逐字共用**同一组金标准字符串（程序化核验逐字一致）：双引擎（Rust 引擎 ↔
//! Node 回退端）交替读写同一项目的 works/<id>/ 与 harness.sqlite，schema 漂移即
//! 两侧必红。
//!
//! 背景：882 号落了 Rust 腿金标准但 TS 腿未随轮落盘（882 备案缺口）；884 号 M1b
//! 清偿并扩展 profile 金标准（含 contextRecipe 全字段——/profiles 端点契约面）。
//!
//! 覆盖：work.json 双版本 artifact 不变量（r1 superseded + r2 current）、
//! strict 未知键/幽灵 currentRevisionId 拒读、WorkProfile 2.0 parse 全量形状
//!（含 contextRecipe）、episode+events 的 JSON 出参形状。

import { mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  CreativeEpisodeStore,
  builtInWorkProfiles,
  createWorkManifest,
  listWorkManifests,
  loadWorkManifest,
  saveWorkManifest,
  WorkManifestSchema,
} from "../index.js";

const GOLDEN_WORK_MANIFEST_JSON = `{
  "version": 2,
  "id": "walk-diff",
  "title": "雨夜碎镜（跨引擎金标准）",
  "profileId": "short-fiction",
  "language": "zh",
  "status": "active",
  "lineage": [],
  "artifacts": [
    {
      "id": "outline",
      "kind": "outline",
      "currentRevisionId": "r2",
      "revisions": [
        {
          "id": "r1",
          "parentRevisionId": null,
          "path": "artifacts/outline/revisions/r1.md",
          "contentType": "text/markdown",
          "status": "superseded",
          "checksum": "sha256:70c4577c024de823b62a833caf7733f07e729921c6b49875db8b1602403e7331",
          "byteLength": 68,
          "createdAt": "2026-11-02T00:00:00.000Z"
        },
        {
          "id": "r2",
          "parentRevisionId": "r1",
          "path": "artifacts/outline/revisions/r2.md",
          "contentType": "text/markdown",
          "status": "current",
          "checksum": "sha256:f0d2e9e9414e33c22c84474ba8902291818bf01a4089bbc5245315d2c16782d6",
          "byteLength": 107,
          "createdAt": "2026-11-03T00:00:00.000Z"
        }
      ],
      "metadata": {}
    }
  ],
  "metadata": {},
  "createdAt": "2026-11-02T00:00:00.000Z",
  "updatedAt": "2026-11-03T00:00:00.000Z"
}`;

const GOLDEN_REV2_TEXT = "# 短篇大纲 v2\n\n雨夜来客叩门；斋主窥见镜中人影与自己同貌，交易自此刻开始。\n";

/// 内置 short-fiction profile 的 /profiles 出参金标准（zod parse 后全字段——
/// contextRecipe/default 补全项齐；字段声明序 = JSON.stringify 序 = Rust serde
/// 结构体声明序）。与 Rust 侧 tests/work_domain_m1b.rs GOLDEN_*_PROFILE_JSON 逐字共用。
const GOLDEN_SHORT_FICTION_PROFILE_JSON = `{
  "version": 2,
  "id": "short-fiction",
  "title": "Short fiction",
  "description": "",
  "capabilityIds": [
    "workspace",
    "short-fiction",
    "adaptation",
    "visual"
  ],
  "requiredSkillIds": [
    "inkos-short-writing"
  ],
  "recommendedSkillIds": [],
  "artifactKinds": [
    "outline",
    "manuscript",
    "sales-package",
    "cover"
  ],
  "confirmation": {
    "inferredMutation": "execute",
    "explicitRecoverableMutation": "execute",
    "destructiveMutation": "confirm"
  },
  "contextRecipe": {
    "id": "short-fiction",
    "sourceIds": [
      "task",
      "skills",
      "work"
    ]
  },
  "artifactSchemas": {
    "source/final/short-story.json": "short-manuscript",
    "source/final/sales-package.json": "short-package"
  },
  "qualityCriteria": [],
  "production": {}
}`;

const GOLDEN_WORKSPACE_DEFAULT_PROFILE_JSON = `{
  "version": 2,
  "id": "workspace-default",
  "title": "Creative workspace",
  "description": "Discuss goals, inspect works, and create derived works.",
  "capabilityIds": [
    "workspace",
    "adaptation",
    "translation",
    "visual"
  ],
  "requiredSkillIds": [],
  "recommendedSkillIds": [],
  "artifactKinds": [
    "brief",
    "source",
    "export"
  ],
  "confirmation": {
    "inferredMutation": "execute",
    "explicitRecoverableMutation": "execute",
    "destructiveMutation": "confirm"
  },
  "contextRecipe": {
    "id": "workspace-default",
    "sourceIds": [
      "task",
      "skills",
      "work"
    ]
  },
  "artifactSchemas": {},
  "qualityCriteria": [],
  "production": {}
}`;

const goldenProfile = (id: string) => {
  const profile = builtInWorkProfiles().find((candidate) => candidate.id === id);
  expect(profile, `内置 profile ${id} 存在`).toBeDefined();
  return JSON.stringify(profile, null, 2);
};

describe("跨引擎 work 域金标准（882/884）", () => {
  let root: string;

  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "inkos-884-"));
  });

  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  it("金标准 work.json parse 后再序列化逐字一致（字段声明序 + strict 字段集）", async () => {
    const manifest = WorkManifestSchema.parse(JSON.parse(GOLDEN_WORK_MANIFEST_JSON));
    expect(JSON.stringify(manifest, null, 2)).toBe(GOLDEN_WORK_MANIFEST_JSON);
    await saveWorkManifest(root, manifest);
    const loaded = await loadWorkManifest(root, "walk-diff");
    expect(loaded).toEqual(manifest);
  });

  it("strict 拒未知键 + superRefine 拒幽灵 currentRevisionId", () => {
    expect(() => WorkManifestSchema.parse(JSON.parse(
      GOLDEN_WORK_MANIFEST_JSON.replace('"lineage": []', '"lineage": [], "sneaky": 1'),
    ))).toThrow();
    expect(() => WorkManifestSchema.parse(JSON.parse(
      GOLDEN_WORK_MANIFEST_JSON.replace('"currentRevisionId": "r2"', '"currentRevisionId": "ghost"'),
    ))).toThrow();
  });

  it("内置 profile 金标准逐字一致（short-fiction / workspace-default，含 contextRecipe）", () => {
    expect(goldenProfile("short-fiction")).toBe(GOLDEN_SHORT_FICTION_PROFILE_JSON);
    expect(goldenProfile("workspace-default")).toBe(GOLDEN_WORKSPACE_DEFAULT_PROFILE_JSON);
    expect(builtInWorkProfiles()).toHaveLength(9);
  });

  it("createWorkManifest 草稿 + listWorkManifests updatedAt 降序 + id tiebreak", async () => {
    const draft = createWorkManifest({ id: "draft-1", title: "草稿", profileId: "short-fiction", language: "zh", now: "2026-11-01T00:00:00.000Z" });
    await saveWorkManifest(root, draft);
    const later = WorkManifestSchema.parse({
      ...JSON.parse(GOLDEN_WORK_MANIFEST_JSON),
      updatedAt: "2026-11-05T00:00:00.000Z",
    });
    await saveWorkManifest(root, later);
    const list = await listWorkManifests(root);
    expect(list.map((work) => work.id)).toEqual(["walk-diff", "draft-1"]);
    const shorts = await listWorkManifests(root, "short-fiction");
    expect(shorts).toHaveLength(2);
  });

  it("episode + events 行形状与 listEpisodes 过滤（node:sqlite ↔ rusqlite 同 schema 同参）", () => {
    const store = new CreativeEpisodeStore(join(root, ".inkos", "harness.sqlite"));
    try {
      store.create({
        version: 2,
        id: "ep-walkdiff-1",
        workId: "walk-diff",
        profileId: "short-fiction",
        status: "running",
        startedAt: "2026-11-03T00:00:00.000Z",
        completedAt: null,
      });
      store.append({
        episodeId: "ep-walkdiff-1",
        type: "stage.completed",
        workId: "walk-diff",
        capabilityId: "short-fiction",
        actionId: "outline-draft",
        payload: { stage: "outline" },
      }, "2026-11-03T00:00:30.000Z");
      store.append({
        episodeId: "ep-walkdiff-1",
        type: "review.completed",
        workId: "walk-diff",
        capabilityId: "short-fiction",
        payload: { score: 91 },
      }, "2026-11-03T00:00:50.000Z");
      store.finish("ep-walkdiff-1", "completed", "2026-11-03T00:01:00.000Z");
      const detail = store.requireEpisode("ep-walkdiff-1");
      expect(detail.status).toBe("completed");
      const events = store.listEvents("ep-walkdiff-1");
      expect(events.map((event) => event.type)).toEqual(["stage.completed", "review.completed"]);
      expect(events[1]?.seq).toBe(1);
      const listed = store.listEpisodes({ workId: "walk-diff", limit: 50 });
      expect(listed.map((episode) => episode.id)).toEqual(["ep-walkdiff-1"]);
    } finally {
      store.close();
    }
  });

  it("金标准 rev2 文本 checksum 锚定（防金标准自身漂移）", async () => {
    const { createHash } = await import("node:crypto");
    const checksum = `sha256:${createHash("sha256").update(GOLDEN_REV2_TEXT).digest("hex")}`;
    expect(checksum).toBe(
      "sha256:f0d2e9e9414e33c22c84474ba8902291818bf01a4089bbc5245315d2c16782d6",
    );
  });
});
