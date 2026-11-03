import {
  CreativeEpisodeStore,
  StoryGraphSchema,
  createWorkManifest,
  migrateLegacyWorks,
  saveStoryGraph,
  saveWorkManifest,
  stageArtifactRevision,
} from "@actalk/inkos-core";
import { mkdir, rm, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const dir = fileURLToPath(new URL(".", import.meta.url));
export const E2E_ROOT = resolve(dir, "../../../..", "test-project");

export const A11Y_BOOK = "e2e-a11y-book";
export const A11Y_FILM = "e2e-a11y-film";
export const A11Y_WORK = "e2e-a11y-work";
export const A11Y_PROFILE = "short-fiction";

/**
 * 877 号：a11y 全路由审计的页面数据。每个参数化路由都要有真实可渲染的实体，
 * 否则页面只渲染空态/错误态，审计覆盖不到真实控件（873 号教训：手写错 hash
 * 回落首页时审计照样「通过」）。
 *
 * - 书：TS 2.0 canonical 形状（book.json + story_bible + 下划线章文件 + index.json，
 *   与 walkthrough-fixture 的 TS 腿播种同形）——书页/设定/时间线/分析/章节/真相六页
 * - 影游：saveStoryGraph（seed-analysis 同款）——play/film/flow/film-author/film-studio
 * - work：core 的 createWorkManifest + stageArtifactRevision（双版本，r1 superseded /
 *   r2 current）+ CreativeEpisodeStore 一条 episode——work/work-chat（875 号同构）
 *
 * 幂等：书与影游覆写同内容；work 目录先删后建（stageArtifactRevision 是追加语义，
 * 重复播种会叠版本）；episode 按 id 存在即跳过。
 */
export async function seedA11y(): Promise<void> {
  await seedBook();
  await seedFilm();
  await seedWork();
}

async function seedBook(): Promise<void> {
  // 2.0 的书即 longform-novel work：/books 走 listWorkManifests，书目录在
  // works/<id>/source。legacy books/<id> 布局写好后经 core 的 migrateLegacyWorks
  // 注册——与 walkthrough-fixture TS 腿（697 号）同一真源路径，不手写 work 清单。
  // 幂等：先删已迁移的 work 再迁（迁移对已存在且来源相同的 work 会跳过，旧内容
  // 不会刷新）。
  await rm(join(E2E_ROOT, "works", A11Y_BOOK), { recursive: true, force: true });
  const bookDir = join(E2E_ROOT, "books", A11Y_BOOK);
  await mkdir(join(bookDir, "story"), { recursive: true });
  await mkdir(join(bookDir, "chapters"), { recursive: true });
  await writeFile(join(bookDir, "book.json"), JSON.stringify({
    id: A11Y_BOOK, title: "可访问性审计样书", platform: "qidian", genre: "xuanhuan", status: "active",
    targetChapters: 12, chapterWordCount: 3000, language: "zh",
    createdAt: "2026-11-03T00:00:00Z", updatedAt: "2026-11-03T00:00:00Z", version: 1,
  }));
  await writeFile(join(bookDir, "story", "story_bible.md"), "# story_bible\n\n- 镜中世界反向修行：镜外越弱，镜内越强。\n");
  await writeFile(join(bookDir, "story", "pending_hooks.md"), [
    "# 伏笔池",
    "",
    "| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 升级 | 备注 | 分类 |",
    "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    "| H01 | 1 | 身世 | open | 1 | 第10章 | slow-burn | 无 | 第一卷 | 是 | 10 | 是 | 镜中世界的来历真相。 | 悬念 |",
    "",
  ].join("\n"));
  await writeFile(join(bookDir, "story", "chapter_summaries.md"), [
    "# chapter_summaries",
    "",
    "| 章 | 标题 | 出场人物 | 关键事件 | 状态变化 | 伏笔动态 | 情绪基调 | 章节类型 | 冲突 | 揭示 |",
    "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    "| 1 | 开端 | 苏檀 | 苏檀坠入镜中世界。 | 主角进入镜界 | H01 埋设 | 紧张 | 开局章 | 6 | 4 |",
    "",
  ].join("\n"));
  await writeFile(join(bookDir, "story", "current_state.md"), "# current_state\n\n- 苏檀位于镜宗外门。\n");
  const chapterBody = "# 第1章 开端\n\n苏檀睁开眼，头顶是一面巨大的铜镜。\n\n碎镜贴在胸口，微微发烫。\n";
  await writeFile(join(bookDir, "chapters", "0001_开端.md"), chapterBody);
  await writeFile(join(bookDir, "chapters", "index.json"), JSON.stringify([{
    number: 1,
    title: "开端",
    wordCount: chapterBody.replace(/\s+/g, "").length,
    createdAt: "2026-11-03T00:00:00.000Z",
    updatedAt: "2026-11-03T00:00:00.000Z",
    provenance: "imported",
  }], null, 2));
  const results = await migrateLegacyWorks(E2E_ROOT, { apply: true, source: `books/${A11Y_BOOK}` });
  const failed = results.filter((item) => item.status !== "migrated" && item.status !== "already-migrated");
  if (failed.length > 0) {
    throw new Error(`a11y 样书迁移失败：${JSON.stringify(failed)}`);
  }
}

async function seedFilm(): Promise<void> {
  await saveStoryGraph(E2E_ROOT, A11Y_FILM, StoryGraphSchema.parse({
    schemaVersion: 1,
    projectId: A11Y_FILM,
    title: "可访问性审计影游",
    worldAnchor: {
      storyCore: "苏檀在镜中世界寻找归途。",
      theme: "自我与本我的对峙",
      genre: "玄幻",
      worldRules: "镜内伤势与镜外互换。",
      durationMinutes: 15,
    },
    characters: [
      { id: "c_sutan", name: "苏檀", role: "protagonist", motivation: "找回打碎的归乡镜。" },
    ],
    variables: [{ name: "trust", type: "counter", default: 0, desc: "对镜影的信任度。" }],
    nodes: [
      {
        id: "n1", title: "倒悬之镜", type: "start", sceneDesc: "苏檀坠入镜面深处。",
        dialogue: [{ speaker: "苏檀", text: "这是哪里？", emotion: "紧张" }],
        choices: [
          { id: "c1", text: "触摸镜面", targetNodeId: "n2", effects: [{ var: "trust", op: "add", value: 1 }] },
          { id: "c2", text: "后退探路", targetNodeId: "n3", effects: [] },
        ],
      },
      {
        id: "n2", title: "镜影低语", type: "branch", sceneDesc: "镜影提出交易。",
        dialogue: [{ speaker: "镜影", text: "把名字给我。", emotion: "坚定" }],
        choices: [{ id: "c3", text: "拒绝并夺镜钥", targetNodeId: "n3", effects: [] }],
      },
      { id: "n3", title: "镜渊归途", type: "ending", sceneDesc: "镜钥开启归途。", dialogue: [], choices: [] },
    ],
    endings: [{ id: "e1", nodeId: "n3", title: "携钥而归", type: "good", description: "苏檀握镜钥踏上归途。" }],
  }));
}

async function seedWork(): Promise<void> {
  await rm(join(E2E_ROOT, "works", A11Y_WORK), { recursive: true, force: true });
  let manifest = createWorkManifest({
    id: A11Y_WORK,
    title: "可访问性审计作品",
    profileId: A11Y_PROFILE,
    language: "zh",
    status: "active",
    now: "2026-11-02T00:00:00.000Z",
  });
  await saveWorkManifest(E2E_ROOT, manifest);
  for (const [revisionId, content, createdAt] of [
    ["r1", "# 短篇大纲 v1\n\n雨夜来客叩门，碎镜斋主以镜换名。\n", "2026-11-02T00:00:00.000Z"],
    ["r2", "# 短篇大纲 v2\n\n雨夜来客叩门；斋主窥见镜中人影与自己同貌。\n", "2026-11-03T00:00:00.000Z"],
  ] as const) {
    ({ manifest } = await stageArtifactRevision({
      projectRoot: E2E_ROOT,
      manifest,
      artifactId: "outline",
      artifactKind: "outline",
      revisionId,
      content,
      contentType: "text/markdown",
      fileName: "outline.md",
      createdAt,
      promote: true,
    }));
  }

  const store = new CreativeEpisodeStore(join(E2E_ROOT, ".inkos", "harness.sqlite"));
  const episodeId = "ep-e2e-a11y-1";
  try {
    if (!store.getEpisode(episodeId)) {
      store.create({
        version: 2, id: episodeId, workId: A11Y_WORK, profileId: A11Y_PROFILE,
        status: "running", startedAt: "2026-11-03T00:00:00.000Z", completedAt: null,
      });
      store.append({
        episodeId, type: "stage.completed", workId: A11Y_WORK,
        capabilityId: A11Y_PROFILE, payload: { stage: "outline" },
      }, "2026-11-03T00:00:30.000Z");
      store.finish(episodeId, "completed", "2026-11-03T00:01:00.000Z");
    }
  } finally {
    store.close();
  }
}
