import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
// 1015 号：测试凭据二轮治理——config 写入值走 env 注入+非凭据回退（855 惯用形二轮）。
const TEST_KEY = process.env.INKOS_TEST_LLM_KEY ?? "test-key-placeholder";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { BookConfigSchema, ProjectConfigSchema, StateManager } from "../index.js";
import { DetectionConfigSchema } from "../models/project.js";

/**
 * 709 号：写键自由面回归锁——Rust BookConfig（engine-rs models/book.rs）与
 * Rust WritingConfig（models/project.rs）镜像的可选键（writing/seriesId/series/
 * governance / project writing）复位进 strict schema。此前 best-of-n（701）/
 * auto-beats、series-id（707）/Rust 引擎（review-mode PUT）把这些键落盘后，
 * strict 根拒收使 loadBookConfig/loadProjectConfig ZodError——整本书/项目在
 * TS 回退腿不可加载的潜伏自毁键。本套件锁定：写面落盘键在位时双加载器不崩。
 */
describe("book/project config writing keys tolerance (709)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "config-writing-keys-"));
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const baseBook = {
    id: "b1",
    title: "镜花缘",
    platform: "qidian",
    genre: "xuanhuan",
    status: "active",
    targetChapters: 12,
    chapterWordCount: 3000,
    language: "zh",
    createdAt: "2026-10-16T00:00:00.000Z",
    updatedAt: "2026-10-16T00:00:00.000Z",
  };

  it("BookConfigSchema：writing/seriesId/series/governance 可选键 roundtrip（Rust BookConfig 镜像）", () => {
    const full = BookConfigSchema.parse({
      ...baseBook,
      seriesId: "jing_hua_yuan",
      series: { name: "镜花缘系列", order: 2 },
      writing: { reviewMode: "manual", revisionGate: "strict", autoTimelineBeats: true, sceneBeats: false },
      governance: { policy: "quality-first", maxConsecutiveDebts: 3, bestOfN: { enabled: true, candidates: 3, minScore: 80 } },
    });
    expect(full.writing?.reviewMode).toBe("manual");
    expect(full.writing?.autoTimelineBeats).toBe(true);
    expect(full.seriesId).toBe("jing_hua_yuan");
    expect(full.series?.order).toBe(2);
    expect(full.governance?.bestOfN?.minScore).toBe(80);

    // 非法子值仍拒：reviewMode 三态外值、candidates 越界、maxConsecutiveDebts 越界
    expect(BookConfigSchema.safeParse({ ...baseBook, writing: { reviewMode: "yolo" } }).success).toBe(false);
    expect(BookConfigSchema.safeParse({ ...baseBook, governance: { bestOfN: { candidates: 9 } } }).success).toBe(false);
    expect(BookConfigSchema.safeParse({ ...baseBook, governance: { maxConsecutiveDebts: 99 } }).success).toBe(false);
  });

  it("StateManager.loadBookConfig：写面落盘键在位时加载不崩（701/707 潜伏自毁键清偿锁）", async () => {
    const state = new StateManager(root);
    // 2.0 StateManager 正典布局 = works/{id}/source（bookDir）；books/ 为
    // legacy 保留树（697 号迁移边界工项管）——本锁钉正典加载器。
    const bookDir = join(root, "works", "b1", "source");
    await mkdir(bookDir, { recursive: true });
    await writeFile(join(bookDir, "book.json"), JSON.stringify({
      ...baseBook,
      seriesId: "jing_hua_yuan",
      writing: { autoTimelineBeats: true },
      governance: { bestOfN: { enabled: true, candidates: 2, minScore: 70 } },
    }), "utf-8");
    const loaded = await state.loadBookConfig("b1");
    expect(loaded.id).toBe("b1");
    expect(loaded.writing?.autoTimelineBeats).toBe(true);
    expect(loaded.governance?.bestOfN?.enabled).toBe(true);
  });

  it("ProjectConfigSchema：writing 段（reviewRetries/reviewMode/revisionGate）合法容忍——Rust 引擎落盘键不再毒化 TS 回退腿", () => {
    const config = ProjectConfigSchema.parse({
      name: "inkos-contract-diff",
      version: "0.1.0",
      llm: { provider: "custom", baseUrl: "http://127.0.0.1:9/v1", apiKey: TEST_KEY, model: "m" },
      researchSearch: { enabled: false, provider: "tavily", allowPrivateEgress: false },
      writing: { reviewRetries: 2, reviewMode: "manual", revisionGate: "always" },
    });
    expect(config.writing?.reviewRetries).toBe(2);
    expect(config.writing?.reviewMode).toBe("manual");

    // 非法子值仍拒
    expect(ProjectConfigSchema.safeParse({
      name: "x", version: "0.1.0",
      llm: { provider: "custom", baseUrl: "http://x/v1", apiKey: TEST_KEY, model: "m" },
      researchSearch: { enabled: false, provider: "tavily", allowPrivateEgress: false },
      writing: { reviewMode: "yolo" },
    }).success).toBe(false);

    // 缺省省略语义不变：无 writing 段照常解析
    expect(ProjectConfigSchema.safeParse({
      name: "x", version: "0.1.0",
      llm: { provider: "custom", baseUrl: "http://x/v1", apiKey: TEST_KEY, model: "m" },
      researchSearch: { enabled: false, provider: "tavily", allowPrivateEgress: false },
    }).success).toBe(true);
  });

  it("ProjectConfigSchema：inputGovernanceMode 遗留键容忍——686 删管线遗留不再毒化 TS 腿（1429）", () => {
    // 样例 = 仓根实采 inkos.json 键形（8/6 起在位，值 "v2"；1429 活体实证：
    // TS dev:server 对该文件 ZodError 拒启、Rust 引擎同文件 200）。
    // 契约面：Rust project_config_routes.rs 头注备案「双端同为未知键忽略」，
    // 本腿把 TS 侧拉齐该契约——strict 根设计不变，仅此遗留键显式容忍。
    const config = ProjectConfigSchema.parse({
      name: "inkosDesktopforRust",
      version: "0.1.0",
      language: "zh",
      llm: { provider: "custom", baseUrl: "http://x/v1", apiKey: TEST_KEY, model: "m", configSource: "studio", apiFormat: "chat", stream: true },
      notify: [],
      inputGovernanceMode: "v2",
      daemon: { schedule: { radarCron: "0 */6 * * *", writeCron: "*/15 * * * *" }, maxConcurrentBooks: 3 },
      researchSearch: { enabled: false, provider: "tavily", allowPrivateEgress: false },
    });
    // 键值原样透传（z.unknown 不冻结已死管线的历史值域）
    expect(config.inputGovernanceMode).toBe("v2");

    // 值域全容忍：legacy 项目里可能是任意历史值（非字符串也放行）
    expect(ProjectConfigSchema.safeParse({
      name: "x", version: "0.1.0",
      llm: { provider: "custom", baseUrl: "http://x/v1", apiKey: TEST_KEY, model: "m" },
      researchSearch: { enabled: false, provider: "tavily", allowPrivateEgress: false },
      inputGovernanceMode: 1,
    }).success).toBe(true);

    // 缺省省略语义不变；其余未知键仍拒（strict 根护栏不松动）
    const base = {
      name: "x", version: "0.1.0",
      llm: { provider: "custom", baseUrl: "http://x/v1", apiKey: TEST_KEY, model: "m" },
      researchSearch: { enabled: false, provider: "tavily", allowPrivateEgress: false },
    };
    expect(ProjectConfigSchema.safeParse(base).success).toBe(true);
    expect(ProjectConfigSchema.safeParse({ ...base, someFutureLegacyKey: true }).success).toBe(false);
  });

  // 1602 号：系统性键面差集清偿——Rust ProjectConfig 序列化树与 TS strict
  // schema 全对照（models/project.rs 为真源）后发现三处「Rust 反序列化面接受 /
  // TS strict 拒」残留：顶层 foundation（Rust 无 skip_serializing_if 必带）、
  // daemon.qualityGates（Rust 必序列化，TS 2.0 strict 化时随段裁掉只剩注释
  // 化石）、detection 四键（threshold/autoRewrite/maxRetries/protectedNames，
  // G4/340 专名表等）。schema 向 Rust 模型收敛（709 先例），strict 根与
  // 未列入键的护栏不松动。
  it("ProjectConfigSchema：foundation 段（Rust FoundationConfig 镜像）容忍且缺省不变", () => {
    const base = {
      name: "x", version: "0.1.0",
      llm: { provider: "custom", baseUrl: "http://x/v1", apiKey: TEST_KEY, model: "m" },
      researchSearch: { enabled: false, provider: "tavily", allowPrivateEgress: false },
    };
    const config = ProjectConfigSchema.parse({ ...base, foundation: { reviewRetries: 2 } });
    expect(config.foundation?.reviewRetries).toBe(2);

    // 非法子值仍拒；缺省省略语义不变
    expect(ProjectConfigSchema.safeParse({ ...base, foundation: { reviewRetries: -1 } }).success).toBe(false);
    expect(ProjectConfigSchema.safeParse(base).success).toBe(true);
  });

  it("ProjectConfigSchema：daemon.qualityGates 容忍且缺省投影与 Rust DaemonConfig::default 对齐", () => {
    const base = {
      name: "x", version: "0.1.0",
      llm: { provider: "custom", baseUrl: "http://x/v1", apiKey: TEST_KEY, model: "m" },
      researchSearch: { enabled: false, provider: "tavily", allowPrivateEgress: false },
    };
    // Rust 必序列化形态（qualityGates 全键在位）
    const config = ProjectConfigSchema.parse({
      ...base,
      daemon: {
        schedule: { radarCron: "0 */6 * * *", writeCron: "*/15 * * * *" },
        maxConcurrentBooks: 3,
        qualityGates: { maxAuditRetries: 2, pauseAfterConsecutiveFailures: 3, retryTemperatureStep: 0.1 },
      },
    });
    expect(config.daemon.qualityGates?.maxAuditRetries).toBe(2);
    expect(config.daemon.qualityGates?.retryTemperatureStep).toBe(0.1);

    // 缺 daemon 段时 default 投影含 qualityGates（540 号 {2,3,0.1} 口径，
    // 与 Rust DaemonConfig::default 序列化形状对齐——689 教训：default 字面量
    // 必须与新增键同步，否则缺段投影形状漂移）
    const defaulted = ProjectConfigSchema.parse(base);
    expect(defaulted.daemon.qualityGates).toEqual({
      maxAuditRetries: 2,
      pauseAfterConsecutiveFailures: 3,
      retryTemperatureStep: 0.1,
    });

    // 非法子值仍拒
    expect(ProjectConfigSchema.safeParse({
      ...base,
      daemon: { schedule: { radarCron: "x", writeCron: "y" }, qualityGates: { maxAuditRetries: -1 } },
    }).success).toBe(false);
  });

  it("DetectionConfigSchema：Rust 四键（threshold/autoRewrite/maxRetries/protectedNames）容忍且 PUT 往返保真", () => {
    // Rust DetectionConfig 完整形态（models/project.rs:211，G4/340 专名表）
    const rustShape = {
      provider: "custom",
      apiUrl: "https://detector.example.com/v1",
      apiKeyEnv: "DETECTOR_API_KEY",
      enabled: true,
      threshold: 0.8,
      autoRewrite: true,
      maxRetries: 3,
      protectedNames: ["林动", "青檀"],
    };
    const parsed = DetectionConfigSchema.parse(rustShape);
    // PUT 往返保真（684 纪律：schema 补键后 parsed.data 全键保留）
    expect(parsed).toEqual(rustShape);

    // 非法子值仍拒；strict 键集护栏不松动
    expect(DetectionConfigSchema.safeParse({ ...rustShape, threshold: "high" }).success).toBe(false);
    expect(DetectionConfigSchema.safeParse({ ...rustShape, someFutureKey: 1 }).success).toBe(false);
  });
});
