import { describe, expect, it } from "vitest";

import {
  buildChatToolCatalog,
  buildChatToolSet,
  isProductionMutationToolName,
  type ChatToolSetParams,
} from "../agent/chat-tool-set.js";
import type { PipelineRunner } from "../pipeline/runner.js";
import { createUseSkillTool } from "../agent/skill-tool.js";

// 555 号：聊天工具装配统一出口（544 号施工图 §2.2 TS 出口）+ debug/tools
// 目录投影（Rust interaction/registry.rs debug 投影对偶面）。

/** Rust 注册表 31 唯一名（registry.rs 声明序；差分器 rust-only=空集硬要求的镜像）。 */
const CROSS_END_CATALOG_NAMES = [
  "set_world_anchor",
  "upsert_characters",
  "add_variable",
  "define_ending",
  "fill_node",
  "revise_node",
  "generate_node_image",
  "propose_action",
  "research_web",
  "import_chapters",
  "sub_agent",
  "use_skill",
  "manage_book_reference",
  "write_truth_file",
  "rename_entity",
  "patch_chapter_text",
  "replace_chapter_text",
  "delete_latest_chapter",
  "generate_cover",
  "resync_chapter_state",
  "create_narrative_forecast",
  "get_narrative_forecast",
  "select_narrative_branch",
  "play_step",
  "play_revise",
  "play_edit",
  "read",
  "ls",
  "grep",
  "ingest_material",
  "retrieve_material",
];

/** node-only 设计内不对称件（Rust 走 propose→confirm + 端点，agent_route.rs 备案）。 */
const NODE_ONLY_CATALOG_NAMES = [
  "fanfic_create",
  "continuation_import",
  "spinoff_create",
  "imitation_create",
  "short_fiction_run",
  "translation_create",
  "script_create",
  "storyboard_create",
  "interactive_film_create",
  "play_start",
  "draft_structure",
  "connect_choice",
  "remove_node",
  // R37（559 号）：自扩展技能写入件（Rust 侧备案）。
  "author_skill",
];

function baseParams(overrides: Partial<ChatToolSetParams> = {}): ChatToolSetParams {
  return {
    pipeline: {} as PipelineRunner,
    bookId: "book-a",
    sessionId: "session-a",
    sessionKind: "book",
    actionSource: "free-text",
    requestedIntent: undefined,
    actionPayload: undefined,
    projectRoot: "/tmp/inkos-chat-tool-set-test",
    allowSystemFileRead: false,
    language: "zh",
    playWorldExists: false,
    ...overrides,
  };
}

describe("buildChatToolCatalog", () => {
  it("所有工厂桩依赖可实例化且条目形状完整（构造期安全回归锁）", () => {
    const catalog = buildChatToolCatalog();
    expect(catalog.length).toBe(CROSS_END_CATALOG_NAMES.length + NODE_ONLY_CATALOG_NAMES.length);
    for (const entry of catalog) {
      expect(entry.name, "目录名非空").toBeTruthy();
      expect(entry.description, `${entry.name} 描述非空`).toBeTruthy();
      expect(entry.parametersSha256, `${entry.name} 哈希 64hex`).toMatch(/^[0-9a-f]{64}$/);
    }
  });

  it("目录名单 = 跨端 31 名（Rust 声明序）+ node-only 14 名，无重复", () => {
    const catalog = buildChatToolCatalog();
    const names = catalog.map((entry) => entry.name);
    expect(names.slice(0, CROSS_END_CATALOG_NAMES.length)).toEqual(CROSS_END_CATALOG_NAMES);
    expect(names.slice(CROSS_END_CATALOG_NAMES.length)).toEqual(NODE_ONLY_CATALOG_NAMES);
    expect(new Set(names).size).toBe(names.length);
  });

  it("同名条目哈希稳定（canonical JSON 键序与 serde BTreeMap 同构）", () => {
    const [first] = buildChatToolCatalog();
    const [second] = buildChatToolCatalog();
    expect(second?.parametersSha256).toBe(first?.parametersSha256);
  });

  it("read 条目取书作用域系统读关闭分支（目录变体裁决）", () => {
    const read = buildChatToolCatalog().find((entry) => entry.name === "read");
    expect(read?.description).toBe("Read a file from the book directory. Path is relative to books/.");
  });
});

describe("buildChatToolSet（agent-session 原位搬移，行为零改动）", () => {
  it("book 会话装配书工具全表", () => {
    const tools = buildChatToolSet(baseParams());
    const names = tools.map((tool) => tool.name);
    expect(names).toContain("sub_agent");
    expect(names).toContain("write_truth_file");
    expect(names).toContain("read");
    expect(names).toContain("ls");
    expect(names).toContain("grep");
    expect(names).toContain("resync_chapter_state");
  });

  it("chat 会话装配通用五件 + author_skill", () => {
    const tools = buildChatToolSet(baseParams({ sessionKind: "chat", bookId: null }));
    expect(tools.map((tool) => tool.name)).toEqual([
      "propose_action",
      "research_web",
      "ingest_material",
      "retrieve_material",
      "import_chapters",
      // R37：author_skill 全模式常驻（buildChatToolSet 统一追加）。
      "author_skill",
    ]);
  });

  it("edit 会话剔除规划面七件", () => {
    const tools = buildChatToolSet(baseParams({ sessionKind: "edit" }));
    const names = tools.map((tool) => tool.name);
    expect(names).not.toContain("sub_agent");
    expect(names).not.toContain("create_narrative_forecast");
    expect(names).toContain("patch_chapter_text");
  });

  it("intentSkillTool 在场时追加会话尾位", () => {
    const intentSkillTool = createUseSkillTool({ registry: { resolveSkills: () => ({ skills: [], disabledSkillIds: [] }) } as never });
    const tools = buildChatToolSet(baseParams({ intentSkillTool }));
    expect(tools[tools.length - 1]?.name).toBe("use_skill");
  });
});

describe("isProductionMutationToolName", () => {
  it("生产变更面十三件命中，读面/材料/propose 不命中", () => {
    for (const name of ["sub_agent", "generate_cover", "write_truth_file", "rename_entity", "patch_chapter_text", "replace_chapter_text", "resync_chapter_state", "delete_latest_chapter", "import_chapters", "fanfic_create", "continuation_import", "spinoff_create", "imitation_create"]) {
      expect(isProductionMutationToolName(name), name).toBe(true);
    }
    for (const name of ["read", "ls", "grep", "research_web", "ingest_material", "retrieve_material", "propose_action", "use_skill"]) {
      expect(isProductionMutationToolName(name), name).toBe(false);
    }
  });
});
